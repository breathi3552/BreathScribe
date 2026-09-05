use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::managers::model::ModelManager;
use crate::managers::transcription::StreamTextEvent;
use crate::providers::gemini::GeminiProvider;
use crate::providers::local::LocalTranscriptionProvider;
use crate::providers::{
    BatchTranscriptionProvider, StreamTextSink, StreamingSession, StreamingTranscriptionProvider,
    TranscriptionOptions,
};
use crate::settings::{AppSettings, TranscriptionMode};

/// 基于 Tauri AppHandle 的流式文本事件接收器
pub struct TauriStreamTextSink {
    app_handle: AppHandle,
}

impl TauriStreamTextSink {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

impl StreamTextSink for TauriStreamTextSink {
    fn emit_text(&self, committed: String, tentative: String) {
        let _ = StreamTextEvent {
            committed,
            tentative,
        }
        .emit(&self.app_handle);
    }
}

pub struct TranscriptionRouter {
    app_handle: AppHandle,
    local_provider: Arc<LocalTranscriptionProvider>,
    gemini_provider: Arc<GeminiProvider>,
    active_cloud_session: Arc<tokio::sync::Mutex<Option<Box<dyn StreamingSession>>>>,
    has_active_stream: Arc<std::sync::atomic::AtomicBool>,
}

impl TranscriptionRouter {
    pub fn new(
        app_handle: AppHandle,
        local_provider: Arc<LocalTranscriptionProvider>,
        gemini_provider: Arc<GeminiProvider>,
    ) -> Self {
        Self {
            app_handle,
            local_provider,
            gemini_provider,
            active_cloud_session: Arc::new(tokio::sync::Mutex::new(None)),
            has_active_stream: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn local_provider(&self) -> &Arc<LocalTranscriptionProvider> {
        &self.local_provider
    }

    pub fn gemini_provider(&self) -> &Arc<GeminiProvider> {
        &self.gemini_provider
    }

    /// 检查是否有正在运行的云端流式会话
    pub fn has_active_cloud_stream(&self) -> bool {
        self.has_active_stream
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// 启动云端流式会话，并将麦克风采集的音频旁路帧持续泵送至会话
    pub async fn start_cloud_stream(
        &self,
        options: &TranscriptionOptions,
        stream_router: &crate::managers::transcription::StreamRouter,
    ) -> Result<(), String> {
        self.cancel_cloud_stream();

        let session = self.start_streaming(options, None).await?;
        let rx = stream_router.open();

        *self.active_cloud_session.lock().await = Some(session);
        self.has_active_stream
            .store(true, std::sync::atomic::Ordering::Release);

        let active_session = Arc::clone(&self.active_cloud_session);
        let has_stream = Arc::clone(&self.has_active_stream);

        tokio::task::spawn_blocking(move || {
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    crate::managers::transcription::StreamCmd::Feed(samples) => {
                        if !has_stream.load(std::sync::atomic::Ordering::Acquire) {
                            break;
                        }
                        if let Some(session) = active_session.blocking_lock().as_ref() {
                            let _ = session.feed_audio(&samples);
                        }
                    }
                    _ => {
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// 结束云端流式会话并返回最终转写文本
    pub async fn finalize_cloud_stream(&self) -> Option<Result<String, String>> {
        self.has_active_stream
            .store(false, std::sync::atomic::Ordering::Release);
        let session = self.active_cloud_session.lock().await.take()?;
        Some(session.finalize().await)
    }

    /// 取消并释放当前的云端流式会话
    pub fn cancel_cloud_stream(&self) {
        self.has_active_stream
            .store(false, std::sync::atomic::Ordering::Release);
        let active = Arc::clone(&self.active_cloud_session);
        tauri::async_runtime::spawn(async move {
            if let Some(session) = active.lock().await.take() {
                session.cancel().await;
            }
        });
    }

    /// 检查当前设置下的转写模式与模型是否支持实时流式识别
    pub fn is_streaming_supported(&self, settings: &AppSettings) -> bool {
        match &settings.transcription_mode {
            TranscriptionMode::Cloud {
                provider_id,
                model_id,
            } => match provider_id.as_str() {
                "gemini" => self.gemini_provider.supports_streaming(model_id),
                _ => false,
            },
            TranscriptionMode::Local => self
                .app_handle
                .try_state::<Arc<ModelManager>>()
                .and_then(|mm| mm.get_model_info(&settings.selected_model))
                .map(|m| m.supports_streaming)
                .unwrap_or(false),
        }
    }

    /// 开启实时流式转写会话（针对支持流式的云端提供商）
    pub async fn start_streaming(
        &self,
        options: &TranscriptionOptions,
        custom_sink: Option<Arc<dyn StreamTextSink>>,
    ) -> Result<Box<dyn StreamingSession>, String> {
        let settings = crate::settings::get_settings(&self.app_handle);
        match &settings.transcription_mode {
            TranscriptionMode::Cloud {
                provider_id,
                model_id,
            } => match provider_id.as_str() {
                "gemini" => {
                    if !self.gemini_provider.supports_streaming(model_id) {
                        return Err(format!("当前配置的模型 {} 不支持流式转写", model_id));
                    }
                    let sink = custom_sink.unwrap_or_else(|| {
                        Arc::new(TauriStreamTextSink::new(self.app_handle.clone()))
                    });
                    self.gemini_provider.start_stream(options, sink).await
                }
                unknown => Err(format!("云端提供商 {} 不支持流式转写", unknown)),
            },
            TranscriptionMode::Local => {
                Err("本地模型流式由本地 TranscriptionManager 驱动".to_string())
            }
        }
    }

    pub async fn transcribe(
        &self,
        audio: Vec<f32>,
        options: &TranscriptionOptions,
    ) -> Result<String, String> {
        let settings = crate::settings::get_settings(&self.app_handle);
        match &settings.transcription_mode {
            TranscriptionMode::Local => self.local_provider.transcribe(audio, options).await,
            TranscriptionMode::Cloud { provider_id, .. } => match provider_id.as_str() {
                "gemini" => self.gemini_provider.transcribe(audio, options).await,
                unknown => Err(format!("未知的云端转写提供商: {}", unknown)),
            },
        }
    }
}

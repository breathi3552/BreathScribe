use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
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

/// Stream text event sink backed by Tauri AppHandle.
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

enum CloudStreamCtrl {
    Finalize(tokio::sync::oneshot::Sender<Option<Result<String, String>>>),
    Cancel,
}

pub struct TranscriptionRouter {
    app_handle: AppHandle,
    local_provider: Arc<LocalTranscriptionProvider>,
    gemini_provider: Arc<GeminiProvider>,
    active_stream_router: Arc<Mutex<Option<Arc<crate::managers::transcription::StreamRouter>>>>,
    active_stream_ctrl: Arc<Mutex<Option<tokio::sync::mpsc::Sender<CloudStreamCtrl>>>>,
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
            active_stream_router: Arc::new(Mutex::new(None)),
            active_stream_ctrl: Arc::new(Mutex::new(None)),
            has_active_stream: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn local_provider(&self) -> &Arc<LocalTranscriptionProvider> {
        &self.local_provider
    }

    pub fn gemini_provider(&self) -> &Arc<GeminiProvider> {
        &self.gemini_provider
    }

    /// Checks whether a cloud streaming session is active.
    pub fn has_active_cloud_stream(&self) -> bool {
        self.has_active_stream
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Starts a cloud streaming session and pipes microphone audio frames to it.
    pub fn start_cloud_stream(
        &self,
        options: &TranscriptionOptions,
        stream_router: Arc<crate::managers::transcription::StreamRouter>,
    ) {
        self.cancel_cloud_stream();

        let rx = stream_router.open();
        *self.active_stream_router.lock() = Some(Arc::clone(&stream_router));
        self.has_active_stream
            .store(true, std::sync::atomic::Ordering::Release);

        let (ctrl_tx, mut ctrl_rx) = tokio::sync::mpsc::channel::<CloudStreamCtrl>(1);
        *self.active_stream_ctrl.lock() = Some(ctrl_tx);

        let active_router = Arc::clone(&self.active_stream_router);
        let has_stream = Arc::clone(&self.has_active_stream);
        let app_handle = self.app_handle.clone();
        let gemini_provider = Arc::clone(&self.gemini_provider);
        let options_clone = options.clone();

        tauri::async_runtime::spawn(async move {
            let session_res = gemini_provider
                .start_stream(
                    &options_clone,
                    Arc::new(TauriStreamTextSink::new(app_handle)),
                )
                .await;

            let session = match session_res {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("Failed to initiate cloud streaming handshake: {}", e);
                    has_stream.store(false, std::sync::atomic::Ordering::Release);
                    if let Some(r) = active_router.lock().take() {
                        r.clear();
                    }
                    if let Some(cmd) = ctrl_rx.recv().await {
                        if let CloudStreamCtrl::Finalize(reply_tx) = cmd {
                            let _ = reply_tx.send(Some(Err(e)));
                        }
                    }
                    return;
                }
            };

            log::info!("Cloud streaming session ready, streaming audio");

            let session_arc = Arc::new(tokio::sync::Mutex::new(Some(session)));
            let session_feed = Arc::clone(&session_arc);
            let feed_finished = Arc::new(tokio::sync::Notify::new());
            let feed_finished_clone = Arc::clone(&feed_finished);
            let has_stream_feed = Arc::clone(&has_stream);

            tokio::task::spawn_blocking(move || {
                while let Ok(cmd) = rx.recv() {
                    if !has_stream_feed.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    match cmd {
                        crate::managers::transcription::StreamCmd::Feed(samples) => {
                            let guard = session_feed.blocking_lock();
                            if let Some(s) = guard.as_ref() {
                                if let Err(e) = s.feed_audio(&samples) {
                                    log::warn!("Failed to feed audio samples to cloud session: {}", e);
                                    break;
                                }
                            }
                        }
                        _ => break,
                    }
                }
                feed_finished_clone.notify_waiters();
            });

            match ctrl_rx.recv().await {
                Some(CloudStreamCtrl::Finalize(reply_tx)) => {
                    let _ = tokio::time::timeout(Duration::from_secs(3), feed_finished.notified())
                        .await;

                    let maybe_session = session_arc.lock().await.take();
                    if let Some(s) = maybe_session {
                        let res = s.finalize().await;
                        let _ = reply_tx.send(Some(res));
                    } else {
                        let _ = reply_tx.send(None);
                    }
                }
                Some(CloudStreamCtrl::Cancel) | None => {
                    let maybe_session = session_arc.lock().await.take();
                    if let Some(s) = maybe_session {
                        s.cancel().await;
                    }
                }
            }

            has_stream.store(false, std::sync::atomic::Ordering::Release);
        });
    }

    /// Finalizes cloud streaming session and returns the transcribed text.
    pub async fn finalize_cloud_stream(&self) -> Option<Result<String, String>> {
        if let Some(router) = self.active_stream_router.lock().take() {
            router.clear();
        }

        let ctrl_tx = self.active_stream_ctrl.lock().take()?;

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if ctrl_tx
            .send(CloudStreamCtrl::Finalize(reply_tx))
            .await
            .is_err()
        {
            self.has_active_stream
                .store(false, std::sync::atomic::Ordering::Release);
            return None;
        }

        let res = match tokio::time::timeout(Duration::from_secs(8), reply_rx).await {
            Ok(Ok(final_res)) => final_res,
            Ok(Err(_)) => None,
            Err(_) => {
                log::warn!("Cloud stream finalization timed out (8s), falling back to batch mode");
                None
            }
        };

        self.has_active_stream
            .store(false, std::sync::atomic::Ordering::Release);
        res
    }

    /// Cancels the active cloud streaming session.
    pub fn cancel_cloud_stream(&self) {
        self.has_active_stream
            .store(false, std::sync::atomic::Ordering::Release);
        if let Some(router) = self.active_stream_router.lock().take() {
            router.clear();
        }
        if let Some(ctrl_tx) = self.active_stream_ctrl.lock().take() {
            tauri::async_runtime::spawn(async move {
                let _ = ctrl_tx.send(CloudStreamCtrl::Cancel).await;
            });
        }
    }

    /// Checks whether current settings support streaming transcription.
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

    /// Starts a real-time streaming transcription session.
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
                        return Err(format!("Model {} does not support streaming", model_id));
                    }
                    let sink = custom_sink.unwrap_or_else(|| {
                        Arc::new(TauriStreamTextSink::new(self.app_handle.clone()))
                    });
                    self.gemini_provider.start_stream(options, sink).await
                }
                unknown => Err(format!("Cloud provider {} does not support streaming", unknown)),
            },
            TranscriptionMode::Local => {
                Err("Local model streaming is handled by TranscriptionManager".to_string())
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
                unknown => Err(format!("Unknown cloud STT provider: {}", unknown)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::managers::transcription::{StreamCmd, StreamRouter};

    #[test]
    fn test_stream_router_pre_buffering_behavior() {
        let router = StreamRouter::new();

        router.feed(&[1.0, 2.0]);
        assert!(!router.is_open());

        let rx = router.open();
        assert!(router.is_open());

        let frame1 = vec![0.1f32; 1600];
        let frame2 = vec![0.2f32; 1600];
        let frame3 = vec![0.3f32; 1600];

        router.feed(&frame1);
        router.feed(&frame2);
        router.feed(&frame3);

        let received1 = rx.recv().expect("frame 1 should be buffered");
        if let StreamCmd::Feed(samples) = received1 {
            assert_eq!(samples.len(), 1600);
            assert_eq!(samples[0], 0.1f32);
        } else {
            panic!("Expected StreamCmd::Feed");
        }

        let received2 = rx.recv().expect("frame 2 should be buffered");
        if let StreamCmd::Feed(samples) = received2 {
            assert_eq!(samples.len(), 1600);
            assert_eq!(samples[0], 0.2f32);
        } else {
            panic!("Expected StreamCmd::Feed");
        }

        let received3 = rx.recv().expect("frame 3 should be buffered");
        if let StreamCmd::Feed(samples) = received3 {
            assert_eq!(samples.len(), 1600);
            assert_eq!(samples[0], 0.3f32);
        } else {
            panic!("Expected StreamCmd::Feed");
        }

        let _ = router.take();
        assert!(!router.is_open());
        router.feed(&[9.9]);
        assert!(rx.try_recv().is_err());
    }
}

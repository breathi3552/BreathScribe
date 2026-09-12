use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;
use tauri_specta::Event;
use tokio::sync::{oneshot, watch};
use tokio::task::{AbortHandle, JoinHandle};

use crate::managers::transcription::{StreamCmd, StreamRouter, StreamTextEvent};
use crate::providers::{
    BatchTranscriptionProvider, StreamTextSink, StreamingTranscriptionProvider,
    TranscriptionOptions,
};
use crate::settings::TranscriptionMode;

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

/// 每次录音独有的输出许可；撤销与正在交付的文本互斥。
struct CloudOutput {
    sink: Mutex<Option<Arc<dyn StreamTextSink>>>,
    cancelled: watch::Sender<bool>,
}

impl CloudOutput {
    fn close(&self) {
        self.sink.lock().take();
    }

    fn cancel(&self) {
        self.close();
        self.cancelled.send_replace(true);
    }
}

impl StreamTextSink for CloudOutput {
    fn emit_text(&self, committed: String, tentative: String) {
        if let Some(sink) = self.sink.lock().as_ref() {
            sink.emit_text(committed, tentative);
        }
    }
}

struct CloudStream {
    route: Option<Arc<StreamRouter>>,
    output: Arc<CloudOutput>,
    task: Option<JoinHandle<Result<String, String>>>,
    abort: AbortHandle,
    finish: Option<oneshot::Sender<()>>,
}

impl Drop for CloudStream {
    fn drop(&mut self) {
        self.output.cancel();
        self.abort.abort();
        if let Some(route) = self.route.take() {
            route.clear();
        }
    }
}

/// 即使调用方丢弃结束 future，也不能留下仍在输出的任务。
struct FinishGuard {
    output: Arc<CloudOutput>,
    abort: AbortHandle,
}

impl Drop for FinishGuard {
    fn drop(&mut self) {
        self.output.cancel();
        self.abort.abort();
    }
}

pub struct TranscriptionRouter {
    local_provider: Arc<dyn BatchTranscriptionProvider>,
    cloud_batch: Arc<dyn BatchTranscriptionProvider>,
    cloud_streaming: Arc<dyn StreamingTranscriptionProvider>,
    text_sink: Arc<dyn StreamTextSink>,
    cloud_stream: Mutex<Option<CloudStream>>,
}

impl TranscriptionRouter {
    /// 使用现有 Provider 与文本输出适配器构造转写路由。
    pub fn new<P>(
        local_provider: Arc<dyn BatchTranscriptionProvider>,
        cloud_provider: Arc<P>,
        text_sink: Arc<dyn StreamTextSink>,
    ) -> Self
    where
        P: BatchTranscriptionProvider + StreamingTranscriptionProvider + 'static,
    {
        Self {
            local_provider,
            cloud_batch: cloud_provider.clone(),
            cloud_streaming: cloud_provider,
            text_sink,
            cloud_stream: Mutex::new(None),
        }
    }

    /// 同步打开音频入口，在网络握手期间保留首段音频。
    pub fn start_cloud_stream(&self, options: &TranscriptionOptions, route: Arc<StreamRouter>) {
        let mut active = self.cloud_stream.lock();
        // 旧入口必须在新入口打开前关闭；后台任务无权访问该槽位。
        active.take();
        let rx = route.open();
        let (cancelled, _) = watch::channel(false);
        let output = Arc::new(CloudOutput {
            sink: Mutex::new(Some(self.text_sink.clone())),
            cancelled,
        });
        let provider = self.cloud_streaming.clone();
        let sink = output.clone();
        let options = options.clone();
        let (finish, finished) = oneshot::channel();
        let runtime = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| tauri::async_runtime::handle().inner().clone());
        let task = runtime.spawn(async move {
            let session = provider.start_stream(&options, sink.clone()).await?;
            // 阻塞接收器只拥有本次 session 和 rx，绝不清理共享音频入口。
            let feeder = tokio::task::spawn_blocking(move || {
                while let Ok(StreamCmd::Feed(samples)) = rx.recv() {
                    if sink.sink.lock().is_none() {
                        return Err("Cloud recording cancelled".to_string());
                    }
                    session.feed_audio(&samples)?;
                }
                Ok::<_, String>(session)
            });
            finished
                .await
                .map_err(|_| "Cloud recording cancelled".to_string())?;
            let session = tokio::time::timeout(Duration::from_millis(500), feeder)
                .await
                .map_err(|_| "Cloud audio drain timed out (500ms)".to_string())?
                .map_err(|e| format!("Cloud audio worker failed: {e}"))??;
            session.finalize().await
        });
        let abort = task.abort_handle();
        *active = Some(CloudStream {
            route: Some(route),
            output,
            task: Some(task),
            abort,
            finish: Some(finish),
        });
    }

    /// 完成本次云端录音；有效 Live 文本优先，其余结果仅回退云端批量路径。
    pub async fn finish_cloud_stream(
        &self,
        audio: Vec<f32>,
        options: &TranscriptionOptions,
        mode: &TranscriptionMode,
    ) -> Result<String, String> {
        if audio.is_empty() {
            self.cancel_cloud_stream();
            return Ok(String::new());
        }
        let pending = {
            let mut active = self.cloud_stream.lock();
            match active.as_mut() {
                Some(stream) => {
                    let task = stream
                        .task
                        .take()
                        .ok_or_else(|| "Cloud recording is already finishing".to_string())?;
                    if let Some(route) = stream.route.take() {
                        route.clear();
                    }
                    if let Some(finish) = stream.finish.take() {
                        let _ = finish.send(());
                    }
                    Some((
                        task,
                        FinishGuard {
                            output: stream.output.clone(),
                            abort: stream.abort.clone(),
                        },
                    ))
                }
                None => None,
            }
        };
        let Some((mut task, guard)) = pending else {
            return self.transcribe_cloud(audio, options, mode).await;
        };
        let preserve_live_error = matches!(
            mode,
            TranscriptionMode::Cloud {
                provider_id,
                model_id,
            } if self.supports_cloud_streaming(provider_id, model_id)
        );
        let mut cancelled = guard.output.cancelled.subscribe();
        let result = if *cancelled.borrow() {
            Err("Cloud recording cancelled".to_string())
        } else {
            tokio::select! {
                biased;
                _ = cancelled.changed() => Err("Cloud recording cancelled".to_string()),
                result = async {
                    let live = tokio::time::timeout(Duration::from_secs(8), &mut task).await;
                    // 超时与错误也撤销输出许可，晚到文本不能覆盖批量结果。
                    guard.output.close();
                    guard.abort.abort();
                    match live {
                        Ok(Ok(Ok(text))) if !text.trim().is_empty() => Ok(text),
                        Ok(Ok(Err(error))) if preserve_live_error => {
                            log::warn!(
                                "Cloud Live streaming failed; preserving the error instead of falling back to batch mode"
                            );
                            Err(error)
                        }
                        Ok(Err(error)) if preserve_live_error => {
                            log::warn!(
                                "Cloud Live streaming worker failed; preserving the error instead of falling back to batch mode"
                            );
                            Err(format!("Cloud streaming worker failed: {error}"))
                        }
                        Err(_) if preserve_live_error => {
                            log::warn!(
                                "Cloud Live stream finalization timed out (8s); preserving the error instead of falling back to batch mode"
                            );
                            Err("Cloud stream finalization timed out (8s)".to_string())
                        }
                        other => {
                            match other {
                                Err(_) => log::warn!("Cloud stream finalization timed out (8s), falling back to batch mode"),
                                Ok(Ok(Err(e))) => log::warn!("Cloud streaming failed ({e}), falling back to batch mode"),
                                Ok(Err(e)) => log::warn!("Cloud streaming worker failed ({e}), falling back to batch mode"),
                                _ => log::debug!("Cloud stream produced no text, falling back to batch mode"),
                            }
                            self.transcribe_cloud(audio, options, mode).await
                        }
                    }
                } => result,
            }
        };
        let mut active = self.cloud_stream.lock();
        if active
            .as_ref()
            .is_some_and(|stream| Arc::ptr_eq(&stream.output, &guard.output))
        {
            let was_cancelled = *guard.output.cancelled.borrow();
            active.take();
            if was_cancelled {
                return Err("Cloud recording cancelled".to_string());
            }
            result
        } else {
            Err("Cloud recording cancelled".to_string())
        }
    }

    /// 撤销当前录音及其输出，下一次开始无需等待旧网络操作退出。
    pub fn cancel_cloud_stream(&self) {
        self.cloud_stream.lock().take();
    }

    /// 查询指定云端模型的流式能力。
    pub fn supports_cloud_streaming(&self, provider_id: &str, model_id: &str) -> bool {
        provider_id == self.cloud_batch.provider_id()
            && self.cloud_streaming.supports_streaming(model_id)
    }

    /// 批量转写入口，历史重试不会消费当前录音的流式会话。
    pub async fn transcribe(
        &self,
        audio: Vec<f32>,
        options: &TranscriptionOptions,
        mode: &TranscriptionMode,
    ) -> Result<String, String> {
        match mode {
            TranscriptionMode::Local => self.local_provider.transcribe(audio, options).await,
            TranscriptionMode::Cloud { .. } => self.transcribe_cloud(audio, options, mode).await,
        }
    }

    async fn transcribe_cloud(
        &self,
        audio: Vec<f32>,
        options: &TranscriptionOptions,
        mode: &TranscriptionMode,
    ) -> Result<String, String> {
        match mode {
            TranscriptionMode::Cloud { provider_id, .. }
                if provider_id == self.cloud_batch.provider_id() =>
            {
                self.cloud_batch.transcribe(audio, options).await
            }
            TranscriptionMode::Cloud { provider_id, .. } => {
                Err(format!("Unknown cloud STT provider: {provider_id}"))
            }
            TranscriptionMode::Local => {
                Err("Cloud recording requires a cloud provider".to_string())
            }
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

#[cfg(test)]
mod lifecycle_tests;

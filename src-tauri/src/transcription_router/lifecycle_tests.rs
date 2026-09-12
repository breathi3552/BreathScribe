use super::*;
use crate::managers::transcription::StreamRouter;
use crate::providers::StreamingSession;
use std::collections::VecDeque;
use tokio::sync::oneshot;

type SessionResult = Result<Box<dyn StreamingSession>, String>;

struct Handshake {
    entered: oneshot::Sender<Arc<dyn StreamTextSink>>,
    release: oneshot::Receiver<SessionResult>,
}

#[derive(Default)]
struct ControlledProvider {
    handshakes: Mutex<VecDeque<Handshake>>,
    batch_failure: Mutex<Option<String>>,
    batch_audio: Mutex<Vec<Vec<f32>>>,
}

impl ControlledProvider {
    fn handshake(
        &self,
    ) -> (
        oneshot::Receiver<Arc<dyn StreamTextSink>>,
        oneshot::Sender<SessionResult>,
    ) {
        let (entered, started) = oneshot::channel();
        let (release, wait) = oneshot::channel();
        self.handshakes.lock().push_back(Handshake {
            entered,
            release: wait,
        });
        (started, release)
    }
}

#[async_trait::async_trait]
impl StreamingTranscriptionProvider for ControlledProvider {
    fn supports_streaming(&self, _: &str) -> bool {
        true
    }

    async fn start_stream(
        &self,
        _: &TranscriptionOptions,
        sink: Arc<dyn StreamTextSink>,
    ) -> SessionResult {
        let handshake = self
            .handshakes
            .lock()
            .pop_front()
            .expect("planned handshake");
        let _ = handshake.entered.send(sink);
        handshake.release.await.expect("release handshake")
    }
}

#[async_trait::async_trait]
impl BatchTranscriptionProvider for ControlledProvider {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        _: &TranscriptionOptions,
    ) -> Result<String, String> {
        self.batch_audio.lock().push(audio);
        match self.batch_failure.lock().as_ref() {
            Some(error) => Err(error.clone()),
            None => Ok("batch transcript".into()),
        }
    }

    fn provider_id(&self) -> &'static str {
        "gemini"
    }
}

struct NoLocal;

#[async_trait::async_trait]
impl BatchTranscriptionProvider for NoLocal {
    async fn transcribe(&self, _: Vec<f32>, _: &TranscriptionOptions) -> Result<String, String> {
        panic!("cloud recording must never load a local model")
    }
    fn provider_id(&self) -> &'static str {
        "local"
    }
}

#[derive(Default)]
struct TextOutput(Mutex<Vec<String>>);
impl StreamTextSink for TextOutput {
    fn emit_text(&self, committed: String, tentative: String) {
        self.0.lock().push(format!("{committed}{tentative}"));
    }
}

struct LiveSession {
    text: String,
    audio: Arc<Mutex<Vec<f32>>>,
}

#[async_trait::async_trait]
impl StreamingSession for LiveSession {
    fn feed_audio(&self, samples: &[f32]) -> Result<(), String> {
        self.audio.lock().extend_from_slice(samples);
        Ok(())
    }
    async fn finalize(self: Box<Self>) -> Result<String, String> {
        Ok(self.text)
    }
    async fn cancel(self: Box<Self>) {}
}

fn options() -> TranscriptionOptions {
    TranscriptionOptions {
        language: "en".into(),
        prompt: None,
    }
}

fn mode() -> TranscriptionMode {
    TranscriptionMode::Cloud {
        provider_id: "gemini".into(),
        model_id: "live".into(),
    }
}

#[tokio::test]
async fn cancelled_handshake_cannot_deliver_text_into_next_recording() {
    let provider = Arc::new(ControlledProvider::default());
    let output = Arc::new(TextOutput::default());
    let router = TranscriptionRouter::new(Arc::new(NoLocal), provider.clone(), output.clone());
    let audio_route = Arc::new(StreamRouter::new());
    let (old_started, old_release) = provider.handshake();
    router.start_cloud_stream(&options(), audio_route.clone());
    let old_sink = old_started.await.unwrap();
    router.cancel_cloud_stream();

    let (new_started, new_release) = provider.handshake();
    router.start_cloud_stream(&options(), audio_route.clone());
    let new_sink = new_started.await.unwrap();
    old_sink.emit_text("cancelled words".into(), String::new());
    new_sink.emit_text("new words".into(), String::new());
    assert_eq!(
        *output.0.lock(),
        ["new words"],
        "cancelled Live text leaked into new recording"
    );

    let _ = old_release.send(Err("late handshake failure".into()));
    let received = Arc::new(Mutex::new(Vec::new()));
    audio_route.feed(&[0.1, 0.2]);
    assert!(new_release
        .send(Ok(Box::new(LiveSession {
            text: "new words".into(),
            audio: received.clone()
        })))
        .is_ok());
    assert_eq!(
        router
            .finish_cloud_stream(vec![0.1, 0.2], &options(), &mode())
            .await
            .unwrap(),
        "new words"
    );
    assert_eq!(*received.lock(), [0.1, 0.2]);
    assert!(
        provider.batch_audio.lock().is_empty(),
        "valid Live text must not trigger batch"
    );
}

async fn complete_next_recording(
    router: &TranscriptionRouter,
    provider: &ControlledProvider,
    route: &Arc<StreamRouter>,
) {
    let (started, release) = provider.handshake();
    router.start_cloud_stream(&options(), route.clone());
    let sink = started.await.unwrap();
    route.feed(&[1.0]);
    route.feed(&[2.0, 3.0]);
    let received = Arc::new(Mutex::new(Vec::new()));
    assert!(release
        .send(Ok(Box::new(LiveSession {
            text: "next recording".into(),
            audio: received.clone(),
        })))
        .is_ok());
    route.feed(&[4.0]);
    sink.emit_text("next recording".into(), String::new());
    assert_eq!(
        router
            .finish_cloud_stream(vec![1.0, 2.0, 3.0, 4.0], &options(), &mode())
            .await
            .unwrap(),
        "next recording"
    );
    assert_eq!(
        *received.lock(),
        [1.0, 2.0, 3.0, 4.0],
        "all prebuffered and tail frames must arrive in FIFO order"
    );
}

#[tokio::test]
async fn handshake_failure_finishes_via_cloud_batch_and_allows_next_recording() {
    let provider = Arc::new(ControlledProvider::default());
    let output = Arc::new(TextOutput::default());
    let router = TranscriptionRouter::new(Arc::new(NoLocal), provider.clone(), output.clone());
    let route = Arc::new(StreamRouter::new());
    let (started, release) = provider.handshake();
    router.start_cloud_stream(&options(), route.clone());
    let failed_sink = started.await.unwrap();
    assert!(release.send(Err("handshake refused".into())).is_ok());
    assert_eq!(
        router
            .finish_cloud_stream(vec![0.25], &options(), &mode())
            .await
            .unwrap(),
        "batch transcript"
    );
    assert_eq!(*provider.batch_audio.lock(), [vec![0.25]]);
    router.cancel_cloud_stream();
    complete_next_recording(&router, &provider, &route).await;
    failed_sink.emit_text("late failure output".into(), String::new());
    assert_eq!(*output.0.lock(), ["next recording"]);
}

#[tokio::test(start_paused = true)]
async fn handshake_timeout_keeps_eight_second_budget_and_next_recording_works() {
    let provider = Arc::new(ControlledProvider::default());
    let output = Arc::new(TextOutput::default());
    let router = TranscriptionRouter::new(Arc::new(NoLocal), provider.clone(), output.clone());
    let route = Arc::new(StreamRouter::new());
    let (started, mut release) = provider.handshake();
    router.start_cloud_stream(&options(), route.clone());
    let old_sink = started.await.unwrap();
    let start = tokio::time::Instant::now();
    assert_eq!(
        router
            .finish_cloud_stream(vec![0.5], &options(), &mode())
            .await
            .unwrap(),
        "batch transcript"
    );
    assert_eq!(start.elapsed(), Duration::from_secs(8));
    release.closed().await;
    tokio::time::resume();
    complete_next_recording(&router, &provider, &route).await;
    old_sink.emit_text("timed out words".into(), String::new());
    assert_eq!(*output.0.lock(), ["next recording"]);
    assert_eq!(*provider.batch_audio.lock(), [vec![0.5]]);
}

struct BlockedFinalize {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<Result<String, String>>,
}

#[async_trait::async_trait]
impl StreamingSession for BlockedFinalize {
    fn feed_audio(&self, _: &[f32]) -> Result<(), String> {
        Ok(())
    }
    async fn finalize(self: Box<Self>) -> Result<String, String> {
        let _ = self.entered.send(());
        self.release.await.expect("release finalization")
    }
    async fn cancel(self: Box<Self>) {}
}

#[tokio::test]
async fn cancelled_finalization_cannot_return_old_text_or_consume_new_recording() {
    let provider = Arc::new(ControlledProvider::default());
    let output = Arc::new(TextOutput::default());
    let router = Arc::new(TranscriptionRouter::new(
        Arc::new(NoLocal),
        provider.clone(),
        output.clone(),
    ));
    let route = Arc::new(StreamRouter::new());
    let (started, release) = provider.handshake();
    router.start_cloud_stream(&options(), route.clone());
    let old_sink = started.await.unwrap();
    let (entered, finalizing) = oneshot::channel();
    let (mut finish_release, wait) = oneshot::channel();
    assert!(release
        .send(Ok(Box::new(BlockedFinalize {
            entered,
            release: wait
        })))
        .is_ok());
    let finishing_router = router.clone();
    let finishing = tokio::spawn(async move {
        finishing_router
            .finish_cloud_stream(vec![0.5], &options(), &mode())
            .await
    });
    finalizing.await.unwrap();
    router.cancel_cloud_stream();
    complete_next_recording(&router, &provider, &route).await;
    old_sink.emit_text("cancelled result".into(), String::new());
    finish_release.closed().await;
    assert!(
        finishing.await.unwrap().is_err(),
        "cancelled finalization must fail rather than deliver or fall back"
    );
    assert_eq!(*output.0.lock(), ["next recording"]);
    assert!(provider.batch_audio.lock().is_empty());
}

#[tokio::test]
async fn empty_audio_cancels_pending_handshake_without_batch_request() {
    let provider = Arc::new(ControlledProvider::default());
    let router = TranscriptionRouter::new(
        Arc::new(NoLocal),
        provider.clone(),
        Arc::new(TextOutput::default()),
    );
    let route = Arc::new(StreamRouter::new());
    let (started, mut release) = provider.handshake();
    router.start_cloud_stream(&options(), route.clone());
    let _sink = started.await.unwrap();
    assert_eq!(
        router
            .finish_cloud_stream(Vec::new(), &options(), &mode())
            .await
            .unwrap(),
        ""
    );
    release.closed().await;
    complete_next_recording(&router, &provider, &route).await;
    assert!(provider.batch_audio.lock().is_empty());
}

struct FailedStream;

#[async_trait::async_trait]
impl StreamingSession for FailedStream {
    fn feed_audio(&self, _: &[f32]) -> Result<(), String> {
        Err("stream disconnected".into())
    }
    async fn finalize(self: Box<Self>) -> Result<String, String> {
        Err("stream disconnected".into())
    }
    async fn cancel(self: Box<Self>) {}
}

#[tokio::test]
async fn missing_empty_and_failed_live_use_only_cloud_batch_and_preserve_errors() {
    let provider = Arc::new(ControlledProvider::default());
    let router = TranscriptionRouter::new(
        Arc::new(NoLocal),
        provider.clone(),
        Arc::new(TextOutput::default()),
    );
    let route = Arc::new(StreamRouter::new());
    assert_eq!(
        router
            .finish_cloud_stream(vec![1.0], &options(), &mode())
            .await
            .unwrap(),
        "batch transcript"
    );

    let (started, release) = provider.handshake();
    router.start_cloud_stream(&options(), route.clone());
    let _sink = started.await.unwrap();
    assert!(release
        .send(Ok(Box::new(LiveSession {
            text: "  ".into(),
            audio: Arc::default()
        })))
        .is_ok());
    assert_eq!(
        router
            .finish_cloud_stream(vec![2.0], &options(), &mode())
            .await
            .unwrap(),
        "batch transcript"
    );

    *provider.batch_failure.lock() = Some("cloud batch unavailable".into());
    let (started, release) = provider.handshake();
    router.start_cloud_stream(&options(), route.clone());
    let _sink = started.await.unwrap();
    route.feed(&[3.0]);
    assert!(release.send(Ok(Box::new(FailedStream))).is_ok());
    assert_eq!(
        router
            .finish_cloud_stream(vec![3.0], &options(), &mode())
            .await
            .unwrap_err(),
        "cloud batch unavailable"
    );
    assert_eq!(
        *provider.batch_audio.lock(),
        [vec![1.0], vec![2.0], vec![3.0]]
    );
}

#[tokio::test]
async fn history_batch_retry_does_not_consume_current_live_recording() {
    let provider = Arc::new(ControlledProvider::default());
    let router = TranscriptionRouter::new(
        Arc::new(NoLocal),
        provider.clone(),
        Arc::new(TextOutput::default()),
    );
    let route = Arc::new(StreamRouter::new());
    let (started, release) = provider.handshake();
    router.start_cloud_stream(&options(), route.clone());
    let _sink = started.await.unwrap();
    assert_eq!(
        router
            .transcribe(vec![9.0], &options(), &mode())
            .await
            .unwrap(),
        "batch transcript"
    );
    route.feed(&[1.0]);
    assert!(release
        .send(Ok(Box::new(LiveSession {
            text: "current recording".into(),
            audio: Arc::default()
        })))
        .is_ok());
    assert_eq!(
        router
            .finish_cloud_stream(vec![1.0], &options(), &mode())
            .await
            .unwrap(),
        "current recording"
    );
    assert_eq!(*provider.batch_audio.lock(), [vec![9.0]]);
}

#[tokio::test]
async fn stalled_finalization_is_reclaimed_at_deadline_before_next_recording() {
    let provider = Arc::new(ControlledProvider::default());
    let output = Arc::new(TextOutput::default());
    let router = Arc::new(TranscriptionRouter::new(
        Arc::new(NoLocal),
        provider.clone(),
        output.clone(),
    ));
    let route = Arc::new(StreamRouter::new());
    let (started, release) = provider.handshake();
    router.start_cloud_stream(&options(), route.clone());
    let old_sink = started.await.unwrap();
    let (entered, finalizing) = oneshot::channel();
    let (mut finish_release, wait) = oneshot::channel();
    assert!(release
        .send(Ok(Box::new(BlockedFinalize {
            entered,
            release: wait
        })))
        .is_ok());
    let finishing_router = router.clone();
    let finishing = tokio::spawn(async move {
        finishing_router
            .finish_cloud_stream(vec![0.75], &options(), &mode())
            .await
    });
    finalizing.await.unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(8)).await;
    assert_eq!(finishing.await.unwrap().unwrap(), "batch transcript");
    finish_release.closed().await;
    tokio::time::resume();
    complete_next_recording(&router, &provider, &route).await;
    old_sink.emit_text("late final result".into(), String::new());
    assert_eq!(*output.0.lock(), ["next recording"]);
    assert_eq!(*provider.batch_audio.lock(), [vec![0.75]]);
}

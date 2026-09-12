use super::*;
use crate::managers::transcription::StreamRouter;
use crate::network::NetworkManager;
use crate::providers::gemini::{GeminiProvider, GEMINI_LIVE_MODEL_ID, SAMPLES_PER_CHUNK};
use crate::providers::StreamingSession;
use crate::settings::{ProxyMode, TranscriptionMode};
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};

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

fn batch_mode() -> TranscriptionMode {
    TranscriptionMode::Cloud {
        provider_id: "gemini".into(),
        model_id: "batch".into(),
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
async fn handshake_failure_returns_live_error_without_cloud_batch_fallback() {
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
            .unwrap_err(),
        "handshake refused"
    );
    assert!(
        provider.batch_audio.lock().is_empty(),
        "Live connection failure must not be masked by cloud batch"
    );
    router.cancel_cloud_stream();
    complete_next_recording(&router, &provider, &route).await;
    failed_sink.emit_text("late failure output".into(), String::new());
    assert_eq!(*output.0.lock(), ["next recording"]);
}

#[tokio::test(start_paused = true)]
async fn handshake_timeout_returns_live_error_without_batch_fallback() {
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
            .unwrap_err(),
        "Cloud stream finalization timed out (8s)"
    );
    assert_eq!(start.elapsed(), Duration::from_secs(8));
    release.closed().await;
    tokio::time::resume();
    complete_next_recording(&router, &provider, &route).await;
    old_sink.emit_text("timed out words".into(), String::new());
    assert_eq!(*output.0.lock(), ["next recording"]);
    assert!(
        provider.batch_audio.lock().is_empty(),
        "Live timeout must not be masked by cloud batch"
    );
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
async fn empty_live_result_falls_back_but_live_failure_preserves_error() {
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
        "stream disconnected"
    );
    assert_eq!(*provider.batch_audio.lock(), [vec![2.0]]);
}

#[tokio::test]
async fn non_streaming_cloud_mode_still_uses_batch_without_live_session() {
    let provider = Arc::new(ControlledProvider::default());
    let router = TranscriptionRouter::new(
        Arc::new(NoLocal),
        provider.clone(),
        Arc::new(TextOutput::default()),
    );

    assert_eq!(
        router
            .finish_cloud_stream(vec![1.0], &options(), &batch_mode())
            .await
            .unwrap(),
        "batch transcript"
    );
    assert_eq!(*provider.batch_audio.lock(), [vec![1.0]]);
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
    assert_eq!(
        finishing.await.unwrap().unwrap_err(),
        "Cloud stream finalization timed out (8s)"
    );
    finish_release.closed().await;
    tokio::time::resume();
    complete_next_recording(&router, &provider, &route).await;
    old_sink.emit_text("late final result".into(), String::new());
    assert_eq!(*output.0.lock(), ["next recording"]);
    assert!(
        provider.batch_audio.lock().is_empty(),
        "Live finalization timeout must not be masked by cloud batch"
    );
}

#[derive(Clone, Copy, Debug)]
enum LocalEndpointAction {
    Close,
    Eof,
    FullResult,
    EmptyResult,
    KeepOpen,
}

struct LocalGeminiEndpoint {
    base_url: String,
    setup_seen: Option<oneshot::Receiver<()>>,
    action: Option<oneshot::Sender<LocalEndpointAction>>,
    stop: Option<oneshot::Sender<()>>,
    batch_requests: Arc<AtomicUsize>,
    task: Option<JoinHandle<Result<(), String>>>,
}

impl LocalGeminiEndpoint {
    async fn wait_for_setup(&mut self) {
        self.setup_seen
            .take()
            .expect("setup receiver should be available")
            .await
            .expect("local endpoint should observe the setup frame");
    }

    fn release(&mut self, action: LocalEndpointAction) {
        self.action
            .take()
            .expect("endpoint action should be available")
            .send(action)
            .expect("local endpoint should still be waiting for its action");
    }

    fn batch_request_count(&self) -> usize {
        self.batch_requests.load(Ordering::SeqCst)
    }

    async fn shutdown(mut self) -> Result<(), String> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.task
            .take()
            .expect("endpoint task should be available")
            .await
            .map_err(|error| format!("local endpoint task failed: {error}"))?
    }
}

impl Drop for LocalGeminiEndpoint {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn read_http_headers(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut request = Vec::with_capacity(2048);
    let mut buffer = [0u8; 1024];
    loop {
        let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer))
            .await
            .map_err(|_| "local HTTP request header timed out".to_string())?
            .map_err(|error| format!("local HTTP request read failed: {error}"))?;
        if read == 0 {
            return Err("local HTTP peer closed before sending headers".to_string());
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(header_end) = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| position + 4)
        {
            let header_text = String::from_utf8_lossy(&request[..header_end]);
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().ok())
                        .flatten()
                })
                .unwrap_or(0usize);
            let mut remaining = content_length.saturating_sub(request.len() - header_end);
            while remaining > 0 {
                let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer))
                    .await
                    .map_err(|_| "local HTTP request body timed out".to_string())?
                    .map_err(|error| format!("local HTTP request body read failed: {error}"))?;
                if read == 0 {
                    return Err("local HTTP peer closed before sending its body".to_string());
                }
                remaining = remaining.saturating_sub(read);
            }
            return Ok(request);
        }
        if request.len() > 64 * 1024 {
            return Err("local HTTP request headers exceeded 64KB".to_string());
        }
    }
}

async fn serve_batch_requests(
    listener: TcpListener,
    mut stop: oneshot::Receiver<()>,
    batch_requests: Arc<AtomicUsize>,
) -> Result<(), String> {
    loop {
        tokio::select! {
            _ = &mut stop => return Ok(()),
            accepted = listener.accept() => {
                let (mut stream, _) = accepted
                    .map_err(|error| format!("local batch endpoint accept failed: {error}"))?;
                let request = read_http_headers(&mut stream).await?;
                if request.starts_with(b"POST ") {
                    batch_requests.fetch_add(1, Ordering::SeqCst);
                }
                let body = br#"{"status":"completed","steps":[]}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .map_err(|error| format!("local batch response header failed: {error}"))?;
                stream
                    .write_all(body)
                    .await
                    .map_err(|error| format!("local batch response body failed: {error}"))?;
            }
        }
    }
}

async fn wait_for_live_shutdown(
    websocket: &mut WebSocketStream<TcpStream>,
    stop: &mut oneshot::Receiver<()>,
) -> bool {
    loop {
        tokio::select! {
            _ = &mut *stop => return true,
            message = websocket.next() => match message {
                Some(Ok(Message::Text(text))) if text.contains("audioStreamEnd") => return false,
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return false,
                Some(_) => {}
            }
        }
    }
}

async fn run_local_gemini_endpoint(
    listener: TcpListener,
    setup_seen: oneshot::Sender<()>,
    action: oneshot::Receiver<LocalEndpointAction>,
    mut stop: oneshot::Receiver<()>,
    batch_requests: Arc<AtomicUsize>,
) -> Result<(), String> {
    let (stream, _) = listener
        .accept()
        .await
        .map_err(|error| format!("local Live endpoint accept failed: {error}"))?;
    let mut websocket = accept_async(stream)
        .await
        .map_err(|error| format!("local Live endpoint handshake failed: {error}"))?;

    let setup = tokio::time::timeout(Duration::from_secs(2), websocket.next())
        .await
        .map_err(|_| "local Live endpoint timed out waiting for setup".to_string())?
        .ok_or_else(|| "local Live endpoint received no setup frame".to_string())?
        .map_err(|error| format!("local Live endpoint setup read failed: {error}"))?;
    match setup {
        Message::Text(text) if text.contains("setup") => {}
        _ => return Err("local Live endpoint received an invalid setup frame".to_string()),
    }
    websocket
        .send(Message::Text(r#"{"setupComplete":{}}"#.into()))
        .await
        .map_err(|error| format!("local Live setup response failed: {error}"))?;
    setup_seen
        .send(())
        .map_err(|_| "setup observer was dropped".to_string())?;

    let stopped = match action
        .await
        .map_err(|_| "local Live endpoint action was dropped".to_string())?
    {
        LocalEndpointAction::Close => {
            websocket
                .send(Message::Text(
                    r#"{"serverContent":{"inputTranscription":{"text":"partial live transcript"}}}"#.into(),
                ))
                .await
                .map_err(|error| format!("local Live partial result failed: {error}"))?;
            websocket
                .send(Message::Close(None))
                .await
                .map_err(|error| format!("local Live close frame failed: {error}"))?;
            false
        }
        LocalEndpointAction::Eof => {
            drop(websocket);
            false
        }
        LocalEndpointAction::FullResult => {
            websocket
                .send(Message::Text(
                    r#"{"serverContent":{"inputTranscription":{"text":"real live transcript"},"turnComplete":true}}"#.into(),
                ))
                .await
                .map_err(|error| format!("local Live result failed: {error}"))?;
            drop(websocket);
            false
        }
        LocalEndpointAction::EmptyResult => {
            websocket
                .send(Message::Text(
                    r#"{"serverContent":{"turnComplete":true}}"#.into(),
                ))
                .await
                .map_err(|error| format!("local Live empty result failed: {error}"))?;
            drop(websocket);
            false
        }
        LocalEndpointAction::KeepOpen => wait_for_live_shutdown(&mut websocket, &mut stop).await,
    };

    if stopped {
        return Ok(());
    }
    serve_batch_requests(listener, stop, batch_requests).await
}

async fn spawn_local_gemini_endpoint() -> LocalGeminiEndpoint {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local Live endpoint should bind");
    let address = listener
        .local_addr()
        .expect("local Live endpoint should have an address");
    let (setup_tx, setup_rx) = oneshot::channel();
    let (action_tx, action_rx) = oneshot::channel();
    let (stop_tx, stop_rx) = oneshot::channel();
    let batch_requests = Arc::new(AtomicUsize::new(0));
    let task = tokio::spawn(run_local_gemini_endpoint(
        listener,
        setup_tx,
        action_rx,
        stop_rx,
        Arc::clone(&batch_requests),
    ));

    LocalGeminiEndpoint {
        base_url: format!("http://{address}"),
        setup_seen: Some(setup_rx),
        action: Some(action_tx),
        stop: Some(stop_tx),
        batch_requests,
        task: Some(task),
    }
}

fn real_gemini_router(endpoint: &str) -> (Arc<GeminiProvider>, TranscriptionRouter) {
    let network = Arc::new(
        NetworkManager::new(crate::settings::ProxySettings {
            mode: ProxyMode::Direct,
            ..Default::default()
        })
        .expect("direct test network should build"),
    );
    let provider = Arc::new(GeminiProvider::new_for_test(
        network,
        "local-gemini-test-key",
        endpoint,
    ));
    let router = TranscriptionRouter::new(
        Arc::new(NoLocal),
        provider.clone(),
        Arc::new(TextOutput::default()),
    );
    (provider, router)
}

fn real_gemini_mode() -> TranscriptionMode {
    TranscriptionMode::Cloud {
        provider_id: "gemini".to_string(),
        model_id: GEMINI_LIVE_MODEL_ID.to_string(),
    }
}

#[tokio::test]
async fn real_gemini_live_failures_reach_router_without_batch_fallback() {
    for action in [
        LocalEndpointAction::Close,
        LocalEndpointAction::Eof,
        LocalEndpointAction::KeepOpen,
    ] {
        let mut endpoint = spawn_local_gemini_endpoint().await;
        let (provider, router) = real_gemini_router(&endpoint.base_url);
        let route = Arc::new(StreamRouter::new());
        router.start_cloud_stream(&options(), route.clone());
        endpoint.wait_for_setup().await;

        if matches!(action, LocalEndpointAction::KeepOpen) {
            let notification = provider.test_fail_next_live_send();
            let send_failed = notification.notified();
            endpoint.release(action);
            route.feed(&vec![0.0; SAMPLES_PER_CHUNK]);
            tokio::time::timeout(Duration::from_secs(1), send_failed)
                .await
                .expect("controlled Live send failure should be observed");
        } else {
            endpoint.release(action);
        }

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            router.finish_cloud_stream(
                vec![0.0; SAMPLES_PER_CHUNK],
                &options(),
                &real_gemini_mode(),
            ),
        )
        .await
        .expect("Live failure should reach the router promptly");
        assert!(
            result.is_err(),
            "{action:?} must not become a successful transcript"
        );
        assert_eq!(
            endpoint.batch_request_count(),
            0,
            "{action:?} must not trigger a cloud batch fallback"
        );
        endpoint
            .shutdown()
            .await
            .expect("local endpoint should shut down cleanly");
    }
}

#[tokio::test]
async fn real_gemini_live_success_and_empty_result_keep_existing_contracts() {
    for (action, expected, expected_batches) in [
        (LocalEndpointAction::FullResult, "real live transcript", 0),
        (LocalEndpointAction::EmptyResult, "", 1),
    ] {
        let mut endpoint = spawn_local_gemini_endpoint().await;
        let (_provider, router) = real_gemini_router(&endpoint.base_url);
        router.start_cloud_stream(&options(), Arc::new(StreamRouter::new()));
        endpoint.wait_for_setup().await;
        endpoint.release(action);

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            router.finish_cloud_stream(
                vec![0.0; SAMPLES_PER_CHUNK],
                &options(),
                &real_gemini_mode(),
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("{action:?} finalization timed out: {error}"))
        .unwrap_or_else(|error| panic!("{action:?} finalization failed: {error}"));
        assert_eq!(result, expected);
        assert_eq!(endpoint.batch_request_count(), expected_batches);
        endpoint
            .shutdown()
            .await
            .expect("local endpoint should shut down cleanly");
    }
}

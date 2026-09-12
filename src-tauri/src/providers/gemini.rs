use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;
use tokio_tungstenite::WebSocketStream;

use crate::network::NetworkManager;
use crate::providers::{
    BatchTranscriptionProvider, StreamTextSink, StreamingSession, StreamingTranscriptionProvider,
    TranscriptionOptions,
};
use crate::settings::DEFAULT_CLOUD_STT_MODEL_ID;

pub const GEMINI_LIVE_MODEL_ID: &str = "gemini-3.5-transcribe-live";
pub const SAMPLES_PER_CHUNK: usize = 1600; // 100ms at 16kHz

/// Gemini Live client setup frame.
#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiLiveSetupFrame {
    pub setup: GeminiLiveSetupConfig,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiLiveSetupConfig {
    pub model: String,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiLiveGenerationConfig>,
    #[serde(
        rename = "inputAudioTranscription",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_audio_transcription: Option<GeminiLiveInputAudioTranscription>,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiLiveGenerationConfig {
    #[serde(rename = "responseModalities")]
    pub response_modalities: Vec<String>,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiLiveInputAudioTranscription {
    #[serde(rename = "languageCodes")]
    pub language_codes: Vec<String>,
    pub mode: String,
    #[serde(rename = "customVocabulary", skip_serializing_if = "Option::is_none")]
    pub custom_vocabulary: Option<Vec<String>>,
}

/// Gemini Live client realtime audio input frame.
#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiLiveRealtimeInputFrame {
    #[serde(rename = "realtimeInput")]
    pub realtime_input: GeminiLiveRealtimeInput,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiLiveRealtimeInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<GeminiLiveAudioData>,
    #[serde(rename = "audioStreamEnd", skip_serializing_if = "Option::is_none")]
    pub audio_stream_end: Option<bool>,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiLiveAudioData {
    pub data: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

/// Gemini Live server setup complete message.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GeminiLiveSetupComplete {}

/// Gemini Live server inbound message.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct GeminiLiveServerMessage {
    #[serde(rename = "setupComplete")]
    pub setup_complete: Option<GeminiLiveSetupComplete>,
    #[serde(rename = "serverContent")]
    pub server_content: Option<GeminiLiveServerContent>,
    pub error: Option<GeminiLiveError>,
}

impl GeminiLiveServerMessage {
    /// Parses WebSocket text and binary JSON frames.
    pub fn parse(msg: &tokio_tungstenite::tungstenite::Message) -> Option<Self> {
        match msg {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                match serde_json::from_str(text.as_str()) {
                    Ok(parsed) => Some(parsed),
                    Err(e) => {
                        log::warn!("Failed to parse Gemini Live text message: {}", e);
                        None
                    }
                }
            }
            tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
                match serde_json::from_slice(bytes.as_ref()) {
                    Ok(parsed) => Some(parsed),
                    Err(e) => {
                        log::warn!(
                            "Failed to parse Gemini Live binary message ({} bytes): {}",
                            bytes.len(),
                            e
                        );
                        None
                    }
                }
            }
            _ => None,
        }
    }
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct GeminiLiveServerContent {
    #[serde(rename = "interimInputTranscription")]
    pub interim_input_transcription: Option<GeminiLiveTranscriptionText>,
    #[serde(rename = "inputTranscription")]
    pub input_transcription: Option<GeminiLiveTranscriptionText>,
    #[serde(rename = "turnComplete")]
    pub turn_complete: Option<bool>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct GeminiLiveTranscriptionText {
    pub text: Option<String>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct GeminiLiveError {
    pub code: Option<i32>,
    pub message: Option<String>,
}
/// Interactions API request payload.
#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiInteractionRequest {
    pub model: String,
    pub input: Vec<GeminiInteractionInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiInteractionGenerationConfig>,
}

/// Interactions API input part.
#[derive(serde::Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum GeminiInteractionInput {
    #[serde(rename = "audio")]
    Audio { data: String, mime_type: String },
    #[serde(rename = "text")]
    Text { text: String },
}

/// Interactions API generation configuration.
#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiInteractionGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcription_config: Option<GeminiTranscriptionConfig>,
}

/// Interactions API transcription mode.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GeminiTranscriptionMode {
    #[default]
    Smart,
    Verbatim,
}

/// Transcription configuration for speech inputs.
#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiTranscriptionConfig {
    pub language_codes: Vec<String>,
    pub mode: GeminiTranscriptionMode,
}

/// Interactions API response payload.
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct GeminiInteractionResponse {
    pub id: Option<String>,
    pub status: Option<String>,
    pub steps: Option<Vec<GeminiInteractionStep>>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct GeminiInteractionStep {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub step_type: Option<String>,
    pub content: Option<Vec<GeminiInteractionContent>>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct GeminiInteractionContent {
    #[serde(rename = "type")]
    pub content_type: Option<String>,
    pub text: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct GeminiErrorResponse {
    error: Option<GeminiErrorDetail>,
}

#[derive(serde::Deserialize, Debug)]
struct GeminiErrorDetail {
    message: Option<String>,
}

fn safe_error_text(text: &str, secrets: &[&str]) -> String {
    crate::llm_client::redact_sensitive_text(text, secrets)
        .chars()
        .take(512)
        .collect()
}

pub struct GeminiProvider {
    network_manager: Arc<NetworkManager>,
    app_handle: AppHandle,
}

impl GeminiProvider {
    pub fn new(network_manager: Arc<NetworkManager>, app_handle: AppHandle) -> Self {
        Self {
            network_manager,
            app_handle,
        }
    }

    /// Encodes mono audio samples into 16kHz 16-bit mono WAV bytes.
    pub fn encode_wav_in_memory(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| format!("Failed to create in-memory WAV writer: {}", e))?;

        for &sample in samples {
            // 保持 NaN 映射为 -1.0，其余采样限制在 [-1.0, 1.0]。
            let clamped = if sample.is_nan() {
                -1.0
            } else {
                sample.clamp(-1.0, 1.0)
            };
            let scaled = (clamped * 32767.0).round() as i16;
            writer
                .write_sample(scaled)
                .map_err(|e| format!("Failed to write WAV sample: {}", e))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV encoding: {}", e))?;
        Ok(cursor.into_inner())
    }
    /// Checks if a key is a Google OAuth access token.
    pub fn is_oauth_token(key: &str) -> bool {
        key.trim().starts_with("ya29.")
    }

    /// Builds the Interactions API endpoint URL.
    pub fn build_request_url(base_url: &str, api_key: &str) -> String {
        let trimmed_base = base_url.trim().trim_end_matches('/');
        let base = if trimmed_base.is_empty() {
            "https://generativelanguage.googleapis.com"
        } else {
            trimmed_base
        };

        let is_bearer = Self::is_oauth_token(api_key);
        let path = if base.ends_with("/v1beta") {
            "/interactions"
        } else {
            "/v1beta/interactions"
        };

        if is_bearer {
            format!("{base}{path}")
        } else {
            format!("{base}{path}?key={api_key}")
        }
    }

    /// Extracts transcription text from an Interactions API response.
    pub fn extract_text_from_response(body: &GeminiInteractionResponse) -> Result<String, String> {
        if let Some(steps) = &body.steps {
            let mut extracted_texts = Vec::new();
            for step in steps {
                if let Some(contents) = &step.content {
                    for content in contents {
                        if let Some(text) = &content.text {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                extracted_texts.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }

            if !extracted_texts.is_empty() {
                return Ok(extracted_texts.join(" "));
            }
        }

        if let Some(status) = &body.status {
            if status == "completed" {
                return Ok(String::new());
            }
        }

        Err("Gemini Interactions API returned no transcription text".to_string())
    }

    /// Parses Gemini API error response without exposing echoed credentials.
    pub fn parse_api_error(status: reqwest::StatusCode, error_text: &str) -> String {
        Self::parse_api_error_with_secrets(status, error_text, &[])
    }

    fn parse_api_error_with_secrets(
        status: reqwest::StatusCode,
        error_text: &str,
        secrets: &[&str],
    ) -> String {
        if let Ok(response) = serde_json::from_str::<GeminiErrorResponse>(error_text) {
            if let Some(message) = response.error.and_then(|error| error.message) {
                return format!(
                    "Gemini API error (HTTP {}): {}",
                    status,
                    safe_error_text(&message, secrets)
                );
            }
        }

        format!(
            "Gemini API returned error HTTP {}: {}",
            status,
            safe_error_text(error_text, secrets)
        )
    }

    /// Tests connectivity to the Gemini API.
    pub async fn test_connection(
        client: &reqwest::Client,
        api_key: &str,
        custom_base_url: Option<&str>,
    ) -> Result<(), String> {
        let base_url = custom_base_url
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("https://generativelanguage.googleapis.com");
        let clean_base = base_url.trim_end_matches('/');
        let is_bearer = Self::is_oauth_token(api_key);
        let test_url = if clean_base.ends_with("/v1beta") {
            if is_bearer {
                format!("{clean_base}/models")
            } else {
                format!("{clean_base}/models?key={api_key}")
            }
        } else if is_bearer {
            format!("{clean_base}/v1beta/models")
        } else {
            format!("{clean_base}/v1beta/models?key={api_key}")
        };

        let mut req = client.get(&test_url);
        if is_bearer {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        } else {
            req = req.header("x-goog-api-key", api_key);
        }
        let response = req.send().await.map_err(|e| {
            crate::llm_client::report_reqwest_error("Gemini connection request failed", &e)
        })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::parse_api_error_with_secrets(
                status,
                &error_text,
                &[api_key],
            ));
        }

        Ok(())
    }

    /// Converts [-1.0, 1.0] f32 samples to 16-bit mono PCM little-endian bytes.
    pub fn convert_samples_to_pcm16_le(samples: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for &sample in samples {
            // 保持 NaN 映射为 -1.0，其余采样限制在 [-1.0, 1.0]。
            let clamped = if sample.is_nan() {
                -1.0
            } else {
                sample.clamp(-1.0, 1.0)
            };
            let scaled = (clamped * 32767.0).round() as i16;
            bytes.extend_from_slice(&scaled.to_le_bytes());
        }
        bytes
    }

    /// Builds Gemini Live bidirectional WebSocket connection URL.
    pub fn build_live_websocket_url(custom_base_url: Option<&str>, api_key: &str) -> String {
        let base = custom_base_url.map(|s| s.trim()).unwrap_or_default();
        let clean_base = base.trim_end_matches('/');
        let host_and_proto = if clean_base.is_empty() {
            "wss://generativelanguage.googleapis.com".to_string()
        } else if let Some(stripped) = clean_base.strip_prefix("https://") {
            format!("wss://{}", stripped)
        } else if let Some(stripped) = clean_base.strip_prefix("http://") {
            format!("ws://{}", stripped)
        } else if clean_base.starts_with("wss://") || clean_base.starts_with("ws://") {
            clean_base.to_string()
        } else {
            format!("wss://{}", clean_base)
        };

        let path = "/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
        if host_and_proto.contains(path) {
            if host_and_proto.contains('?') {
                format!("{}&key={}", host_and_proto, api_key)
            } else {
                format!("{}?key={}", host_and_proto, api_key)
            }
        } else {
            format!("{}{path}?key={}", host_and_proto, api_key)
        }
    }
}

#[async_trait::async_trait]
impl BatchTranscriptionProvider for GeminiProvider {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        options: &TranscriptionOptions,
    ) -> Result<String, String> {
        let settings = crate::settings::get_settings(&self.app_handle);
        let api_key = settings
            .cloud_stt_api_keys
            .get("gemini")
            .map(|k| k.trim())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| "Gemini API key is not configured".to_string())?;

        let provider_config = settings
            .cloud_stt_providers
            .get("gemini")
            .cloned()
            .unwrap_or_default();

        let raw_model = provider_config.model_id.trim();
        let model = if raw_model.is_empty() || raw_model.contains("transcribe-live") {
            DEFAULT_CLOUD_STT_MODEL_ID
        } else {
            raw_model
        };

        let custom_base = provider_config
            .custom_base_url
            .as_deref()
            .unwrap_or_default();

        let wav_bytes = Self::encode_wav_in_memory(&audio, 16000)?;
        let base64_audio = BASE64.encode(&wav_bytes);

        let request_url = Self::build_request_url(custom_base, api_key);

        let mut inputs = vec![GeminiInteractionInput::Audio {
            data: base64_audio,
            mime_type: "audio/wav".to_string(),
        }];

        if let Some(prompt) = &options.prompt {
            let trimmed_prompt = prompt.trim();
            if !trimmed_prompt.is_empty() {
                inputs.push(GeminiInteractionInput::Text {
                    text: trimmed_prompt.to_string(),
                });
            }
        }

        let mut language_codes = Vec::new();
        if options.language != "auto" && !options.language.trim().is_empty() {
            language_codes.push(options.language.trim().to_string());
        }

        let payload = GeminiInteractionRequest {
            model: model.to_string(),
            input: inputs,
            generation_config: Some(GeminiInteractionGenerationConfig {
                transcription_config: Some(GeminiTranscriptionConfig {
                    language_codes,
                    mode: GeminiTranscriptionMode::Smart,
                }),
            }),
        };

        let client = self.network_manager.client().await;
        let mut req = client.post(&request_url);
        if Self::is_oauth_token(api_key) {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        } else {
            req = req.header("x-goog-api-key", api_key);
        }
        let response = req.json(&payload).send().await.map_err(|e| {
            crate::llm_client::report_reqwest_error("Gemini transcription request failed", &e)
        })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::parse_api_error_with_secrets(
                status,
                &error_text,
                &[api_key],
            ));
        }

        let body: GeminiInteractionResponse = response.json().await.map_err(|e| {
            crate::llm_client::report_reqwest_error("Failed to parse Gemini response JSON", &e)
        })?;

        Self::extract_text_from_response(&body)
    }

    fn provider_id(&self) -> &'static str {
        "gemini"
    }
}

enum SessionCmd {
    Finalize(tokio::sync::oneshot::Sender<Result<String, String>>),
    Cancel,
}

/// Streaming session backed by Gemini Live bidirectional WebSocket.
pub struct GeminiLiveStreamingSession {
    audio_tx: tokio::sync::mpsc::UnboundedSender<Vec<f32>>,
    cmd_tx: tokio::sync::mpsc::Sender<SessionCmd>,
    worker_handle: tokio::task::JoinHandle<()>,
}

impl Drop for GeminiLiveStreamingSession {
    fn drop(&mut self) {
        self.worker_handle.abort();
    }
}

#[async_trait::async_trait]
impl StreamingSession for GeminiLiveStreamingSession {
    fn feed_audio(&self, samples: &[f32]) -> Result<(), String> {
        self.audio_tx
            .send(samples.to_vec())
            .map_err(|e| format!("Failed to feed audio samples to streaming session: {}", e))
    }

    async fn finalize(self: Box<Self>) -> Result<String, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(SessionCmd::Finalize(reply_tx))
            .await
            .map_err(|e| format!("Failed to send finalize command: {}", e))?;
        reply_rx
            .await
            .map_err(|_| "Streaming worker failed to respond to finalize command".to_string())?
    }

    async fn cancel(self: Box<Self>) {
        let _ = self.cmd_tx.send(SessionCmd::Cancel).await;
    }
}

async fn run_gemini_live_worker<S>(
    ws: WebSocketStream<S>,
    mut audio_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>,
    mut cmd_rx: tokio::sync::mpsc::Receiver<SessionCmd>,
    text_sink: Arc<dyn StreamTextSink>,
    api_key: String,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut ws_sink, mut ws_stream) = ws.split();

    struct LiveState {
        committed_text: String,
        tentative_text: String,
        session_error: Option<String>,
        turn_completed: bool,
        finalizing: bool,
        has_received_input_after_finalize: bool,
    }

    let state = Arc::new(parking_lot::Mutex::new(LiveState {
        committed_text: String::new(),
        tentative_text: String::new(),
        session_error: None,
        turn_completed: false,
        finalizing: false,
        has_received_input_after_finalize: false,
    }));
    let turn_notify = Arc::new(tokio::sync::Notify::new());
    let (sink_tx, mut sink_rx) =
        tokio::sync::mpsc::channel::<tokio_tungstenite::tungstenite::Message>(64);
    let mut child_tasks = tokio::task::JoinSet::new();

    let state_receiver = Arc::clone(&state);
    let sink_tx_receiver = sink_tx.clone();
    let turn_notify_receiver = Arc::clone(&turn_notify);
    let text_sink_receiver = Arc::clone(&text_sink);
    let receiver_api_key = api_key.clone();

    let receiver_abort = child_tasks.spawn(async move {
        while let Some(msg_res) = ws_stream.next().await {
            match msg_res {
                Ok(msg) => match msg {
                    tokio_tungstenite::tungstenite::Message::Ping(data) => {
                        let _ = sink_tx_receiver
                            .send(tokio_tungstenite::tungstenite::Message::Pong(data))
                            .await;
                    }
                    tokio_tungstenite::tungstenite::Message::Close(_) => {
                        log::info!("Gemini Live server closed connection");
                        turn_notify_receiver.notify_one();
                        break;
                    }
                    other => {
                        if let Some(server_msg) = GeminiLiveServerMessage::parse(&other) {
                            if let Some(err) = server_msg.error {
                                let err_text = err
                                    .message
                                    .unwrap_or_else(|| "Unknown server error".to_string());
                                let err_text =
                                    safe_error_text(&err_text, &[receiver_api_key.as_str()]);
                                log::warn!("Gemini Live server error: {}", err_text);
                                state_receiver.lock().session_error = Some(err_text);
                                turn_notify_receiver.notify_one();
                            }
                            if let Some(content) = server_msg.server_content {
                                if let Some(interim) = content.interim_input_transcription {
                                    if let Some(t) = interim.text {
                                        let (committed, tentative) = {
                                            let mut s = state_receiver.lock();
                                            s.tentative_text = t.clone();
                                            if s.finalizing {
                                                s.has_received_input_after_finalize = true;
                                            }
                                            (s.committed_text.clone(), t)
                                        };
                                        text_sink_receiver.emit_text(committed, tentative);
                                        turn_notify_receiver.notify_one();
                                    }
                                }
                                if let Some(input) = content.input_transcription {
                                    if let Some(c) = input.text {
                                        let committed = {
                                            let mut s = state_receiver.lock();
                                            s.committed_text.push_str(&c);
                                            s.tentative_text.clear();
                                            if s.finalizing {
                                                s.has_received_input_after_finalize = true;
                                            }
                                            s.committed_text.clone()
                                        };
                                        text_sink_receiver.emit_text(committed, String::new());
                                        turn_notify_receiver.notify_one();
                                    }
                                }
                                if content.turn_complete.unwrap_or(false) {
                                    state_receiver.lock().turn_completed = true;
                                    turn_notify_receiver.notify_one();
                                    break;
                                }
                            }
                        }
                    }
                },
                Err(e) => {
                    let error = safe_error_text(&e.to_string(), &[receiver_api_key.as_str()]);
                    log::warn!("Gemini Live WebSocket receive error: {}", error);
                    state_receiver.lock().session_error =
                        Some(format!("WebSocket receive error: {}", error));
                    turn_notify_receiver.notify_one();
                    break;
                }
            }
        }
        turn_notify_receiver.notify_one();
    });

    let sender_api_key = api_key;
    child_tasks.spawn(async move {
        while let Some(msg) = sink_rx.recv().await {
            if let Err(e) = ws_sink.send(msg).await {
                let error = safe_error_text(&e.to_string(), &[sender_api_key.as_str()]);
                log::warn!("Gemini Live WebSocket send failed: {}", error);
                break;
            }
        }
        let _ = ws_sink.close().await;
    });

    let mut pcm_buffer: Vec<u8> = Vec::with_capacity(SAMPLES_PER_CHUNK * 2);

    loop {
        tokio::select! {
            maybe_samples = audio_rx.recv() => {
                if let Some(samples) = maybe_samples {
                    let pcm_bytes = GeminiProvider::convert_samples_to_pcm16_le(&samples);
                    pcm_buffer.extend_from_slice(&pcm_bytes);

                    let chunk_size = SAMPLES_PER_CHUNK * 2;
                    while pcm_buffer.len() >= chunk_size {
                        let chunk: Vec<u8> = pcm_buffer.drain(..chunk_size).collect();
                        let base64_pcm = BASE64.encode(&chunk);
                        let input_frame = GeminiLiveRealtimeInputFrame {
                            realtime_input: GeminiLiveRealtimeInput {
                                audio: Some(GeminiLiveAudioData {
                                    data: base64_pcm,
                                    mime_type: "audio/pcm;rate=16000".to_string(),
                                }),
                                audio_stream_end: None,
                            },
                        };
                        if let Ok(frame_json) = serde_json::to_string(&input_frame) {
                            if sink_tx
                                .send(tokio_tungstenite::tungstenite::Message::Text(frame_json.into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCmd::Finalize(reply_tx)) => {
                        while let Ok(samples) = audio_rx.try_recv() {
                            let pcm_bytes = GeminiProvider::convert_samples_to_pcm16_le(&samples);
                            pcm_buffer.extend_from_slice(&pcm_bytes);
                        }

                        let had_pending_audio = !pcm_buffer.is_empty();
                        if had_pending_audio {
                            let chunk = std::mem::take(&mut pcm_buffer);
                            let base64_pcm = BASE64.encode(&chunk);
                            let input_frame = GeminiLiveRealtimeInputFrame {
                                realtime_input: GeminiLiveRealtimeInput {
                                    audio: Some(GeminiLiveAudioData {
                                        data: base64_pcm,
                                        mime_type: "audio/pcm;rate=16000".to_string(),
                                    }),
                                    audio_stream_end: None,
                                },
                            };
                            if let Ok(frame_json) = serde_json::to_string(&input_frame) {
                                let _ = sink_tx
                                    .send(tokio_tungstenite::tungstenite::Message::Text(frame_json.into()))
                                    .await;
                            }
                        }

                        let end_frame = GeminiLiveRealtimeInputFrame {
                            realtime_input: GeminiLiveRealtimeInput {
                                audio: None,
                                audio_stream_end: Some(true),
                            },
                        };
                        if let Ok(frame_json) = serde_json::to_string(&end_frame) {
                            let _ = sink_tx
                                .send(tokio_tungstenite::tungstenite::Message::Text(frame_json.into()))
                                .await;
                        }

                        let had_pending_work = {
                            let mut s = state.lock();
                            s.finalizing = true;
                            had_pending_audio
                        };

                        let has_any_text = {
                            let s = state.lock();
                            !s.committed_text.trim().is_empty()
                                || !s.tentative_text.trim().is_empty()
                        };

                        let should_wait = had_pending_work || !has_any_text;

                        if should_wait {
                            let finalize_deadline =
                                tokio::time::Instant::now() + Duration::from_millis(300);
                            while tokio::time::Instant::now() < finalize_deadline {
                                {
                                    let s = state.lock();
                                    if s.turn_completed
                                        || s.session_error.is_some()
                                        || receiver_abort.is_finished()
                                    {
                                        break;
                                    }
                                    if s.has_received_input_after_finalize {
                                        break;
                                    }
                                    if !had_pending_audio
                                        && (!s.committed_text.trim().is_empty()
                                            || !s.tentative_text.trim().is_empty())
                                    {
                                        break;
                                    }
                                }
                                let remaining = finalize_deadline
                                    .saturating_duration_since(tokio::time::Instant::now());
                                if remaining.is_zero() {
                                    break;
                                }
                                let _ =
                                    tokio::time::timeout(remaining, turn_notify.notified()).await;
                            }
                        }

                        receiver_abort.abort();
                        drop(sink_tx);
                        while child_tasks.join_next().await.is_some() {}
                        let final_state = state.lock();
                        let mut result = final_state.committed_text.clone();
                        let tentative = final_state.tentative_text.trim();
                        if !tentative.is_empty() && !result.ends_with(tentative) {
                            result.push_str(tentative);
                        }
                        let trimmed = result.trim().to_string();
                        if !trimmed.is_empty() {
                            let _ = reply_tx.send(Ok(trimmed));
                        } else if let Some(err) = &final_state.session_error {
                            let _ = reply_tx.send(Err(err.clone()));
                        } else {
                            let _ = reply_tx.send(Ok(String::new()));
                        }
                        return;
                    }
                    Some(SessionCmd::Cancel) | None => {
                        receiver_abort.abort();
                        drop(sink_tx);
                        while child_tasks.join_next().await.is_some() {}
                        return;
                    }
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl StreamingTranscriptionProvider for GeminiProvider {
    fn supports_streaming(&self, model: &str) -> bool {
        let trimmed = model.trim();
        trimmed == GEMINI_LIVE_MODEL_ID
            || trimmed == "models/gemini-3.5-transcribe-live"
            || trimmed.ends_with("transcribe-live")
    }

    async fn start_stream(
        &self,
        options: &TranscriptionOptions,
        text_sink: Arc<dyn StreamTextSink>,
    ) -> Result<Box<dyn StreamingSession>, String> {
        let settings = crate::settings::get_settings(&self.app_handle);
        let api_key = settings
            .cloud_stt_api_keys
            .get("gemini")
            .map(|k| k.trim())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| "Gemini API key is not configured".to_string())?;

        let provider_config = settings
            .cloud_stt_providers
            .get("gemini")
            .cloned()
            .unwrap_or_default();

        let custom_base = provider_config.custom_base_url.as_deref();
        let ws_url = Self::build_live_websocket_url(custom_base, api_key);
        let mut ws = self
            .network_manager
            .connect_websocket(&ws_url)
            .await
            .map_err(|error| {
                format!(
                    "Gemini Live WebSocket connection failed: {}",
                    safe_error_text(&error, &[api_key])
                )
            })?;

        let mut language_codes = Vec::new();
        if options.language != "auto" && !options.language.trim().is_empty() {
            language_codes.push(options.language.trim().to_string());
        }

        let custom_vocab = options
            .prompt
            .as_ref()
            .map(|p| {
                p.split(&[',', '，', '、', ' '][..])
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());

        let setup_frame = GeminiLiveSetupFrame {
            setup: GeminiLiveSetupConfig {
                model: "models/gemini-3.5-transcribe-live".to_string(),
                generation_config: Some(GeminiLiveGenerationConfig {
                    response_modalities: vec!["TEXT".to_string()],
                }),
                input_audio_transcription: Some(GeminiLiveInputAudioTranscription {
                    language_codes,
                    mode: "SMART".to_string(),
                    custom_vocabulary: custom_vocab,
                }),
            },
        };

        let setup_json = serde_json::to_string(&setup_frame)
            .map_err(|e| format!("Failed to serialize Gemini Live setup frame: {}", e))?;

        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            setup_json.into(),
        ))
        .await
        .map_err(|e| {
            format!(
                "Failed to send Gemini Live setup frame: {}",
                safe_error_text(&e.to_string(), &[api_key])
            )
        })?;

        let setup_timeout = Duration::from_secs(10);
        let setup_result = tokio::time::timeout(setup_timeout, async {
            while let Some(msg_res) = ws.next().await {
                match msg_res {
                    Ok(msg) => match msg {
                        tokio_tungstenite::tungstenite::Message::Close(_) => {
                            return Err(
                                "Gemini Live server closed connection before setup completed"
                                    .to_string(),
                            );
                        }
                        tokio_tungstenite::tungstenite::Message::Ping(data) => {
                            let _ = ws
                                .send(tokio_tungstenite::tungstenite::Message::Pong(data))
                                .await;
                        }
                        other => {
                            if let Some(server_msg) = GeminiLiveServerMessage::parse(&other) {
                                if let Some(err) = server_msg.error {
                                    let message =
                                        err.message.unwrap_or_else(|| "Unknown error".to_string());
                                    return Err(format!(
                                        "Gemini Live setup failed: {}",
                                        safe_error_text(&message, &[api_key])
                                    ));
                                }
                                if server_msg.setup_complete.is_some() {
                                    return Ok(());
                                }
                            }
                        }
                    },
                    Err(e) => {
                        return Err(format!(
                            "Gemini Live failed to receive handshake response: {}",
                            safe_error_text(&e.to_string(), &[api_key])
                        ));
                    }
                }
            }
            Err("Gemini Live server disconnected before handshake completed".to_string())
        })
        .await;

        match setup_result {
            Ok(Ok(())) => {
                log::info!("Gemini Live setup completed");
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err("Timed out waiting for Gemini Live setupComplete (10s)".to_string())
            }
        }

        let (audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(1);

        let worker_handle = tokio::spawn(run_gemini_live_worker(
            ws,
            audio_rx,
            cmd_rx,
            text_sink,
            api_key.to_string(),
        ));

        Ok(Box::new(GeminiLiveStreamingSession {
            audio_tx,
            cmd_tx,
            worker_handle,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn test_encode_wav_in_memory_format_and_clamping() {
        let samples = vec![0.0f32, 0.5f32, -0.5f32, 1.5f32, -2.0f32];
        let wav_bytes =
            GeminiProvider::encode_wav_in_memory(&samples, 16000).expect("encoding should succeed");

        let mut reader =
            hound::WavReader::new(Cursor::new(wav_bytes)).expect("WAV reader should parse output");
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16000);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);

        let decoded_samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(decoded_samples.len(), 5);
        assert_eq!(decoded_samples[0], 0);
        assert!((decoded_samples[1] - 16384).abs() <= 1);
        assert!((decoded_samples[2] - (-16384)).abs() <= 1);
        assert_eq!(decoded_samples[3], 32767); // 1.5 clamped to 1.0 -> 32767
        assert_eq!(decoded_samples[4], -32767); // -2.0 clamped to -1.0 -> -32767
    }

    #[test]
    fn test_encode_wav_in_memory_empty() {
        let samples: Vec<f32> = Vec::new();
        let wav_bytes = GeminiProvider::encode_wav_in_memory(&samples, 16000)
            .expect("empty encoding should succeed");
        let mut reader = hound::WavReader::new(Cursor::new(wav_bytes))
            .expect("WAV reader should parse empty WAV");
        assert_eq!(reader.samples::<i16>().count(), 0);
    }

    #[test]
    fn test_build_request_url() {
        let url1 = GeminiProvider::build_request_url("", "my-key");
        assert_eq!(
            url1,
            "https://generativelanguage.googleapis.com/v1beta/interactions?key=my-key"
        );

        let url2 = GeminiProvider::build_request_url("https://custom-proxy.internal", "my-key");
        assert_eq!(
            url2,
            "https://custom-proxy.internal/v1beta/interactions?key=my-key"
        );

        let url3 =
            GeminiProvider::build_request_url("https://custom-proxy.internal/v1beta", "my-key");
        assert_eq!(
            url3,
            "https://custom-proxy.internal/v1beta/interactions?key=my-key"
        );

        let url4 =
            GeminiProvider::build_request_url("https://custom-proxy.internal/v1beta/", "my-key");
        assert_eq!(
            url4,
            "https://custom-proxy.internal/v1beta/interactions?key=my-key"
        );

        let url_aq = GeminiProvider::build_request_url("", "AQ.Ab8RN6Test");
        assert_eq!(
            url_aq,
            "https://generativelanguage.googleapis.com/v1beta/interactions?key=AQ.Ab8RN6Test"
        );

        let url_oauth = GeminiProvider::build_request_url("", "ya29.a0AfH6SMTest");
        assert_eq!(
            url_oauth,
            "https://generativelanguage.googleapis.com/v1beta/interactions"
        );
    }

    #[test]
    fn test_parse_api_error() {
        let status = reqwest::StatusCode::BAD_REQUEST;
        let json_err = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        let formatted = GeminiProvider::parse_api_error(status, json_err);
        assert!(formatted.contains("API key not valid"));
        assert!(formatted.contains("400"));

        let raw_err = "Gateway timeout";
        let raw_formatted =
            GeminiProvider::parse_api_error(reqwest::StatusCode::GATEWAY_TIMEOUT, raw_err);
        assert!(raw_formatted.contains("504"));
        assert!(raw_formatted.contains("Gateway timeout"));
    }

    #[tokio::test]
    async fn gemini_connection_failure_redacts_credentials_and_keeps_http_classification() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let api_key = "live-api-key-should-not-escape";
        let body = format!(
            "{{\"error\":{{\"message\":\"upstream echoed {api_key} http://proxy-user:proxy-pass@example.test:8080/path?key={api_key}\"}}}}"
        );
        let response = format!(
            "HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let base_url = format!("http://{address}");
        let error = GeminiProvider::test_connection(&client, api_key, Some(&base_url))
            .await
            .unwrap_err();
        server.await.unwrap();

        assert!(error.contains("HTTP 403"));
        assert!(error.contains("upstream echoed"));
        assert!(!error.contains(api_key));
        assert!(!error.contains("proxy-user"));
        assert!(!error.contains("proxy-pass"));
        assert!(!error.contains("?key="));
    }

    #[test]
    fn test_interaction_request_serialization() {
        let req = GeminiInteractionRequest {
            model: "gemini-3.5-transcribe".to_string(),
            input: vec![
                GeminiInteractionInput::Audio {
                    data: "base64audio".to_string(),
                    mime_type: "audio/wav".to_string(),
                },
                GeminiInteractionInput::Text {
                    text: "Speech prompt".to_string(),
                },
            ],
            generation_config: Some(GeminiInteractionGenerationConfig {
                transcription_config: Some(GeminiTranscriptionConfig {
                    language_codes: vec!["zh-CN".to_string()],
                    mode: GeminiTranscriptionMode::Smart,
                }),
            }),
        };

        let json_val = serde_json::to_value(&req).expect("should serialize request");
        assert_eq!(json_val["model"], "gemini-3.5-transcribe");
        assert_eq!(json_val["input"][0]["type"], "audio");
        assert_eq!(json_val["input"][0]["data"], "base64audio");
        assert_eq!(json_val["input"][0]["mime_type"], "audio/wav");
        assert_eq!(json_val["input"][1]["type"], "text");
        assert_eq!(json_val["input"][1]["text"], "Speech prompt");
        assert_eq!(
            json_val["generation_config"]["transcription_config"]["mode"],
            "smart"
        );
        assert_eq!(
            json_val["generation_config"]["transcription_config"]["language_codes"][0],
            "zh-CN"
        );
    }

    #[test]
    fn test_interaction_response_deserialization_and_text_extraction() {
        let json_str = r#"{
            "id": "interactions/int-20260905-xyz891",
            "status": "completed",
            "steps": [
                {
                    "id": "step_001",
                    "type": "model_output",
                    "content": [
                        {
                            "type": "text",
                            "text": "This is high-accuracy transcribed text from Gemini 3.5 Transcribe."
                        }
                    ]
                }
            ],
            "usage": {
                "total_input_tokens": 128,
                "total_output_tokens": 32,
                "total_tokens": 160
            }
        }"#;

        let res: GeminiInteractionResponse =
            serde_json::from_str(json_str).expect("should deserialize response");
        assert_eq!(res.status.as_deref(), Some("completed"));

        let extracted =
            GeminiProvider::extract_text_from_response(&res).expect("should extract text");
        assert_eq!(
            extracted,
            "This is high-accuracy transcribed text from Gemini 3.5 Transcribe."
        );
    }

    #[test]
    fn test_extract_text_multiple_steps_and_contents() {
        let res = GeminiInteractionResponse {
            id: Some("int-test".to_string()),
            status: Some("completed".to_string()),
            steps: Some(vec![
                GeminiInteractionStep {
                    id: Some("s1".to_string()),
                    step_type: Some("model_output".to_string()),
                    content: Some(vec![GeminiInteractionContent {
                        content_type: Some("text".to_string()),
                        text: Some("Hello".to_string()),
                    }]),
                },
                GeminiInteractionStep {
                    id: Some("s2".to_string()),
                    step_type: Some("model_output".to_string()),
                    content: Some(vec![GeminiInteractionContent {
                        content_type: Some("text".to_string()),
                        text: Some("World".to_string()),
                    }]),
                },
            ]),
        };

        let extracted =
            GeminiProvider::extract_text_from_response(&res).expect("should extract text");
        assert_eq!(extracted, "Hello World");
    }

    #[test]
    fn test_extract_text_completed_empty() {
        let res = GeminiInteractionResponse {
            id: Some("int-empty".to_string()),
            status: Some("completed".to_string()),
            steps: Some(vec![]),
        };

        let extracted = GeminiProvider::extract_text_from_response(&res)
            .expect("empty completed should return empty string");
        assert_eq!(extracted, "");
    }

    #[test]
    fn test_transcription_mode_serialization() {
        let smart_json = serde_json::to_string(&GeminiTranscriptionMode::Smart).unwrap();
        assert_eq!(smart_json, "\"smart\"");

        let verbatim_json = serde_json::to_string(&GeminiTranscriptionMode::Verbatim).unwrap();
        assert_eq!(verbatim_json, "\"verbatim\"");

        let default_mode: GeminiTranscriptionMode = Default::default();
        assert_eq!(default_mode, GeminiTranscriptionMode::Smart);
    }

    #[test]
    fn test_convert_samples_to_pcm16_le() {
        let samples = vec![0.0, 1.0, -1.0, 0.5, 2.0, -2.0];
        let pcm_bytes = GeminiProvider::convert_samples_to_pcm16_le(&samples);
        assert_eq!(pcm_bytes.len(), samples.len() * 2);

        // 0.0 -> 0
        let val0 = i16::from_le_bytes([pcm_bytes[0], pcm_bytes[1]]);
        assert_eq!(val0, 0);

        // 1.0 -> 32767
        let val1 = i16::from_le_bytes([pcm_bytes[2], pcm_bytes[3]]);
        assert_eq!(val1, 32767);

        // -1.0 -> -32767
        let val2 = i16::from_le_bytes([pcm_bytes[4], pcm_bytes[5]]);
        assert_eq!(val2, -32767);

        // 2.0 clamped to 1.0 -> 32767
        let val4 = i16::from_le_bytes([pcm_bytes[8], pcm_bytes[9]]);
        assert_eq!(val4, 32767);
    }

    #[test]
    fn test_build_live_websocket_url() {
        let default_url = GeminiProvider::build_live_websocket_url(None, "my_api_key");
        assert_eq!(
            default_url,
            "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key=my_api_key"
        );

        let https_url = GeminiProvider::build_live_websocket_url(
            Some("https://api.mygateway.com/v1"),
            "test_key",
        );
        assert_eq!(
            https_url,
            "wss://api.mygateway.com/v1/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key=test_key"
        );

        let http_url =
            GeminiProvider::build_live_websocket_url(Some("http://localhost:8080"), "test_key");
        assert_eq!(
            http_url,
            "ws://localhost:8080/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key=test_key"
        );
    }

    #[test]
    fn test_gemini_live_setup_frame_serialization() {
        let frame = GeminiLiveSetupFrame {
            setup: GeminiLiveSetupConfig {
                model: "models/gemini-3.5-transcribe-live".to_string(),
                generation_config: Some(GeminiLiveGenerationConfig {
                    response_modalities: vec!["TEXT".to_string()],
                }),
                input_audio_transcription: Some(GeminiLiveInputAudioTranscription {
                    language_codes: vec!["zh-CN".to_string()],
                    mode: "SMART".to_string(),
                    custom_vocabulary: Some(vec!["Handy".to_string(), "Tauri".to_string()]),
                }),
            },
        };

        let json_val = serde_json::to_value(&frame).expect("should serialize setup frame");
        assert_eq!(
            json_val["setup"]["model"],
            "models/gemini-3.5-transcribe-live"
        );
        assert_eq!(
            json_val["setup"]["generationConfig"]["responseModalities"][0],
            "TEXT"
        );
        assert_eq!(
            json_val["setup"]["inputAudioTranscription"]["mode"],
            "SMART"
        );
        assert_eq!(
            json_val["setup"]["inputAudioTranscription"]["languageCodes"][0],
            "zh-CN"
        );
        assert_eq!(
            json_val["setup"]["inputAudioTranscription"]["customVocabulary"][0],
            "Handy"
        );
    }

    #[test]
    fn test_gemini_live_realtime_input_frame_serialization() {
        let audio_frame = GeminiLiveRealtimeInputFrame {
            realtime_input: GeminiLiveRealtimeInput {
                audio: Some(GeminiLiveAudioData {
                    data: "base64pcm".to_string(),
                    mime_type: "audio/pcm;rate=16000".to_string(),
                }),
                audio_stream_end: None,
            },
        };
        let json_audio = serde_json::to_value(&audio_frame).unwrap();
        assert_eq!(json_audio["realtimeInput"]["audio"]["data"], "base64pcm");
        assert_eq!(
            json_audio["realtimeInput"]["audio"]["mimeType"],
            "audio/pcm;rate=16000"
        );
        assert!(json_audio["realtimeInput"]["audioStreamEnd"].is_null());

        let end_frame = GeminiLiveRealtimeInputFrame {
            realtime_input: GeminiLiveRealtimeInput {
                audio: None,
                audio_stream_end: Some(true),
            },
        };
        let json_end = serde_json::to_value(&end_frame).unwrap();
        assert_eq!(json_end["realtimeInput"]["audioStreamEnd"], true);
        assert!(json_end["realtimeInput"]["audio"].is_null());
    }

    #[test]
    fn test_gemini_live_server_message_deserialization() {
        let json_interim = r#"{
            "serverContent": {
                "interimInputTranscription": {
                    "text": "hello"
                }
            }
        }"#;
        let msg: GeminiLiveServerMessage = serde_json::from_str(json_interim).unwrap();
        let interim = msg
            .server_content
            .unwrap()
            .interim_input_transcription
            .unwrap();
        assert_eq!(interim.text.as_deref(), Some("hello"));

        let json_final = r#"{
            "serverContent": {
                "inputTranscription": {
                    "text": "hello, world!"
                },
                "turnComplete": true
            }
        }"#;
        let msg2: GeminiLiveServerMessage = serde_json::from_str(json_final).unwrap();
        let content = msg2.server_content.unwrap();
        assert_eq!(
            content.input_transcription.unwrap().text.as_deref(),
            Some("hello, world!")
        );
        assert_eq!(content.turn_complete, Some(true));

        let json_err = r#"{
            "error": {
                "code": 400,
                "message": "Invalid API Key"
            }
        }"#;
        let msg3: GeminiLiveServerMessage = serde_json::from_str(json_err).unwrap();
        let err = msg3.error.unwrap();
        assert_eq!(err.code, Some(400));
        assert_eq!(err.message.as_deref(), Some("Invalid API Key"));
    }

    struct MockSink {
        emitted: Arc<parking_lot::Mutex<Vec<(String, String)>>>,
    }
    impl StreamTextSink for MockSink {
        fn emit_text(&self, committed: String, tentative: String) {
            self.emitted.lock().push((committed, tentative));
        }
    }

    #[tokio::test]
    async fn dropping_session_or_finalize_future_closes_live_connection() {
        use tokio_tungstenite::tungstenite::{protocol::Role, Message};

        for during_finalize in [false, true] {
            let (client_io, server_io) = tokio::io::duplex(64 * 1024);
            let client_ws = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
            let mut server_ws =
                WebSocketStream::from_raw_socket(server_io, Role::Server, None).await;
            let (audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();
            let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(1);
            let sink = Arc::new(MockSink {
                emitted: Arc::new(parking_lot::Mutex::new(Vec::new())),
            });
            let worker_handle = tokio::spawn(run_gemini_live_worker(
                client_ws,
                audio_rx,
                cmd_rx,
                sink,
                String::new(),
            ));
            let session = Box::new(GeminiLiveStreamingSession {
                audio_tx,
                cmd_tx,
                worker_handle,
            });
            session.feed_audio(&vec![0.0; SAMPLES_PER_CHUNK]).unwrap();
            assert!(matches!(server_ws.next().await, Some(Ok(Message::Text(_)))));

            if during_finalize {
                let finishing = tokio::spawn(session.finalize());
                let end = server_ws.next().await.unwrap().unwrap();
                assert!(matches!(end, Message::Text(text) if text.contains("audioStreamEnd")));
                finishing.abort();
                let _ = finishing.await;
            } else {
                drop(session);
            }

            let closed = tokio::time::timeout(Duration::from_secs(1), server_ws.next())
                .await
                .expect("cancelled connection must close without server cooperation");
            assert!(matches!(
                closed,
                None | Some(Err(_)) | Some(Ok(Message::Close(_)))
            ));
        }
    }

    #[tokio::test]
    async fn live_server_error_is_redacted_before_returning_to_router() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        use tokio_tungstenite::tungstenite::Message;

        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let client_ws = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
        let mut server_ws = WebSocketStream::from_raw_socket(server_io, Role::Server, None).await;
        let (_audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(1);
        let sink = Arc::new(MockSink {
            emitted: Arc::new(parking_lot::Mutex::new(Vec::new())),
        });
        let api_key = "live-worker-api-key";
        let worker = tokio::spawn(run_gemini_live_worker(
            client_ws,
            audio_rx,
            cmd_rx,
            sink,
            api_key.to_string(),
        ));

        let server_error = format!(
            r#"{{"error":{{"message":"echo {api_key} http://user:pass@example.test:8080/path?key={api_key}"}}}}"#
        );
        server_ws
            .send(Message::Text(server_error.into()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        cmd_tx.send(SessionCmd::Finalize(reply_tx)).await.unwrap();
        let end = server_ws.next().await.unwrap().unwrap();
        assert!(matches!(end, Message::Text(text) if text.contains("audioStreamEnd")));

        let error = reply_rx.await.unwrap().unwrap_err();
        assert!(error.contains("echo"));
        assert!(!error.contains(api_key));
        assert!(!error.contains("user:pass"));
        assert!(!error.contains("?key="));
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn test_gemini_live_worker_full_duplex_flow() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        use tokio_tungstenite::tungstenite::Message;

        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let client_ws =
            tokio_tungstenite::WebSocketStream::from_raw_socket(client_io, Role::Client, None)
                .await;
        let mut server_ws =
            tokio_tungstenite::WebSocketStream::from_raw_socket(server_io, Role::Server, None)
                .await;

        let (audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(1);
        let emitted = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = Arc::new(MockSink {
            emitted: Arc::clone(&emitted),
        });

        let worker_handle = tokio::spawn(run_gemini_live_worker(
            client_ws,
            audio_rx,
            cmd_rx,
            sink,
            String::new(),
        ));

        let samples = vec![0.0f32; 1600];
        audio_tx.send(samples).unwrap();

        let msg = server_ws.next().await.unwrap().unwrap();
        if let Message::Text(text) = msg {
            assert!(text.contains("realtimeInput"));
            assert!(text.contains("audio"));
        } else {
            panic!("Expected text message from client");
        }

        let interim_json = r#"{"serverContent":{"interimInputTranscription":{"text":"hello"}}}"#;
        server_ws
            .send(Message::Binary(interim_json.as_bytes().to_vec().into()))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        {
            let events = emitted.lock();
            assert_eq!(events.last(), Some(&("".to_string(), "hello".to_string())));
        }

        let final_text_json =
            r#"{"serverContent":{"inputTranscription":{"text":"hello, world!"}}}"#;
        server_ws
            .send(Message::Text(final_text_json.into()))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        {
            let events = emitted.lock();
            assert_eq!(
                events.last(),
                Some(&("hello, world!".to_string(), "".to_string()))
            );
        }

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        cmd_tx.send(SessionCmd::Finalize(reply_tx)).await.unwrap();

        let end_msg = server_ws.next().await.unwrap().unwrap();
        if let Message::Text(text) = end_msg {
            assert!(text.contains("audioStreamEnd"));
        } else {
            panic!("Expected audioStreamEnd frame");
        }
        let final_result = reply_rx.await.unwrap().unwrap();
        assert_eq!(final_result, "hello, world!");

        worker_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_gemini_live_worker_finalize_without_turn_complete() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        use tokio_tungstenite::tungstenite::Message;

        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let client_ws =
            tokio_tungstenite::WebSocketStream::from_raw_socket(client_io, Role::Client, None)
                .await;
        let mut server_ws =
            tokio_tungstenite::WebSocketStream::from_raw_socket(server_io, Role::Server, None)
                .await;

        let (_audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(1);
        let emitted = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = Arc::new(MockSink {
            emitted: Arc::clone(&emitted),
        });

        let _worker_handle = tokio::spawn(run_gemini_live_worker(
            client_ws,
            audio_rx,
            cmd_rx,
            sink,
            String::new(),
        ));

        let final_text_json =
            r#"{"serverContent":{"inputTranscription":{"text":"instant transcription"}}}"#;
        server_ws
            .send(Message::Text(final_text_json.into()))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(30)).await;

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let start = tokio::time::Instant::now();
        cmd_tx.send(SessionCmd::Finalize(reply_tx)).await.unwrap();

        let end_msg = server_ws.next().await.unwrap().unwrap();
        if let Message::Text(text) = end_msg {
            assert!(text.contains("audioStreamEnd"));
        } else {
            panic!("Expected audioStreamEnd frame");
        }

        // Server sends NO turnComplete (real gemini-3.5-transcribe-live behavior)
        let final_result = reply_rx.await.unwrap().unwrap();
        let elapsed = start.elapsed();
        assert_eq!(final_result, "instant transcription");
        assert!(
            elapsed < Duration::from_millis(500),
            "Finalize took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_gemini_live_worker_finalize_with_interim_text_only_completes_instantly() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        use tokio_tungstenite::tungstenite::Message;

        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let client_ws =
            tokio_tungstenite::WebSocketStream::from_raw_socket(client_io, Role::Client, None)
                .await;
        let mut server_ws =
            tokio_tungstenite::WebSocketStream::from_raw_socket(server_io, Role::Server, None)
                .await;

        let (_audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(1);
        let emitted = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = Arc::new(MockSink {
            emitted: Arc::clone(&emitted),
        });

        let _worker_handle = tokio::spawn(run_gemini_live_worker(
            client_ws,
            audio_rx,
            cmd_rx,
            sink,
            String::new(),
        ));

        let interim_text_json =
            r#"{"serverContent":{"interimInputTranscription":{"text":"live spoken phrase"}}}"#;
        server_ws
            .send(Message::Text(interim_text_json.into()))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(30)).await;

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let start = tokio::time::Instant::now();
        cmd_tx.send(SessionCmd::Finalize(reply_tx)).await.unwrap();

        let end_msg = server_ws.next().await.unwrap().unwrap();
        if let Message::Text(text) = end_msg {
            assert!(text.contains("audioStreamEnd"));
        } else {
            panic!("Expected audioStreamEnd frame");
        }

        let final_result = reply_rx.await.unwrap().unwrap();
        let elapsed = start.elapsed();
        assert_eq!(final_result, "live spoken phrase");
        assert!(
            elapsed < Duration::from_millis(150),
            "Finalize with interim text took too long: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_gemini_live_server_message_setup_complete() {
        let json_setup = r#"{"setupComplete":{}}"#;
        let msg: GeminiLiveServerMessage = serde_json::from_str(json_setup).unwrap();
        assert!(msg.setup_complete.is_some());
    }

    #[test]
    fn test_gemini_live_server_message_parse_text_and_binary() {
        use tokio_tungstenite::tungstenite::Message;

        let text_msg = Message::Text(r#"{"setupComplete":{}}"#.into());
        let parsed_text = GeminiLiveServerMessage::parse(&text_msg);
        assert!(parsed_text.is_some());
        assert!(parsed_text.unwrap().setup_complete.is_some());

        let bin_msg = Message::Binary(b"{\n  \"setupComplete\": {}\n}\n".to_vec().into());
        let parsed_bin = GeminiLiveServerMessage::parse(&bin_msg);
        assert!(parsed_bin.is_some());
        assert!(parsed_bin.unwrap().setup_complete.is_some());

        let bin_interim = Message::Binary(
            r#"{"serverContent":{"interimInputTranscription":{"text":"test phrase"}}}"#
                .as_bytes()
                .to_vec()
                .into(),
        );
        let parsed_interim = GeminiLiveServerMessage::parse(&bin_interim);
        assert!(parsed_interim.is_some());
        let content = parsed_interim.unwrap().server_content.unwrap();
        assert_eq!(
            content.interim_input_transcription.unwrap().text.unwrap(),
            "test phrase"
        );

        let ping_msg = Message::Ping(vec![1, 2, 3].into());
        assert!(GeminiLiveServerMessage::parse(&ping_msg).is_none());
    }
}

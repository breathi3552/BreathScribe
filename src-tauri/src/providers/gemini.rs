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

/// Gemini Live 客户端握手帧（Setup）
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

/// Gemini Live 实时音频推流或结束帧（Client Message）
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

/// Gemini Live 服务端握手完成报文
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GeminiLiveSetupComplete {}

/// Gemini Live 服务端下行消息契约
#[derive(serde::Deserialize, Debug, Clone)]
pub struct GeminiLiveServerMessage {
    #[serde(rename = "setupComplete")]
    pub setup_complete: Option<GeminiLiveSetupComplete>,
    #[serde(rename = "serverContent")]
    pub server_content: Option<GeminiLiveServerContent>,
    pub error: Option<GeminiLiveError>,
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
/// Interactions API 请求体契约
#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiInteractionRequest {
    pub model: String,
    pub input: Vec<GeminiInteractionInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiInteractionGenerationConfig>,
}

/// Interactions API 多模态输入单元
#[derive(serde::Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum GeminiInteractionInput {
    #[serde(rename = "audio")]
    Audio { data: String, mime_type: String },
    #[serde(rename = "text")]
    Text { text: String },
}

/// Interactions API 生成与转录配置
#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiInteractionGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcription_config: Option<GeminiTranscriptionConfig>,
}

/// Interactions API 转录模式
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeminiTranscriptionMode {
    /// 智能听写模式：自动过滤语气助词、口误修正与标点规整化（推荐）
    Smart,
    /// 原始逐字稿模式
    Verbatim,
}

impl Default for GeminiTranscriptionMode {
    fn default() -> Self {
        Self::Smart
    }
}

/// 专用于语音转录的 transcription_config
#[derive(serde::Serialize, Debug, Clone)]
pub struct GeminiTranscriptionConfig {
    pub language_codes: Vec<String>,
    pub mode: GeminiTranscriptionMode,
}

/// Interactions API 响应契约
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

    /// 在内存中将单声道音频样本列表编码为标准的 16kHz 16-bit Mono WAV 二进制数据
    pub fn encode_wav_in_memory(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| format!("创建内存 WAV 写入器失败: {}", e))?;

        for &sample in samples {
            // [-1.0, 1.0] 浮点截断并映射至 i16 范围
            let clamped = sample.max(-1.0).min(1.0);
            let scaled = (clamped * 32767.0).round() as i16;
            writer
                .write_sample(scaled)
                .map_err(|e| format!("写入 WAV 采样失败: {}", e))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("完成 WAV 编码失败: {}", e))?;
        Ok(cursor.into_inner())
    }
    /// 检查凭据是否为 Google OAuth 访问令牌（通常以 ya29. 开头）
    pub fn is_oauth_token(key: &str) -> bool {
        key.trim().starts_with("ya29.")
    }

    /// 根据 Base URL 与 API Key 构造 Interactions API 请求端点 URL
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

    /// 从 Interactions API 响应中提取转录结果
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

        Err("Gemini Interactions API 未返回有效的转写文本".to_string())
    }

    /// 统一解析 Gemini API 的错误响应文本
    pub fn parse_api_error(status: reqwest::StatusCode, error_text: &str) -> String {
        if let Ok(err_json) = serde_json::from_str::<GeminiErrorResponse>(error_text) {
            if let Some(msg) = err_json.error.and_then(|e| e.message) {
                return format!("Gemini API 错误 (HTTP {}): {}", status, msg);
            }
        }
        format!("Gemini API 返回错误 HTTP {}: {}", status, error_text)
    }

    /// 测试与 Gemini 接口的连通性与 API Key 有效性
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
        let response = req
            .send()
            .await
            .map_err(|e| format!("网络请求发送失败: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::parse_api_error(status, &error_text));
        }

        Ok(())
    }

    /// 将 [-1.0, 1.0] 的 16kHz f32 音频采样转换为 16 位单声道无损 PCM 小端序字节流
    pub fn convert_samples_to_pcm16_le(samples: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for &sample in samples {
            let clamped = sample.max(-1.0).min(1.0);
            let scaled = (clamped * 32767.0).round() as i16;
            bytes.extend_from_slice(&scaled.to_le_bytes());
        }
        bytes
    }

    /// 构造 Gemini Live 双向全双工 WebSocket 连接 URL
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
            .ok_or_else(|| "Gemini API Key 未配置，请前往【转录模型】设置配置凭据".to_string())?;

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

        // 1. 内存中将 16000Hz 浮点音频编码为标准 WAV 二进制并转为 Base64
        let wav_bytes = Self::encode_wav_in_memory(&audio, 16000)?;
        let base64_audio = BASE64.encode(&wav_bytes);

        // 2. 构造 REST 请求 URL 与请求载荷
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

        // 3. 复用全局网络管理器的共享连接池客户端
        let client = self.network_manager.client().await;
        let mut req = client.post(&request_url);
        if Self::is_oauth_token(api_key) {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        } else {
            req = req.header("x-goog-api-key", api_key);
        }
        let response = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("发送 Gemini 转写请求失败 (网络错误): {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::parse_api_error(status, &error_text));
        }

        let body: GeminiInteractionResponse = response
            .json()
            .await
            .map_err(|e| format!("解析 Gemini 响应 JSON 失败: {}", e))?;

        // 4. 提取输出文本
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

/// 基于 Google Gemini Live 双向全双工 WebSocket 的实时流式会话句柄
pub struct GeminiLiveStreamingSession {
    audio_tx: tokio::sync::mpsc::UnboundedSender<Vec<f32>>,
    cmd_tx: tokio::sync::mpsc::Sender<SessionCmd>,
}

#[async_trait::async_trait]
impl StreamingSession for GeminiLiveStreamingSession {
    fn feed_audio(&self, samples: &[f32]) -> Result<(), String> {
        self.audio_tx
            .send(samples.to_vec())
            .map_err(|e| format!("推送音频采样至流式会话失败: {}", e))
    }

    async fn finalize(self: Box<Self>) -> Result<String, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(SessionCmd::Finalize(reply_tx))
            .await
            .map_err(|e| format!("发送 finalize 指令失败: {}", e))?;
        reply_rx
            .await
            .map_err(|_| "会话工作协程未响应 finalize 指令".to_string())?
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
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut ws_sink, mut ws_stream) = ws.split();

    struct LiveState {
        committed_text: String,
        tentative_text: String,
        session_error: Option<String>,
        turn_completed: bool,
    }

    let state = Arc::new(parking_lot::Mutex::new(LiveState {
        committed_text: String::new(),
        tentative_text: String::new(),
        session_error: None,
        turn_completed: false,
    }));

    let turn_notify = Arc::new(tokio::sync::Notify::new());
    let (sink_tx, mut sink_rx) =
        tokio::sync::mpsc::channel::<tokio_tungstenite::tungstenite::Message>(64);

    // 1. 出站写入协程：独占持有 ws_sink，解耦所有发送操作
    let writer_handle = tokio::spawn(async move {
        while let Some(msg) = sink_rx.recv().await {
            if let Err(e) = ws_sink.send(msg).await {
                log::warn!("Gemini Live WebSocket 发送失败: {}", e);
                break;
            }
        }
        let _ = ws_sink.close().await;
    });

    // 2. 下行接收协程：独占持有 ws_stream，毫秒级广播 interim 与 inputTranscription
    let state_receiver = Arc::clone(&state);
    let sink_tx_receiver = sink_tx.clone();
    let turn_notify_receiver = Arc::clone(&turn_notify);
    let text_sink_receiver = Arc::clone(&text_sink);

    let receiver_handle = tokio::spawn(async move {
        while let Some(msg_res) = ws_stream.next().await {
            match msg_res {
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    if let Ok(server_msg) = serde_json::from_str::<GeminiLiveServerMessage>(&text) {
                        if let Some(err) = server_msg.error {
                            let err_text =
                                err.message.unwrap_or_else(|| "未知服务端错误".to_string());
                            log::warn!("Gemini Live 服务端返回错误: {}", err_text);
                            state_receiver.lock().session_error = Some(err_text);
                            turn_notify_receiver.notify_waiters();
                        }
                        if let Some(content) = server_msg.server_content {
                            if let Some(interim) = content.interim_input_transcription {
                                if let Some(t) = interim.text {
                                    let (committed, tentative) = {
                                        let mut s = state_receiver.lock();
                                        s.tentative_text = t.clone();
                                        (s.committed_text.clone(), t)
                                    };
                                    text_sink_receiver.emit_text(committed, tentative);
                                }
                            }
                            if let Some(input) = content.input_transcription {
                                if let Some(c) = input.text {
                                    let committed = {
                                        let mut s = state_receiver.lock();
                                        s.committed_text.push_str(&c);
                                        s.tentative_text.clear();
                                        s.committed_text.clone()
                                    };
                                    text_sink_receiver.emit_text(committed, String::new());
                                }
                            }
                            if content.turn_complete.unwrap_or(false) {
                                state_receiver.lock().turn_completed = true;
                                turn_notify_receiver.notify_waiters();
                                break;
                            }
                        }
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Ping(data)) => {
                    let _ = sink_tx_receiver
                        .send(tokio_tungstenite::tungstenite::Message::Pong(data))
                        .await;
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                    log::info!("Gemini Live 服务端关闭长连接");
                    turn_notify_receiver.notify_waiters();
                    break;
                }
                Err(e) => {
                    log::warn!("Gemini Live WebSocket 接收异常: {}", e);
                    state_receiver.lock().session_error =
                        Some(format!("WebSocket 接收异常: {}", e));
                    turn_notify_receiver.notify_waiters();
                    break;
                }
                _ => {}
            }
        }
        turn_notify_receiver.notify_waiters();
    });

    // 3. 上行推流与生命周期协程
    let mut pcm_buffer: Vec<u8> = Vec::with_capacity(SAMPLES_PER_CHUNK * 2);

    loop {
        tokio::select! {
            maybe_samples = audio_rx.recv() => {
                match maybe_samples {
                    Some(samples) => {
                        let pcm_bytes = GeminiProvider::convert_samples_to_pcm16_le(&samples);
                        pcm_buffer.extend_from_slice(&pcm_bytes);

                        // 达到 100ms 周期（1600 个采样 = 3200 字节 PCM）打包推流
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
                    None => {
                        // audio_rx 管道关闭
                    }
                }
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCmd::Finalize(reply_tx)) => {
                        // 冲刷 audio_rx 通道中积压的未处理采样
                        while let Ok(samples) = audio_rx.try_recv() {
                            let pcm_bytes = GeminiProvider::convert_samples_to_pcm16_le(&samples);
                            pcm_buffer.extend_from_slice(&pcm_bytes);
                        }

                        // 冲刷残留采样（若有）
                        if !pcm_buffer.is_empty() {
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

                        // 发送 audioStreamEnd 结束标记
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

                        // 最多等待 5 秒获取服务端最终分句与收尾确认
                        let finalize_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                        while tokio::time::Instant::now() < finalize_deadline {
                            if state.lock().turn_completed
                                || state.lock().session_error.is_some()
                                || receiver_handle.is_finished()
                            {
                                break;
                            }
                            let remaining = finalize_deadline - tokio::time::Instant::now();
                            let _ = tokio::time::timeout(remaining, turn_notify.notified()).await;
                        }

                        drop(sink_tx);
                        let _ = writer_handle.await;
                        receiver_handle.abort();
                        let _ = receiver_handle.await;
                        let final_state = state.lock();
                        let trimmed = final_state.committed_text.trim().to_string();
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
                        drop(sink_tx);
                        let _ = writer_handle.await;
                        receiver_handle.abort();
                        let _ = receiver_handle.await;
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
            .ok_or_else(|| "Gemini API Key 未配置，请前往【转录模型】设置配置凭据".to_string())?;

        let provider_config = settings
            .cloud_stt_providers
            .get("gemini")
            .cloned()
            .unwrap_or_default();

        let custom_base = provider_config.custom_base_url.as_deref();
        let ws_url = Self::build_live_websocket_url(custom_base, api_key);
        let proxy_settings = self.network_manager.proxy_settings().await;

        let mut ws =
            crate::network::proxy_tunnel::connect_websocket_tunnel(&ws_url, &proxy_settings)
                .await?;

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
            .map_err(|e| format!("序列化 Gemini Live Setup 报文失败: {}", e))?;

        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            setup_json.into(),
        ))
        .await
        .map_err(|e| format!("发送 Gemini Live Setup 握手帧失败: {}", e))?;

        let setup_timeout = Duration::from_secs(10);
        let setup_result = tokio::time::timeout(setup_timeout, async {
            while let Some(msg_res) = ws.next().await {
                match msg_res {
                    Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                        if let Ok(server_msg) =
                            serde_json::from_str::<GeminiLiveServerMessage>(&text)
                        {
                            if let Some(err) = server_msg.error {
                                return Err(format!(
                                    "Gemini Live Setup 握手失败: {}",
                                    err.message.unwrap_or_else(|| "未知错误".to_string())
                                ));
                            }
                            if server_msg.setup_complete.is_some() {
                                return Ok(());
                            }
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::Message::Close(frame)) => {
                        return Err(format!("Gemini Live 服务端关闭连接: {:?}", frame));
                    }
                    Err(e) => {
                        return Err(format!("Gemini Live 接收握手响应失败: {}", e));
                    }
                    _ => {}
                }
            }
            Err("Gemini Live 服务端在握手完成前断开连接".to_string())
        })
        .await;

        match setup_result {
            Ok(Ok(())) => {
                log::info!("Gemini Live Setup 握手成功 (setupComplete 已就绪)");
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("等待 Gemini Live setupComplete 握手超时 (10s)".to_string()),
        }

        let (audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(1);

        tokio::spawn(run_gemini_live_worker(ws, audio_rx, cmd_rx, text_sink));

        Ok(Box::new(GeminiLiveStreamingSession { audio_tx, cmd_tx }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_encode_wav_in_memory_format_and_clamping() {
        // 包含正常值、0值、以及超过 [-1.0, 1.0] 的极值
        let samples = vec![0.0f32, 0.5f32, -0.5f32, 1.5f32, -2.0f32];
        let wav_bytes =
            GeminiProvider::encode_wav_in_memory(&samples, 16000).expect("encoding should succeed");

        // 验证生成的 WAV 二进制数据合法性
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
                            "text": "这是一段通过 Gemini 3.5 Transcribe 模型转写完成的高准确度文本。"
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
            "这是一段通过 Gemini 3.5 Transcribe 模型转写完成的高准确度文本。"
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
                    "text": "你好"
                }
            }
        }"#;
        let msg: GeminiLiveServerMessage = serde_json::from_str(json_interim).unwrap();
        let interim = msg
            .server_content
            .unwrap()
            .interim_input_transcription
            .unwrap();
        assert_eq!(interim.text.as_deref(), Some("你好"));

        let json_final = r#"{
            "serverContent": {
                "inputTranscription": {
                    "text": "你好，世界！"
                },
                "turnComplete": true
            }
        }"#;
        let msg2: GeminiLiveServerMessage = serde_json::from_str(json_final).unwrap();
        let content = msg2.server_content.unwrap();
        assert_eq!(
            content.input_transcription.unwrap().text.as_deref(),
            Some("你好，世界！")
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

        let worker_handle = tokio::spawn(run_gemini_live_worker(client_ws, audio_rx, cmd_rx, sink));

        // 1. 发送 1600 个静音采样（刚好 100ms）
        let samples = vec![0.0f32; 1600];
        audio_tx.send(samples).unwrap();

        // 2. 服务端应当收到一个包含 realtimeInput.audio 的消息
        let msg = server_ws.next().await.unwrap().unwrap();
        if let Message::Text(text) = msg {
            assert!(text.contains("realtimeInput"));
            assert!(text.contains("audio"));
        } else {
            panic!("Expected text message from client");
        }

        // 3. 服务端并发推送 interim 和 committed 消息
        let interim_json = r#"{"serverContent":{"interimInputTranscription":{"text":"你好"}}}"#;
        server_ws
            .send(Message::Text(interim_json.into()))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        {
            let events = emitted.lock();
            assert_eq!(events.last(), Some(&("".to_string(), "你好".to_string())));
        }

        let final_text_json = r#"{"serverContent":{"inputTranscription":{"text":"你好，世界！"}}}"#;
        server_ws
            .send(Message::Text(final_text_json.into()))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        {
            let events = emitted.lock();
            assert_eq!(
                events.last(),
                Some(&("你好，世界！".to_string(), "".to_string()))
            );
        }

        // 4. 客户端发起 finalize
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        cmd_tx.send(SessionCmd::Finalize(reply_tx)).await.unwrap();

        // 5. 服务端应当收到 audioStreamEnd 帧
        let end_msg = server_ws.next().await.unwrap().unwrap();
        if let Message::Text(text) = end_msg {
            assert!(text.contains("audioStreamEnd"));
        } else {
            panic!("Expected audioStreamEnd frame");
        }

        // 6. 服务端响应 turnComplete
        let turn_complete_json = r#"{"serverContent":{"turnComplete":true}}"#;
        server_ws
            .send(Message::Text(turn_complete_json.into()))
            .await
            .unwrap();

        // 7. finalize 应当收到完整的最终转写结果
        let final_result = reply_rx.await.unwrap().unwrap();
        assert_eq!(final_result, "你好，世界！");

        worker_handle.await.unwrap();
    }

    #[test]
    fn test_gemini_live_server_message_setup_complete() {
        let json_setup = r#"{"setupComplete":{}}"#;
        let msg: GeminiLiveServerMessage = serde_json::from_str(json_setup).unwrap();
        assert!(msg.setup_complete.is_some());
    }
}

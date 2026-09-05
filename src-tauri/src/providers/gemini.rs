use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use std::sync::Arc;
use tauri::AppHandle;

use crate::network::NetworkManager;
use crate::providers::{BatchTranscriptionProvider, TranscriptionOptions};
use crate::settings::DEFAULT_CLOUD_STT_MODEL_ID;

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
    Audio {
        data: String,
        mime_type: String,
    },
    #[serde(rename = "text")]
    Text {
        text: String,
    },
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

    /// 根据 Base URL 与 API Key 构造 Interactions API 请求端点 URL
    pub fn build_request_url(base_url: &str, api_key: &str) -> String {
        let trimmed_base = base_url.trim().trim_end_matches('/');
        let base = if trimmed_base.is_empty() {
            "https://generativelanguage.googleapis.com"
        } else {
            trimmed_base
        };

        if base.ends_with("/v1beta") {
            format!("{}/interactions?key={}", base, api_key)
        } else {
            format!("{}/v1beta/interactions?key={}", base, api_key)
        }
    }

    /// 从 Interactions API 响应中提取转录结果
    pub fn extract_text_from_response(
        body: &GeminiInteractionResponse,
    ) -> Result<String, String> {
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
        let test_url = if clean_base.ends_with("/v1beta") {
            format!("{}/models?key={}", clean_base, api_key)
        } else {
            format!("{}/v1beta/models?key={}", clean_base, api_key)
        };

        let response = client
            .get(&test_url)
            .header("x-goog-api-key", api_key)
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

        let model = if provider_config.model_id.trim().is_empty() {
            DEFAULT_CLOUD_STT_MODEL_ID
        } else {
            provider_config.model_id.trim()
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
        let response = client
            .post(&request_url)
            .header("x-goog-api-key", api_key)
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

        let url3 = GeminiProvider::build_request_url("https://custom-proxy.internal/v1beta", "my-key");
        assert_eq!(
            url3,
            "https://custom-proxy.internal/v1beta/interactions?key=my-key"
        );

        let url4 = GeminiProvider::build_request_url("https://custom-proxy.internal/v1beta/", "my-key");
        assert_eq!(
            url4,
            "https://custom-proxy.internal/v1beta/interactions?key=my-key"
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

        let extracted = GeminiProvider::extract_text_from_response(&res).expect("should extract text");
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

        let extracted = GeminiProvider::extract_text_from_response(&res).expect("should extract text");
        assert_eq!(extracted, "Hello World");
    }

    #[test]
    fn test_extract_text_completed_empty() {
        let res = GeminiInteractionResponse {
            id: Some("int-empty".to_string()),
            status: Some("completed".to_string()),
            steps: Some(vec![]),
        };

        let extracted = GeminiProvider::extract_text_from_response(&res).expect("empty completed should return empty string");
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
}

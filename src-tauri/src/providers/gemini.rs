use std::sync::Arc;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tauri::AppHandle;

use crate::network::NetworkManager;
use crate::providers::{BatchTranscriptionProvider, TranscriptionOptions};
#[derive(serde::Deserialize, Debug)]
pub struct GeminiResponse {
    pub candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(serde::Deserialize, Debug)]
pub struct GeminiCandidate {
    pub content: Option<GeminiContent>,
    #[serde(rename = "finishReason")]
    pub finish_reason: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct GeminiContent {
    pub parts: Option<Vec<GeminiPart>>,
}

#[derive(serde::Deserialize, Debug)]
pub struct GeminiPart {
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

    /// 根据 Base URL、模型 ID 与 API Key 构造请求端点 URL
    pub fn build_request_url(base_url: &str, model: &str, api_key: &str) -> String {
        let trimmed_base = base_url.trim().trim_end_matches('/');
        let base = if trimmed_base.is_empty() {
            "https://generativelanguage.googleapis.com"
        } else {
            trimmed_base
        };

        if base.ends_with("/v1beta") {
            format!("{}/models/{}:generateContent?key={}", base, model, api_key)
        } else {
            format!(
                "{}/v1beta/models/{}:generateContent?key={}",
                base, model, api_key
            )
        }
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
            "gemini-2.5-flash"
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

        // 2. 构造 REST 请求 URL
        let request_url = Self::build_request_url(custom_base, model, api_key);

        let system_instruction = "You are an expert speech recognition engine. Your ONLY task is to transcribe the spoken words in the provided audio file with extreme accuracy. Output verbatim text without commentary, pleasantries, or explanations. If speech is in a specific language, transcribe in that language unless instructed otherwise.";

        let user_prompt = match &options.prompt {
            Some(p) if !p.trim().is_empty() => p.clone(),
            _ => {
                if options.language != "auto" && !options.language.trim().is_empty() {
                    format!(
                        "Transcribe the speech in the audio verbatim. The spoken language is {}.",
                        options.language
                    )
                } else {
                    "Transcribe the speech in the audio verbatim.".to_string()
                }
            }
        };

        let payload = serde_json::json!({
            "system_instruction": {
                "parts": [{ "text": system_instruction }]
            },
            "contents": [{
                "parts": [
                    {
                        "inline_data": {
                            "mime_type": "audio/wav",
                            "data": base64_audio
                        }
                    },
                    {
                        "text": user_prompt
                    }
                ]
            }],
            "generationConfig": {
                "temperature": 0.0
            }
        });

        // 3. 复用全局网络管理器的共享连接池客户端
        let client = self.network_manager.client().await;
        let response = client
            .post(&request_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("发送 Gemini 转写请求失败 (网络错误): {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::parse_api_error(status, &error_text));
        }

        let body: GeminiResponse = response
            .json()
            .await
            .map_err(|e| format!("解析 Gemini 响应 JSON 失败: {}", e))?;

        // 4. 提取输出文本
        if let Some(candidate) = body.candidates.as_deref().and_then(|c| c.first()) {
            if let Some(text) = candidate
                .content
                .as_ref()
                .and_then(|c| c.parts.as_deref())
                .and_then(|p| p.first())
                .and_then(|part| part.text.as_deref())
            {
                return Ok(text.trim().to_string());
            }

            if let Some(finish_reason) = candidate.finish_reason.as_deref() {
                if finish_reason == "STOP" || finish_reason == "MAX_TOKENS" {
                    return Ok(String::new());
                }
            }
        }

        Err("Gemini 未返回有效的转写文本".to_string())
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
        let wav_bytes = GeminiProvider::encode_wav_in_memory(&samples, 16000).expect("encoding should succeed");

        // 验证生成的 WAV 二进制数据合法性
        let mut reader = hound::WavReader::new(Cursor::new(wav_bytes)).expect("WAV reader should parse output");
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
        let wav_bytes = GeminiProvider::encode_wav_in_memory(&samples, 16000).expect("empty encoding should succeed");
        let mut reader = hound::WavReader::new(Cursor::new(wav_bytes)).expect("WAV reader should parse empty WAV");
        assert_eq!(reader.samples::<i16>().count(), 0);
    }

    #[test]
    fn test_build_request_url() {
        let url1 = GeminiProvider::build_request_url("", "gemini-2.5-flash", "my-key");
        assert_eq!(
            url1,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key=my-key"
        );

        let url2 = GeminiProvider::build_request_url("https://custom-proxy.internal", "gemini-2.5-pro", "my-key");
        assert_eq!(
            url2,
            "https://custom-proxy.internal/v1beta/models/gemini-2.5-pro:generateContent?key=my-key"
        );

        let url3 = GeminiProvider::build_request_url("https://custom-proxy.internal/v1beta/", "gemini-2.5-flash", "my-key");
        assert_eq!(
            url3,
            "https://custom-proxy.internal/v1beta/models/gemini-2.5-flash:generateContent?key=my-key"
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
        let raw_formatted = GeminiProvider::parse_api_error(reqwest::StatusCode::GATEWAY_TIMEOUT, raw_err);
        assert!(raw_formatted.contains("504"));
        assert!(raw_formatted.contains("Gateway timeout"));
    }

    #[test]
    fn test_gemini_response_deserialization() {
        let json = r#"{
            "candidates": [
                {
                    "content": {
                        "parts": [
                            { "text": "This is a transcribed test." }
                        ]
                    },
                    "finishReason": "STOP"
                }
            ]
        }"#;
        let res: GeminiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            res.candidates.as_ref().unwrap()[0]
                .content.as_ref().unwrap()
                .parts.as_ref().unwrap()[0]
                .text.as_deref().unwrap(),
            "This is a transcribed test."
        );
    }
}

use std::sync::Arc;
use tauri::AppHandle;

use crate::providers::gemini::GeminiProvider;
use crate::providers::local::LocalTranscriptionProvider;
use crate::providers::{BatchTranscriptionProvider, TranscriptionOptions};
use crate::settings::TranscriptionMode;

pub struct TranscriptionRouter {
    app_handle: AppHandle,
    local_provider: Arc<LocalTranscriptionProvider>,
    gemini_provider: Arc<GeminiProvider>,
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
        }
    }

    pub fn local_provider(&self) -> &Arc<LocalTranscriptionProvider> {
        &self.local_provider
    }

    pub fn gemini_provider(&self) -> &Arc<GeminiProvider> {
        &self.gemini_provider
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

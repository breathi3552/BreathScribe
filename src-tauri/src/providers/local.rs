use crate::managers::transcription::TranscriptionManager;
use crate::providers::{BatchTranscriptionProvider, TranscriptionOptions};
use std::sync::Arc;

pub struct LocalTranscriptionProvider {
    transcription_manager: Arc<TranscriptionManager>,
}

impl LocalTranscriptionProvider {
    pub fn new(tm: Arc<TranscriptionManager>) -> Self {
        Self {
            transcription_manager: tm,
        }
    }
}

#[async_trait::async_trait]
impl BatchTranscriptionProvider for LocalTranscriptionProvider {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        _options: &TranscriptionOptions,
    ) -> Result<String, String> {
        let tm = Arc::clone(&self.transcription_manager);
        tauri::async_runtime::spawn_blocking(move || tm.transcribe(audio))
            .await
            .map_err(|e| format!("Local transcription task panicked: {}", e))?
            .map_err(|e| e.to_string())
    }

    fn provider_id(&self) -> &'static str {
        "local"
    }
}

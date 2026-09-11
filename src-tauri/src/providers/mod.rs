pub mod gemini;
pub mod local;

#[derive(Debug, Clone)]
pub struct TranscriptionOptions {
    pub language: String,
    pub prompt: Option<String>,
}

#[async_trait::async_trait]
pub trait BatchTranscriptionProvider: Send + Sync {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        options: &TranscriptionOptions,
    ) -> Result<String, String>;

    fn provider_id(&self) -> &'static str;
}

/// Sink for streaming transcription partial and committed text.
pub trait StreamTextSink: Send + Sync {
    fn emit_text(&self, committed: String, tentative: String);
}

/// Lifecycle of an active streaming transcription session.
#[async_trait::async_trait]
pub trait StreamingSession: Send + Sync {
    fn feed_audio(&self, samples: &[f32]) -> Result<(), String>;
    async fn finalize(self: Box<Self>) -> Result<String, String>;
    async fn cancel(self: Box<Self>);
}

/// Provider capable of real-time streaming transcription.
#[async_trait::async_trait]
pub trait StreamingTranscriptionProvider: Send + Sync {
    fn supports_streaming(&self, model: &str) -> bool;
    async fn start_stream(
        &self,
        options: &TranscriptionOptions,
        text_sink: std::sync::Arc<dyn StreamTextSink>,
    ) -> Result<Box<dyn StreamingSession>, String>;
}

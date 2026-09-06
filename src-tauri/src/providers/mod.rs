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
#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider {
        id: &'static str,
        response: String,
    }

    #[async_trait::async_trait]
    impl BatchTranscriptionProvider for MockProvider {
        async fn transcribe(
            &self,
            _audio: Vec<f32>,
            _options: &TranscriptionOptions,
        ) -> Result<String, String> {
            Ok(self.response.clone())
        }

        fn provider_id(&self) -> &'static str {
            self.id
        }
    }

    #[tokio::test]
    async fn test_mock_provider_contract() {
        let provider = MockProvider {
            id: "mock_test",
            response: "Hello, world!".to_string(),
        };
        assert_eq!(provider.provider_id(), "mock_test");

        let options = TranscriptionOptions {
            language: "en".to_string(),
            prompt: None,
        };
        let result = provider.transcribe(vec![0.0; 100], &options).await;
        assert_eq!(result.unwrap(), "Hello, world!");
    }

    struct MockSession {
        sink: std::sync::Arc<dyn StreamTextSink>,
        fed_count: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl StreamingSession for MockSession {
        fn feed_audio(&self, samples: &[f32]) -> Result<(), String> {
            self.fed_count
                .fetch_add(samples.len(), std::sync::atomic::Ordering::Relaxed);
            self.sink.emit_text("hello".to_string(), "world".to_string());
            Ok(())
        }

        async fn finalize(self: Box<Self>) -> Result<String, String> {
            Ok("hello world, test complete".to_string())
        }

        async fn cancel(self: Box<Self>) {}
    }

    struct MockStreamingProviderImpl {
        supported_model: &'static str,
    }

    #[async_trait::async_trait]
    impl StreamingTranscriptionProvider for MockStreamingProviderImpl {
        fn supports_streaming(&self, model: &str) -> bool {
            model == self.supported_model
        }

        async fn start_stream(
            &self,
            _options: &TranscriptionOptions,
            text_sink: std::sync::Arc<dyn StreamTextSink>,
        ) -> Result<Box<dyn StreamingSession>, String> {
            Ok(Box::new(MockSession {
                sink: text_sink,
                fed_count: std::sync::atomic::AtomicUsize::new(0),
            }))
        }
    }

    struct CollectingSink {
        events: parking_lot::Mutex<Vec<(String, String)>>,
    }

    impl StreamTextSink for CollectingSink {
        fn emit_text(&self, committed: String, tentative: String) {
            self.events.lock().push((committed, tentative));
        }
    }

    #[tokio::test]
    async fn test_streaming_provider_contract() {
        let provider = MockStreamingProviderImpl {
            supported_model: "gemini-3.5-transcribe-live",
        };
        assert!(provider.supports_streaming("gemini-3.5-transcribe-live"));
        assert!(!provider.supports_streaming("gemini-3.5-transcribe"));

        let sink = std::sync::Arc::new(CollectingSink {
            events: parking_lot::Mutex::new(Vec::new()),
        });

        let options = TranscriptionOptions {
            language: "en".to_string(),
            prompt: None,
        };

        let session = provider
            .start_stream(&options, sink.clone())
            .await
            .expect("start_stream should succeed");

        session
            .feed_audio(&[0.1, 0.2, 0.3])
            .expect("feed_audio should succeed");

        let events = sink.events.lock().clone();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], ("hello".to_string(), "world".to_string()));

        let final_text = session.finalize().await.expect("finalize should succeed");
        assert_eq!(final_text, "hello world, test complete");
    }
}

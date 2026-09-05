pub mod gemini;
pub mod local;

#[derive(Debug, Clone)]
pub struct TranscriptionOptions {
    pub language: String,
    pub prompt: Option<String>,
}

#[async_trait::async_trait]
pub trait BatchTranscriptionProvider: Send + Sync {
    /// 执行整段音频转写。入参采用所有权移动 `audio: Vec<f32>`，解除生命周期限制与多余拷贝
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        options: &TranscriptionOptions,
    ) -> Result<String, String>;

    fn provider_id(&self) -> &'static str;
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
}

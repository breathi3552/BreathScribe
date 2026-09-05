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

/// 接收流式转写增量文本的输出汇（Sink）
pub trait StreamTextSink: Send + Sync {
    /// 发送流式文本更新
    /// `committed`: 权威分句确认文本（稳定前缀）
    /// `tentative`: 毫秒级推测片段（暂态后缀）
    fn emit_text(&self, committed: String, tentative: String);
}

/// 单次实时流式会话生命周期
#[async_trait::async_trait]
pub trait StreamingSession: Send + Sync {
    /// 持续喂入 16kHz 单声道 f32 音频采样
    fn feed_audio(&self, samples: &[f32]) -> Result<(), String>;

    /// 停止录音，冲刷缓冲区并等待最终文本输出
    async fn finalize(self: Box<Self>) -> Result<String, String>;

    /// 放弃转写并清理长连接
    async fn cancel(self: Box<Self>);
}

/// 支持流式转写的服务商抽象 Trait
#[async_trait::async_trait]
pub trait StreamingTranscriptionProvider: Send + Sync {
    /// 检查指定的模型标识是否支持流式实时转写
    fn supports_streaming(&self, model: &str) -> bool;

    /// 开启实时流式转写会话
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
            self.sink.emit_text("你好".to_string(), "世界".to_string());
            Ok(())
        }

        async fn finalize(self: Box<Self>) -> Result<String, String> {
            Ok("你好世界，完整测试".to_string())
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
            language: "zh".to_string(),
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
        assert_eq!(events[0], ("你好".to_string(), "世界".to_string()));

        let final_text = session.finalize().await.expect("finalize should succeed");
        assert_eq!(final_text, "你好世界，完整测试");
    }
}

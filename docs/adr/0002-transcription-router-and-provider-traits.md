# 转写路由（TranscriptionRouter）与 Provider Trait 抽象架构决策

Handy-Cloud 需要在不破坏 Handy 现有本地离线转写实现（Whisper / Parakeet）的前提下，接入云端批量语音识别（如 Google Gemini API），并在未来支持实时双向流式识别。

## 决策内容

1. **双 Trait 体系与分阶段演进**：
   - 第一阶段（P1）聚焦批量转写接口 `BatchTranscriptionProvider`，实现整段音频向本地引擎与云端 REST API 的分流；
   - 第二阶段（P2）预留实时流式接口 `StreamingTranscriptionProvider`，对接 Handy 原生 `StreamRouter` 与云端 WebSocket 双向流；
   - 本阶段不扩展范围，只为云端批量转写增加外层抽象，不重构现有本地推理核心。

2. **核心 Trait 接口定义**：

   ```rust
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
   ```

   - **音频样本所有权**：接口使用 `audio: Vec<f32>`，由调用方直接移交所有权。本地 Provider 能直接将其 `move` 入 `spawn_blocking` 线程池，彻底规避 Rust 异步跨线程的 `'static` 借用生命周期限制与多余内存拷贝；云端 Provider 内部通过内存 `hound::WavWriter` 编码为 WAV 字节流时直接引用。
   - **上下文精简**：`TranscriptionOptions` 仅收敛 `language` 与 `prompt`。特定于模型的参数（如 Gemini 的 `temperature = 0.0`）由 Provider 内部固定处理，不污染通用抽象。
   - **错误契约**：返回值保持轻量 `Result<String, String>`，避免现阶段引入过度设计的复杂错误枚举或元数据。

3. **分流门面架构（Facade Router）**：
   - 引入 `TranscriptionRouter` 单例，聚合 `Arc<TranscriptionManager>` 与各云端 Provider。
   - `LocalTranscriptionProvider` 作为薄包装层，仅转发调用现有的 `TranscriptionManager::transcribe(audio)`。
   - 仅在 `actions.rs`（录音结束出字点）与 `commands/history.rs`（历史记录重试转写点）将直接调用 `tm.transcribe` 替换为 `router.transcribe`。
   - 原有本地模型的加载（`initiate_model_load`）、卸载（`unload_model`）、状态监听、VAD 过滤与托盘联动完全由 `TranscriptionManager` 继续托管，Router 不做全量状态代理。

4. **设置数据建模（Settings Modeling）**：
   - 在 `AppSettings` 中新增独立的 `transcription_mode` 枚举字段（Local / Cloud { provider, model }），指示当前生效的转写引擎。
   - 原有的 `selected_model` 保持现状，专门记录本地已下载模型的选择，二者互不干扰。用户在云端与本地之间切换时，本地模型配置不丢失。

5. **云端错误快速失败（Fail-Fast）**：
   - 当云端 Provider 遭遇超时、断网或 HTTP 4xx/5xx 时，立即返回错误并触发原生的 `"transcription-error"` 事件，向前端展示明确失败原因。
   - 不自动、静默回退至本地模型，避免意外触发数秒的模型冷启动卡顿和突发显存占用。
   - 录音数据在转写失败前已落盘至音频历史记录，用户排查网络或 API 凭据后可直接在历史面板中一键重试。

6. **本地模型显存管理**：
   - 切换至云端模式后，不强制立即销毁本地模型，避免用户快速切回对比测试时的反复冷启动。
   - 复用 Handy 原有的 `model_unload_timeout` 闲置监控机制：在云端模式下本地模型无活动触发，达到设定阈值后由内部定时器自动卸载释放显存与内存。

## 备选方案与否决原因

- **否决入参使用借用切片 `&[f32]`**：`&[f32]` 无法直接传入 `spawn_blocking`（缺乏 `'static` 生命周期约束），导致本地 Provider 必须强制调用 `.to_vec()` 分配内存，而在调用起点 `actions.rs` 中 `stop_recording` 本身产出的就是拥有所有权的 `Vec<f32>`。
- **否决 Router 全量接管 `TranscriptionManager`**：会导致大量与云端无关的本地模型管理接口（状态查询、模型下载、卸载）被强制套上一层无意义的转发代码，大幅增加与上游 Handy 仓库同步时的代码冲突风险。
- **否决云端失败时自动静默降级到本地**：本地模型若未预热，会产生不可预期的数秒加载等待与显存暴涨，且破坏了用户使用云端高质量识别的确定性预期。

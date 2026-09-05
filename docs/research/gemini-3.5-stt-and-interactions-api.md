# Google Gemini 最新语音转录 API 调研报告：从 Gemini 2.5 到 Gemini 3.5 的架构演进与落地方案

- **调研日期**：2026-09-05
- **调研目标**：核实 Google Gemini 官网最新 ASR/STT 模型与 API 体系，定位现存 `HTTP 404 Not Found` 错误的根本原因，明确 `gemini-3.5-transcribe` 与 `gemini-3.5-transcribe-live` 的事实边界，并为 Handy-Cloud 提供分阶段落地改造方案。
- **一手事实来源**：
  - [Google AI for Developers - Gemini Live API (Live Transcription)](https://ai.google.dev/gemini-api/docs/live-api/live-transcribe)
  - [Google AI for Developers - Gemini 3.5 Transcribe Model Card](https://ai.google.dev/gemini-api/docs/models/gemini-3.5-transcribe)
  - [Google AI for Developers - Audio Transcription Guide](https://ai.google.dev/gemini-api/docs/transcribe)
  - [Google AI for Developers - Interactions API Reference](https://ai.google.dev/api/interactions-api)
  - [Google AI for Developers - File Input Methods](https://ai.google.dev/gemini-api/docs/interactions/file-input-methods)

---

## 一、现状与报错溯源（Root Cause）

### 1. 运行时错误重现

在 Handy-Cloud 实机运行转录时，前端弹出错误通知：

```text
转录失败
Gemini API 错误 (HTTP 404 Not Found): This model models/gemini-2.5-flash is no longer available to new users. Please update your code to use models/gemini-3.6-flash for the latest features and improvements. We recommend you to use the Interactions API.
```

### 2. 根因分析

1. **旧模型下线**：Google 已在 2026 年将 `gemini-2.5-flash` 标记为废弃（Deprecated），并彻底对新 API Key / 新用户关闭访问权限，旧端点直接返回 HTTP 404；
2. **端点体系迭代**：Handy-Cloud 当前在 `src-tauri/src/providers/gemini.rs` 中调用的旧版端点是：
   `POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={key}`
   Google 当前主推的全新一元交互规范为 **Interactions API**（端点：`POST https://generativelanguage.googleapis.com/v1beta/interactions`）；
3. **模型矩阵重构**：Google 在 Gemini 3 世代对通用多模态大模型（`gemini-3.6-flash`）与专用语音识别模型进行了职责分离，正式推出了专用的语音转写模型矩阵（`Gemini 3.5 Transcribe` 系列）。

---

## 二、一手事实与技术澄清（纠偏关键误区）

针对用户提出的“使用 `Gemini 3.5 Transcribe Live` 这个模型，并改成对应 SDK”的需求，经过对照 Google 2026 年 8~9 月最新官方文档，需明确以下客观事实与技术边界：

### 1. 核心事实：`gemini-3.5-transcribe` 与 `gemini-3.5-transcribe-live` 属于两套截然不同的架构与协议

| 维度                        | `gemini-3.5-transcribe` (批处理 / 非 Live)                                                                   | `gemini-3.5-transcribe-live` (Live API)                                                                                       |
| --------------------------- | ------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------- |
| **协议类型**                | **HTTP REST (一元请求-响应)**                                                                                | **WebSocket (双向全双工长连接)**                                                                                              |
| **API 入口**                | **Interactions API** (`POST /v1beta/interactions`)                                                           | **Gemini Live API** (`wss://.../BidiGenerateContent`)                                                                         |
| **工作时序**                | 用户录音结束 $\to$ 一次性上传完整音频 $\to$ 接收完整转录文本                                                 | 用户说话过程中 $\to$ 边录音边持续推送音频切片 $\to$ 实时推回流式字幕                                                          |
| **输入数据**                | 完整音频（支持内存 Base64 内联 WAV/MP3/FLAC，最长 1 小时）                                                   | 音频数据流（必须分片推送 Raw 16-bit Mono 16kHz PCM，通常每片 100ms）                                                          |
| **输出事件**                | 单次返回最终文本，支持结构化解析                                                                             | 实时返回两级事件：<br>1. `interim_input_transcription` (极低延迟推测字幕)<br>2. `input_transcription` (停顿/分句权威确认文本) |
| **单会话上限**              | 最长 1 小时（开启时间戳/声纹分离时限 30 分钟）                                                               | 单会话最长 10 分钟                                                                                                            |
| **核心特性**                | 自动语言检测 (85+ 语言)、`smart` 模式 (去语气词/自动纠错/标点数字排版)、说话人分离、词级时间戳、自定义词汇表 | 实时极低延迟听写、`smart` 模式、实时推测字幕、自定义词汇表                                                                    |
| **与 Handy 现有架构匹配度** | **100% 契合**。Handy 当前的 `BatchTranscriptionProvider` 接口契约即为整段音频输入与单次文本输出。            | **不兼容现有批处理接口**。需要为 Handy 新增流式音频采集管道与实时悬浮窗显示。                                                 |

### 2. SDK 现状事实：Google 官方没有 Rust SDK

- Google 官方发布的 GenAI SDK 矩阵仅涵盖：
  - Python (`google-genai`)
  - TypeScript / Node.js (`@google/genai`)
  - Go (`google.golang.org/genai`)
  - Java (`com.google.genai`)
- **结论**：Handy-Cloud 是 Tauri (Rust + React) 桌面应用，转写与网络层运行在 Rust 后端。不能使用 npm 或 pip SDK，必须基于 Rust 生态原生接入：
  - 对于 Interactions API（REST）：直接使用项目已有的 `reqwest` + `serde_json`；
  - 对于 Live API（WebSocket）：需引入 `tokio-tungstenite` 建立长连接并序列化 JSON 帧。

---

## 三、一手 API 报文契约与技术规范

### 1. Interactions API (`gemini-3.5-transcribe`)

#### 请求规范

- **URL**: `https://generativelanguage.googleapis.com/v1beta/interactions`
- **Method**: `POST`
- **Headers**:
  - `x-goog-api-key: <API_KEY>`（或 URL Query 参数 `?key=<API_KEY>`）
  - `Content-Type: application/json`

#### 请求报文（Payload）

```json
{
  "model": "gemini-3.5-transcribe",
  "input": [
    {
      "type": "audio",
      "data": "<BASE64_ENCODED_WAV_AUDIO>",
      "mime_type": "audio/wav"
    }
  ],
  "generation_config": {
    "transcription_config": {
      "language_codes": [],
      "mode": "smart"
    }
  }
}
```

- `data`: 内存生成的标准 16kHz 16-bit 单声道 WAV 音频的 Base64 字符串；
- `language_codes`: 空数组 `[]` 表示自动检测 85+ 种语言并支持语种混杂；若用户在 Handy 设置了特定语言（如 `zh-CN`, `en-US`），可传递对应的 BCP-47 编码；
- `mode`: `"smart"` 启用智能听写（自动移除 “嗯/啊/这个” 等语气助词、修正口误并自动进行标点符号与阿拉伯数字/货币单位规整化）。若需要原始逐字稿，可设为 `{"type": "verbatim"}`。

#### 响应报文（Response）

```json
{
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
}
```

- **解析逻辑**：遍历 `steps`，筛选 `type == "model_output"`，从中提取 `content[].text` 并拼接即为最终转录结果。

---

### 2. Live API (`gemini-3.5-transcribe-live`)

#### 连接规范

- **URL**: `wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key=<API_KEY>`
- **协议**: WebSocket 双向流

#### 握手配置帧（Client -> Server）

连接建立后，客户端必须发送的第一条消息：

```json
{
  "setup": {
    "model": "models/gemini-3.5-transcribe-live",
    "generationConfig": {
      "responseModalities": ["TEXT"]
    },
    "inputAudioTranscription": {
      "languageCodes": []
    }
  }
}
```

#### 实时推流帧（Client -> Server，每 100ms 持续发送）

```json
{
  "realtimeInput": {
    "audio": {
      "data": "<BASE64_PCM_CHUNK>",
      "mimeType": "audio/pcm;rate=16000"
    }
  }
}
```

#### 结束流帧（Client -> Server，按键释放停止录音时发送）

```json
{
  "realtimeInput": {
    "audioStreamEnd": true
  }
}
```

#### 服务端事件帧（Server -> Client）

```json
{
  "serverContent": {
    "interimInputTranscription": {
      "text": "正在说话的推测片段..."
    },
    "inputTranscription": {
      "text": "说话停顿或本句结束的最终确认文本。"
    }
  }
}
```

---

## 四、Handy-Cloud 落地改造方案与路径设计

为了兼顾**紧急解除线上阻断**与**未来实时流式转录架构演进**，严禁采用中立妥协或过度设计，明确划分为两个落地阶段：

### 阶段一（P0 紧急就绪）：迁移至 Interactions API + `gemini-3.5-transcribe`

**目标**：在不改变 Handy 现有录音逻辑和架构前提下，彻底消除 HTTP 404 错误，恢复云端转写功能，并享受到 Gemini 3.5 专用语音识别模型的精度提升。

1. **后端 `src-tauri/src/providers/gemini.rs` 改造**：
   - 替换请求端点构建逻辑：由 `/v1beta/models/{model}:generateContent` 改为 `/v1beta/interactions`；
   - 重构请求体 Serde 数据结构：构造符合 Interactions API 的 `GeminiInteractionRequest`；
   - 音频数据源直接复用现有的 `encode_wav_in_memory`（标准 16kHz 16-bit Mono WAV 经 Base64 编码）；
   - 在 `generation_config` 中注入 `transcription_config: { "mode": "smart" }`，开启智能降噪与去语气词；
   - 重构响应体反序列化结构：解析 `steps[].content[].text` 提取转录文字；
   - 保持复用 `NetworkManager` 的全局代理连接池。

2. **配置与默认模型升级**：
   - 将系统默认云端模型由 `gemini-2.5-flash` 改为 `gemini-3.5-transcribe`；
   - 可选支持通用模型 `gemini-3.6-flash`（作为备用多模态大模型选项）；
   - 更新 `settings.rs`、`tray.rs` 与前端 `CloudSTTSettings.tsx` 的模型列表；
   - 补齐多语言字典（i18n）中的模型描述。

3. **优势**：
   - 改动范围严格局限在 `GeminiProvider` 及其配置层，约 100 行核心代码变动；
   - 原有快捷键流程、单通道音频缓冲区、历史记录重试、剪贴板自动粘贴等外围系统 100% 保持稳定。

---

### 阶段二（P1 架构升级）：扩展实时流式转录架构，接入 `gemini-3.5-transcribe-live`

**目标**：突破传统“按键录音 $\to$ 释放 $\to$ 等待上传 $\to$ 粘贴”的固有延迟，实现用户边说话、屏幕边实时出字的沉浸式打字体验。

1. **抽象流式接口契约（`StreamingTranscriptionProvider`）**：

   ```rust
   #[async_trait]
   pub trait StreamingTranscriptionProvider: Send + Sync {
       async fn start_stream(
           &self,
           options: &TranscriptionOptions,
           on_interim: Box<dyn Fn(String) + Send + Sync>,
           on_final: Box<dyn Fn(String) + Send + Sync>,
       ) -> Result<Box<dyn StreamingAudioSink>, String>;
   }

   pub trait StreamingAudioSink: Send + Sync {
       fn send_pcm_chunk(&mut self, pcm_chunk: &[i16]) -> Result<(), String>;
       fn finish(self: Box<Self>) -> Result<(), String>;
       fn cancel(self: Box<Self>) -> Result<(), String>;
   }
   ```

2. **新增 WebSocket 基础设施**：
   - 引入 `tokio-tungstenite` 依赖，支持通过全局 SOCKS5/HTTP 代理握手建立 WebSocket 连接；
   - 实现 `GeminiLiveStreamingProvider`，封装握手帧、PCM 切片发送（100ms 缓冲切片）与接收事件循环。

3. **改造音频录制核心（`audio.rs`）**：
   - 录音开始时打开流通道；
   - 采集线程在向本地环形缓冲区写入的同时，将 16kHz PCM 采样分发给流式通道；
   - 录音停止时发送 `audioStreamEnd`，等待最终分句收尾。

4. **录音悬浮窗（Overlay）实时预览**：
   - 前端 Overlay 监听 Tauri 事件 `streaming-interim-text`，实时动态渲染推测文字；
   - 最终文本回传后直接交由系统剪贴板粘贴。

---

## 五、技术风险与防御方案

1. **代理与 WebSocket 穿透性**：
   - Interactions API 使用标准 HTTPS POST，完全复用现有 `NetworkManager` 的 `reqwest` 代理池，零穿透风险；
   - Live API 使用 WebSocket 协议，部分企事业单位网络环境下的 HTTP 代理可能会拦截或超时关闭 WebSocket 连接，需要设计 WebSocket 心跳维持与优雅降级为 Batch API 的容灾策略。
2. **Base URL 自定义反代兼容性**：
   - Interactions API 规范将端点由 `/models/...` 简化为了顶层 `/interactions`，需同步更新 `build_request_url` 单元测试，确保反向代理用户填写的自定义域名（无论是否带有 `/v1beta` 前缀）均能正确解析为 `/v1beta/interactions`。
3. **音频尺寸与限制**：
   - Interactions API 的内联 JSON Base64 格式适用于桌面短语音（通常几秒至几分钟，Base64 仅几百 KB 到数 MB，完全在单次 HTTP 限制内）；
   - 官方建议对于超长录音（> 100MB）应先走 Files API 上传获取 URI，但桌面热键输入场景单次录音通常不超过 5 分钟（16kHz Mono 16-bit WAV 每分钟约 1.8MB），内联 Base64 是最轻量且低延迟的最佳方案。

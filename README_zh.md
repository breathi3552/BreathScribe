# BreathScribe

中文 | [English](README.md)

BreathScribe 是一款专注于高精度转写的桌面语音输入工具，支持本地离线 AI 模型与云端大模型语音识别。

## 云端语音转写 (Cloud Transcription)

采集麦克风音频并发送至云端语音识别接口，识别结果直接粘贴至当前焦点窗口光标处。

支持两种模式：

- **整段识别 (REST)**：录音停止后发送完整音频数据。
- **实时流式 (WebSocket)**：录音过程中实时分块传输 16kHz PCM 音频，实现边录边识别。

### 支持模型

| 模型                       | 模型 ID                      | 协议      | 说明                       |
| -------------------------- | ---------------------------- | --------- | -------------------------- |
| Gemini 3.5 Transcribe      | `gemini-3.5-transcribe`      | REST      | 录音结束后进行整段音频转写 |
| Gemini 3.5 Transcribe Live | `gemini-3.5-transcribe-live` | WebSocket | 录音期间实时双向流式转写   |

使用云端转写需提供 Google Gemini API Key。在「设置 > 云端语音转写 (Cloud STT)」中配置密钥与模型。

## 使用方式

1. 从 [Releases](https://github.com/breathi3552/BreathScribe/releases) 下载可执行文件。
2. 打开「设置 > 云端语音转写 (Cloud STT)」填入 Gemini API Key 并选择模型。
3. 按下录音快捷键（默认 `Ctrl+Space`）进行输入。

## 本地开发

环境要求：Rust、Bun。

```bash
bun install
bun run tauri dev
bun run tauri build
```

## 开源协议

MIT License，详见 [LICENSE](LICENSE)。

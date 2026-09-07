# BreathScribe

[中文](README_zh.md) | English

BreathScribe is a high-performance desktop speech-to-text application with cloud transcription support and local AI models.

## Cloud Transcription

Audio captured from the microphone is sent to a cloud transcription API. The resulting text is pasted directly into the active window at the cursor.

Supported modes:

- **Batch (REST)**: Audio is sent after recording stops.
- **Streaming (WebSocket)**: Audio chunks (16kHz PCM) are streamed during recording for real-time text output.

### Supported Models

| Model                      | Model ID                     | Protocol  | Description                                        |
| -------------------------- | ---------------------------- | --------- | -------------------------------------------------- |
| Gemini 3.5 Transcribe      | `gemini-3.5-transcribe`      | REST      | Full-audio transcription after recording ends      |
| Gemini 3.5 Transcribe Live | `gemini-3.5-transcribe-live` | WebSocket | Real-time streaming transcription during recording |

A Google Gemini API key is required. Configure the key and select a model in **Settings > Cloud STT**.

## Usage

1. Download the binary from [Releases](https://github.com/breathi3552/BreathScribe/releases).
2. Set your Gemini API key in **Settings > Cloud STT**.
3. Trigger recording with the hotkey (default: `Ctrl+Space`).

## Development

Prerequisites: Rust, Bun.

```bash
bun install
bun run tauri dev
bun run tauri build
```

## License

MIT License. See [LICENSE](LICENSE).

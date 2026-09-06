<p align="center">
  <img src="brand/breath-scribe-icon-source.svg" width="128" height="128" alt="BreathScribe Logo" />
</p>

<h1 align="center">BreathScribe</h1>

<p align="center">
  <strong>随时随地，由声音直达文字。双引擎驱动的桌面智能语音听写助手。</strong><br>
  <em>A smart, dual-engine speech-to-text desktop companion powered by local offline models & cloud LLMs.</em>
</p>

<p align="center">
  <a href="https://github.com/breathi3552/Handy-Cloud/releases"><img src="https://img.shields.io/github/v/release/breathi3552/Handy-Cloud?style=flat-square&color=38bdf8" alt="Release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="License" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%20(x64)-0284c7?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/built%20with-Tauri%202%20%2B%20Rust-orange?style=flat-square" alt="Built With Tauri & Rust" />
</p>

---

## 🌟 项目定位与上游致谢 (Upstream Heritage)

**BreathScribe** 是基于开源项目 [cjpais/Handy](https://github.com/cjpais/Handy)（原作者 [@cjpais](https://github.com/cjpais)）深度衍生的独立分支与云端增强版本。

- **致敬上游**：衷心感谢原作者与开源社区为 Handy 打造的高性能跨平台离线语音转写架构与极简交互理念；
- **独立品牌**：严格遵循上游开源协议与品牌保护准则，本项目全面启用全新独立品牌 **BreathScribe**、全新声波云朵视觉资产与独立包标识，不代表上游官方立场；
- **差异演进**：在上游优秀的离线转写基础上，专注于**云端大模型接入**、**低延迟流式听写**、**网络代理集成**与 **Windows 免安装便携化**打磨。

| 能力对比                                            | 上游原版 (cjpais/Handy) |      BreathScribe      |
| :-------------------------------------------------- | :---------------------: | :--------------------: |
| **本地离线模型** (Whisper / Parakeet / Moonshine)   |         ✅ 支持         |   ✅ 支持 (完全保留)   |
| **云端大模型转写** (Gemini Transcribe 批处理)       |        ❌ 仅离线        |        ✅ 支持         |
| **实时流式听写** (Gemini Live WebSocket 边说边出字) |        ❌ 仅离线        |  ✅ 支持 (毫秒级响应)  |
| **网络代理支持** (HTTP/HTTPS/SOCKS5 / 系统嗅探)     |       ❌ 无需网络       | ✅ 支持 (智能自动嗅探) |
| **免安装便携版** (`Data/` 目录本地隔离)             |     ⚠️ 依赖系统路径     |   ✅ 支持 (解压即用)   |
| **核心维护平台**                                    | macOS / Linux / Windows | **专注 Windows (x64)** |

---

## 🚀 核心特性 (Key Features)

### 1. 离线/云端双引擎协同 (Dual-Engine STT)

- **云端大模型通道**：原生支持 Google Gemini Transcribe（高精度整段识别）与 **Gemini Transcribe Live**（基于 WebSocket 双向流式实时听写，边说边打字）；
- **全套离线引擎**：保留 Whisper（Small/Medium/Turbo/Large）、Parakeet、Moonshine 等本地引擎，离线与云端通道按需一键切换；
- **智能切分与过滤**：集成 Silero VAD 静音过滤，精准过滤无效录音片段。

### 2. 全局与系统网络代理 (Network Proxy)

- **系统代理自动嗅探**：一键读取 Windows 系统当前代理配置并实时应用；
- **灵活手动协议**：内置支持 HTTP、HTTPS、SOCKS5 代理通道，确保在受限网络环境下稳定连接云端 API。

### 3. Windows 绿色免安装便携版 (Portable First)

- **解压即用**：零系统依赖，免管理员权限与复杂安装流程；
- **本地数据隔离**：配置与历史数据库完全存放于同级 `Data/` 目录中，不污染系统 `%APPDATA%`，随身 U 盘或网盘即开即走；
- **平滑无感升级**：内置旧版安装数据平移引擎，升级迁移零数据丢失。

### 4. 极致交互与桌面集成 (Desktop Integration)

- **全局快捷键**：支持「按住说话 (Push-to-Talk)」与「点按切换 (Toggle)」模式，适配各种输入习惯；
- **悬浮波形反馈**：半透明录音悬浮条实时呈现录音波形与转录状态，录音完毕自动将文字键入当前光标位置。

---

## 💻 平台支持范围 (Platform Scope)

- **当前重点专注**：**Windows 10 / 11 (x64)**。所有日常特性演进、构建产物、高 DPI 任务栏托盘优化与自动化 CI 均优先保障 Windows 平台体验。
- **macOS / Linux 说明**：若需轻量级纯本地离线转写体验，建议优先使用上游原版 [cjpais/Handy](https://github.com/cjpais/Handy)；如需在非 Windows 平台体验云端特性，可基于本仓库源码自行编译（参考 [BUILD.md](BUILD.md)）。

---

## ⚡ 快速上手 (Quick Start)

1. **下载解压**：前往 [Releases 页面](https://github.com/breathi3552/Handy-Cloud/releases) 下载最新便携版压缩包（`breath-scribe-portable-x64.zip`），解压至任意目录；
2. **启动配置**：双击运行 `breath-scribe.exe`，在托盘图标右键进入「设置」，配置你的 Google Gemini API Key（如有需要可同步开启代理）；
3. **开始听写**：按下全局听写快捷键，说完后文字自动流式或批量键入当前活跃光标位置。

### 常用命令行控制 (CLI)

```bash
breath-scribe.exe --toggle-transcription    # 触发/停止录音转写
breath-scribe.exe --cancel                  # 取消当前操作
breath-scribe.exe --start-hidden            # 启动时最小化至托盘
```

---

## 🛠️ 本地开发与构建 (Development)

本项目基于 **Tauri 2.x + Rust + React (TypeScript)** 构建：

```bash
bun install             # 安装前端依赖
bun run tauri dev       # 启动本地开发环境
bun run build:portable  # 编译 Windows 绿色便携版产物
```

详细构建与模型配置指南请参见 [BUILD.md](BUILD.md)。

---

## 📄 开源许可与品牌说明 (License & Trademark)

- **代码许可**：本项目代码继承遵循 [MIT License](LICENSE)，保留原作者 Copyright (c) 2025 CJ Pais 与本项目的相关增量提交；
- **品牌资产**：上游 Handy 的名称、徽标与原版图标归原作者所有；本项目采用的 **BreathScribe** 名称、声波云朵图形资产及衍生界面受独立开源维护，与原作者无关，不暗示任何官方背书。

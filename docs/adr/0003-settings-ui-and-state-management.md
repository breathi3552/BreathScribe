# 设置界面 UI 与状态管理架构决策（变体 A：深度整合型）

Handy-Cloud 需要在前端呈现全局网络代理配置（`ProxySettings`）与云端语音转写配置（`CloudSTTSettings`），同时保持与原有 Handy 设置界面的视觉语言高度一致、避免过度破坏既有侧边栏导航层次。

## 决策内容

1. **采用变体 A（深度整合型 In-Place）**：
   - **零新增顶层菜单**：不增加侧边栏层级，维持原有 5 大主功能项（通用、历史记录、转录模型、高级、关于）；
   - **网络与代理配置（`ProxySettings`）下沉至【高级设置 (Advanced)】**：
     - 作为全局基础设施，在【高级设置】顶部新增 `网络与代理 (Network & Proxy)` 的 `SettingsGroup` 卡片；
     - 代理策略支持三种模式切换：`跟随系统代理 (System，默认)`、`手动配置代理 (Manual)`、`直接连接 (Direct)`；
     - 手动模式下平滑展开协议（HTTP/SOCKS5）、服务器、端口及认证（用户名/密码）表单；
     - 卡片内嵌“测试代理连通性”操作，即时反馈与 Google 服务的连通状态与往返延迟（RTT）。
   - **云端 STT 配置（`CloudSTTSettings`）整合于【转录模型 (Models)】**：
     - 在【转录模型】页面顶部增加分段选择器（Segmented Control）：`[ 本地离线模型 ]` / `[ 云端 API (Gemini) ]`；
     - 处于“本地离线模型”时，展示原有已下载模型卡片与管理；
     - 处于“云端 API (Gemini)”时，平滑切换为云端配置面板（服务商选择、模型选择、API Key 密码输入与眼睛图标切换、可选自定义 Base URL、凭据验证按钮）；
     - 提示显存自动卸载行为（依赖后端已决议的 `model_unload_timeout` 闲置机制）。
   - **底部状态栏指示**：
     - 左下角状态胶囊动态联动：处于云端模式显示 `☁️ Gemini 2.5 Flash ⌵`，本地模式显示 `● Qwen3-ASR 0.6B ⌵`；
     - 右侧追加全局网络代理生效摘要。

2. **前端状态管理（Zustand Store）设计**：
   - 在 `settingsStore.ts` 中扩展字段：

     ```typescript
     interface ProxySettingsState {
       proxy_mode: "System" | "Manual" | "Direct";
       proxy_protocol: "Http" | "Socks5";
       proxy_server: string;
       proxy_port: number;
       proxy_auth_required: boolean;
       proxy_username?: string;
       proxy_password?: string;
     }

     interface CloudSTTSettingsState {
       transcription_mode: "Local" | "Cloud";
       cloud_provider: "Gemini" | "GoogleCloudSTT";
       gemini_model: string;
       gemini_api_key: string;
       gemini_base_url?: string;
     }
     ```

   - 遵循 Handy 既有的响应式持久化链路：前端通过 `commands.update_settings` 下发，后端更新并广播事件，前端 Zustand Store 订阅同步。

3. **原型工件归档**：
   - 交互原型代码保留在 `prototypes/settings-ui-prototype.html`，作为一手视觉设计规范。

## 备选方案与否决原因

- **否决变体 B（独立专属页面型）**：在侧边栏新增“云端转写”与“网络代理”导致侧边栏垂直空间过挤，且与上游 Handy 仓库后续版本合并时容易产生冲突。
- **否决变体 C（混合矩阵控制台）**：过度重构了转录模型页的整体交互习惯，增加了普通离线用户的认知成本。

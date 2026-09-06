# 独立品牌重塑（BreathScribe）与无缝数据迁移架构决策

在上游项目 [cjpais/Handy](https://github.com/cjpais/Handy) 中，核心代码采用 MIT 开源协议，但“Handy”名称、卡通小手 Logo、图标及关联品牌资产并不属于开源授权范围。上游维护者在 Issue #27 中明确要求衍生分支建立独立品牌。为此，本项目需完成全面的品牌资产剥离与重塑，确立全新的视觉符号与元数据体系，同时保障既有用户的平滑过渡。

## 决策内容

1. **全新品牌命名与标识符（BreathScribe）**：
   - 品牌名称确定为 **BreathScribe**，寓意“声音与呼吸皆可落字成篇，由云端与本地双引擎共同记录速记”。
   - 二进制可执行文件名统一为 `breath-scribe.exe`，Rust 包名与二进制目标定为 `breath-scribe`。
   - Tauri 客户端应用标识符更新为 `io.github.breathi3552.breathscribe`。
   - 前端包名与展示标题同步更新为 `BreathScribe`。

2. **视觉标识彻底去“小手化”并建立“声波云朵”符号（Soundwave & Cloud Motif）**：
   - 彻底移除原版所有由卡通小手构成的矢量路径（`HandyHand`）及托盘手形轮廓。
   - 确立全新视觉符号：由代表音频输入的“立体声波律动”与代表云端推理的“流线云朵底座”融合构成。
   - 保留团队已验证的天青/蔚蓝（#38bdf8 / #0284c7 渐变）经典科技配色方案，维持深浅色任务栏与 UI 界面的一致性。

3. **双重运行模式下的非破坏性数据迁移机制**：
   - **安装模式（Installed Mode）**：首次启动检测到新应用数据目录（`%APPDATA%\io.github.breathi3552.breathscribe`）为空、且存在旧目录（`%APPDATA%\io.github.breathi3552.handycloud`）时，自动触发非破坏性安全迁移。配置文件 `settings` 与历史数据库 `history.db` 完整复制；针对数 GB 级的已下载模型文件，优先采用 NTFS 硬链接（Hard Link）实现即时挂载且零磁盘额外消耗，不支持跨卷时降级为复制。
   - **便携模式（Portable Mode）**：启动探测兼容原版 `"Handy Portable Mode"` 与全新 `"BreathScribe Portable Mode"` 标记文本，直接挂载同级 `Data/` 目录，旧便携版用户替换可执行文件即可无缝延续使用。

## 备选方案与否决原因

- **否决在原名上增加修饰（如 Handy-Cloud / Handy-Next）**：直接违反上游知识产权规范中关于分支衍生版“不得包含原产品名以避免品牌混淆与代言暗示”的要求。
- **否决已有市场重名方案（如 VoxType、VoxFlow、AuraType、DictaFlow 等）**：经全网开源生态与商业软件检索，这些名称均存在同类语音识别或打字项目，采纳将带来二次品牌污染与用户混淆。
- **否决原子重命名/移动旧数据目录（In-place Move）**：直接对旧目录执行剪切移动会导致用户无法降级或回滚旧版本，违背“用户数据安全至上”原则。

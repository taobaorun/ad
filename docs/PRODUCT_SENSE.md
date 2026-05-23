# 产品理念

AD 是给 Claude Code 重度用户的"配置管理器"，不是给所有人的工具。

## 目标用户

**核心画像**：每天用 Claude Code 工作，并且需要在多个 Anthropic 账号 / 不同 base URL / 不同 model 偏好之间切换的开发者。

不是目标用户：
- 偶尔用一下 Claude Code 的人——他们手改 settings.json 就够了
- 非开发者——AD 是 menubar 工具，UI 充满了 JSON 字眼，不友好

## 核心价值

| 用户痛 | AD 的回答 |
|---|---|
| 手改 `~/.claude/settings.json` 容易写错 | profile 编辑器 + 校验，结构化输入 |
| 切换账号要复制粘贴一长串 env vars | profile 一键应用 |
| 改完忘了改回来 | 历史记录 + 一键 restore 之前的备份 |
| 多个项目想用不同配置 | 分层 profile（计划中），写到项目本地不污染全局 |

## 不做的事

- **不做云同步**：profile 是高度敏感的（API key），跨设备同步是另一个项目
- **不做 GUI 调用 Claude Code 本身**：AD 不嵌入 PTY、不做 chat UI，只管配置
- **不做团队协作**：AD 是单机工具
- **不做 Anthropic 之外的 AI 平台**：作用域明确就是 Claude Code

## 设计取舍的优先级

当多个目标冲突时，按以下顺序权衡：

1. **数据安全** > 一切。宁可多一次确认 dialog，不能丢用户的 settings
2. **可发现性** > 优雅。tray + 主窗口都要呈现关键状态（哪个 profile active）
3. **零配置上手** > 高度定制。第一次启动应该能直接用，不需要看文档
4. **macOS 原生感** > 跨平台一致性。颜色、字体、动效跟 macOS 走

## 当前的产品张力

**全局 settings 覆盖 vs. 实例级隔离**：当前架构是覆盖 `~/.claude/settings.json`，影响所有已运行的 CC 实例。重构方向是用 CC 自身的分层配置（`.claude/settings.json` + `.claude/settings.local.json` + env vars），把"profile"变成"可应用到指定项目的分层 recipe"。详细方案见 `docs/exec-plans/active/`（待）。

## 命名

- 产品名 `AD`（大写显示）
- 包名 / crate 名 `ad`（小写，遵循生态约定）
- bundle id `com.jiaxy.ad`

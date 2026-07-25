# 设计文档索引

本目录存放 AD 系统架构和模块设计的详细文档。

**本目录的具体设计文档使用 MD + HTML 双格式**：MD 是模型读取和持续更新的真理源，HTML 是同步的人读评审视图（激活 `html-artifact` skill 编写）。架构图、时序图、数据流图和决策树优先在 HTML 中可视化。

例外：`core-beliefs.md` 这类没有可视化需求的纯文字设计信念清单可以保持 MD。

## 文档列表

| 文档 | 描述 | 状态 |
|---|---|---|
| [architecture.md](architecture.md) / [HTML](architecture.html) | 系统架构 v2.0：多 Agent 边界、安全执行、双窗口、运行时数据与代码结构 | 已实现 |
| [multi-agent-architecture.md](multi-agent-architecture.md) / [HTML](multi-agent-architecture.html) | 多 Agent v1 抽象：Definition/Installation/Context、capability ports、resource snapshot、MutationPlan 与安全执行引擎 | 已实现 |
| [agent-conversion-workbench.md](agent-conversion-workbench.md) / [HTML](agent-conversion-workbench.html) | Agent 配置转换工作台：完整 artifact inventory、多载体路由、可执行 resolution 与危险权限确认 | 已实现 |
| [codex-plugin-conversion.md](codex-plugin-conversion.md) / [HTML](codex-plugin-conversion.html) | Codex Plugin 自动转换：Project Codex Home、共享登录、默认配置继承、隔离安装与 LSP 边界 | 已实现 |
| [conversion-preview-progress.md](conversion-preview-progress.md) / [HTML](conversion-preview-progress.html) | 转换预览真实进度：每次调用独享 Channel、语义阶段事件、耗时与无障碍状态反馈 | 已实现 |
| [theme-system.md](theme-system.md) / [HTML](theme-system.html) | AD 主题系统：Catppuccin Mocha/Latte palette、语义 token、组件状态、编辑器与无障碍约束 | 已实现 |
| [core-beliefs.md](core-beliefs.md) | AD 核心设计信念和原则 | 活跃 |
| [ui-redesign-options.html](ui-redesign-options.html) | UI 整体重设计 Stage 2 多方案探索：现状 9 痛点 + 3 个方向（项目工作台 / 配方工作台 / 关系画布）并排对比 + 同页内可交互原型（A/B/C tab 切换） | 已完成（A′ cmux 方向已实现，见 exec-plans/completed/ui-redesign.md） |
| [terminal-launcher.html](terminal-launcher.html) | 外部终端 launcher 设计：4 个 backend 行为契约、Ghostty/AppleScript 取舍、cmux 模型兼容、Custom 模板规范、错误反馈策略 | 活跃 |

## 如何添加新设计文档

1. 在本目录创建同名 MD + HTML（MD 为真理源，HTML 激活 `html-artifact` skill 的 report 场景）
2. 更新本索引，添加一行链接 + 一句话描述
3. 文档应包含：背景、设计决策、权衡考量、实现要点

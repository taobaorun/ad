# 设计文档索引

本目录存放 AD 系统架构和模块设计的详细文档。

**本目录的具体设计文档使用 HTML 格式**（Tier 3 HTML native，激活 `html-artifact` skill 编写）。架构图、时序图、数据流图、决策树这些可视化信息是设计文档的核心价值，Markdown 表达不了。本索引文件保持 Markdown，方便 Agent 快速扫描。

例外：`core-beliefs.md` 这类"纯文字的设计信念清单"可以保持 MD（没有需要可视化的内容）。

## 文档列表

| 文档 | 描述 | 状态 |
|---|---|---|
| [architecture.md](architecture.md) | 系统架构 v1.0：双窗口、项目配置模型、Skill 管理、运行时数据、项目结构 | 活跃 |
| [core-beliefs.md](core-beliefs.md) | AD 核心设计信念和原则 | 活跃 |
| [ui-redesign-options.html](ui-redesign-options.html) | UI 整体重设计 Stage 2 多方案探索：现状 9 痛点 + 3 个方向（项目工作台 / 配方工作台 / 关系画布）并排对比 + 同页内可交互原型（A/B/C tab 切换） | 已完成（A′ cmux 方向已实现，见 exec-plans/completed/ui-redesign.md） |
| [terminal-launcher.html](terminal-launcher.html) | 外部终端 launcher 设计：4 个 backend 行为契约、Ghostty/AppleScript 取舍、cmux 模型兼容、Custom 模板规范、错误反馈策略 | 活跃 |

## 如何添加新设计文档

1. 在本目录创建新的 HTML 文件（激活 `html-artifact` skill 的 report 场景）
2. 更新本索引，添加一行链接 + 一句话描述
3. 文档应包含：背景、设计决策、权衡考量、实现要点

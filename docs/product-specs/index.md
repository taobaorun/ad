# 产品规格索引

本目录存放 AD 的产品功能需求和用户故事。

**本目录的具体规格文档使用 MD + HTML 双格式**：MD 是模型读取和持续更新的真理源，HTML 是同步的人读评审视图（激活 `html-artifact` skill 编写）。

## 产品愿景

给 Claude Code 重度用户的 macOS 配置管理器：管理多个 settings profile、一键切换、永不丢数据。

## 功能规格列表

| 文档 | 功能名称 | 优先级 | 状态 | 摘要 |
|---|---|---|---|---|
| [multi-agent-support.md](multi-agent-support.md) / [HTML](multi-agent-support.html) | 多 Agent 支持基础 | P0 | v1 架构已批准，实施中 | 支持 Claude Code、Codex 及未来内置 Agent；新抽象见 [设计文档](../design-docs/multi-agent-architecture.md) |

## 优先级定义

- **P0**：核心功能，必须完成（profile CRUD、激活、备份）
- **P1**：高价值（分层 profile、tray 状态、历史 restore）
- **P2**：体验提升（拖拽导入、批量操作、键盘快捷键）
- **P3**：探索性（自动同步、模板市场——目前不做）

## 如何添加新规格

1. 在本目录创建同名 MD + HTML（MD 为真理源，HTML 激活 `html-artifact` skill 的 spec / report 场景）
2. 更新本索引，添加一行
3. 规格应包含：用户故事、验收标准、UI 草图、边界条件

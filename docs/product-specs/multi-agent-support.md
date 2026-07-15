# 多 Agent 支持产品规格

> 状态：已实现，发布门禁验证中（2026-07-15）
>
> 设计依据：`docs/design-docs/multi-agent-architecture.md`

## Objective

将 AD 从 Claude Code 专用配置管理器演进为管理多个 coding Agent 的 macOS 桌面应用。第一阶段支持 Claude Code 和 Codex，并保证以下用户任务能力对等：

- 用户级和项目级配置管理；
- profile 创建、编辑、应用、历史和恢复；
- Skills 管理；
- Plugins 管理；
- 运行进程探测；
- 从现有四种 terminal backend 启动正确 Agent；
- Claude Code setup 非破坏性转换为 Codex setup。

成功体验：用户选择一个 canonical Agent installation 和项目后，可以看到该上下文的真实配置来源、可执行能力和限制；任何写操作都先预览，写前备份，失败有明确结果且可恢复。

## Implementation Status

- Claude Code 与 Codex 的 Settings、Profiles、Skills、Plugins、进程探测和终端入口已统一到 AgentContext/capability-driven UI。
- Profile 创建、编辑、应用、history 和 rollback 使用 adapter-owned payload 与共享安全执行路径。
- Claude Code → Codex 转换逐 artifact 展示无法转换、需确认和冲突项，source 保持只读，target 支持 backup 和 rollback。
- Plugin install 不做能力伪装：Claude 当前无 install operation；Codex marketplace/cache/授权尚未纳入安全 MutationPlan，因此 descriptor 标记 degraded，调用返回结构化 `Unsupported`。列表和 enable/disable 已实现。
- legacy Claude template/import/shortcut façade 仍保留给兼容入口，不属于未来 Agent 扩展面。

## User Stories

1. 作为同时使用 Claude Code 和 Codex 的开发者，我可以分别管理二者的配置，不会串写文件。
2. 作为拥有多个 Codex config home 的用户，我可以选择正确 installation，而不是被固定到 `~/.codex`。
3. 作为 profile 用户，我可以在相同 profileId 下分别保存 Claude 和 Codex 内容，其身份为 `(agentId, profileId)`。
4. 作为迁移用户，我可以把 Claude setup 转换为独立 Codex setup，Claude 源文件始终不变。
5. 作为谨慎用户，我能在覆盖已有目标前看到差异、冲突和无法转换项，并在确认后才写入。
6. 作为未来 Agent 的使用者，我获得同一套用户流程；不支持或退化的操作会明确说明，而不是假成功。

## Product Model

### AgentDefinition

编译进 AD 的 Agent 产品类型。首期只有 `claude-code` 和 `codex`；用户不能通过配置声明 adapter。

### AgentInstallation

本机一个 canonical 配置实例。identity 由 adapter 根据有效 config home 判定；发现结果去重后只保留 canonical installation，不保留 aliases。

### AgentContext

当前操作目标：一个 installation，加上可选项目路径。UI 和 store 不再只记录 agentId。

### AgentProfile

使用 `(agentId, profileId)` 复合身份的配置 recipe。通用 envelope 保存元数据和 payload schema；内容由对应 adapter 验证，不共享 Claude/Codex schema。

### Managed Resource

adapter allowlist 中的配置对象，例如 settings、instructions、skills、plugins、hooks、MCP、agents 或 rules。auth、tokens、sessions、history、logs、cache 和数据库永不属于 managed resource。

### Capability

由可调用能力端口推导。除能力名称外，还必须报告 user/project scope、可用 operation、availability 和 limitations。

## Core Workflows

### 1. Discover and select

1. 内置 adapter 从已知默认位置、环境、可执行入口和用户确认位置发现 candidates。
2. adapter 校验并计算 canonical identity。
3. registry 合并重复 candidates，只返回 canonical installations。
4. 用户选择 installation；项目操作同时选择 projectPath，形成 AgentContext。

### 2. Inspect and edit

1. UI 请求当前 AgentContext 的 capability descriptors 和 resources。
2. adapter 读取 allowlist resource，返回 location、format、content snapshot 和 digest。
3. UI 使用通用编辑器或集中注册的 built-in editor 编辑 profile/resource。
4. adapter 验证 Agent-specific payload 并生成 preview，不直接写盘。

### 3. Preview and apply

1. adapter 根据当前 snapshots 生成 backend-owned MutationPlan。
2. UI 展示变更、冲突、限制和目标位置；不接收可修改的真实 mutation plan。
3. 用户确认时只提交 planId。
4. shared ExecutionEngine 重新检查 digest，先完成全部备份，再逐文件原子写。
5. UI 展示 complete、compensated 或 partial failure receipt。

### 4. Rollback

1. 用户从 operation receipt 选择恢复。
2. ExecutionEngine 检查当前目标是否仍等于 apply 后 digest。
3. 未被外部修改时恢复 backup；已被修改时拒绝静默覆盖并要求重新审阅。

### 5. Claude Code → Codex

1. 分别快照 source 和 target setup。
2. 按 artifact 分类转换，不把所有内容压成一个 TOML 文档。
3. 每项标记 exact、mapped、requires_input、unsupported、conflict 或 unchanged。
4. source resource 只能进入 read-set，永远不能进入 write-set。
5. 用户确认后通过相同 ExecutionEngine 写入 Codex target；Claude setup 保持不动。

## Capability Parity

| 用户能力 | Claude Code | Codex | 验收要求 |
|---|---|---|---|
| Settings | P0 | P0 | user/project 读取、编辑、预览、apply、backup、history、rollback，未知字段保留 |
| Profiles | P0 | P0 | `(agentId, profileId)` 隔离，adapter-owned payload；创建、编辑、plan/apply、history、rollback |
| Skills | P0 | P0 | 列表、来源识别、安装/启用/禁用及真实 scope |
| Plugins | P0 | P0 | 列表和启用/禁用对等；install 只在真实安全实现存在时声明，否则 degraded/unsupported |
| Process detection | P0 | P0 | 不误报另一个 Agent 或 config instance |
| Terminal launch | P0 | P0 | 正确 launcher、env、cwd 和 terminal backend |
| Conversion source | P0 | 不适用 | Claude setup 只读 |
| Conversion target | 不适用 | P0 | preview、conflict、backup、apply、rollback |

“对等”表示用户任务和安全保证对等，不表示字段或底层文件结构相同。真实平台缺少某项 operation 时必须报告 degraded/unavailable，不能伪造成功。

## UI Requirements

- 主窗口提供 Agent installation selector；多个 installation 必须可区分。
- capability descriptor 控制入口、操作状态和 disabled reason。
- profile、project state、history 和 receipt 按 AgentContext 隔离。
- conversion preview 展示 artifact、目标位置、差异、disposition 和所需用户输入。
- 所有用户文案进入 zh/en i18n；后端错误 message 保持英文，前端按 error code 映射可操作提示。
- Agent-specific editor 只能在集中 registry 注册，通用组件和 store 不散布 agentId 业务分支。

## Data Safety

- 用户文件写入必须经过 MutationPlan + explicit confirmation。
- 写入前保存所有目标 backup 和 manifest；任一 backup 失败时不得开始写入。
- 每个文件使用 APFS atomic rename；多文件更新使用补偿恢复，不宣称整体原子事务。
- source/target 外部修改通过 digest 检测；过期 plan 必须重新预览。
- rollback 不覆盖 apply 后的用户新修改。
- auth/token/session/history/log/cache/database 不读取、不展示、不备份、不记录日志。

## Commands

```bash
pnpm typecheck
pnpm lint
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
```

## Testing Strategy

### Rust unit tests

- typed ID、serde contract、capability descriptor derivation；
- canonical installation identity 和 deterministic dedup；
- Agent-specific parser/validator 和未知字段保留；
- plan invariants、digest conflict、receipt status。

### Rust integration tests

- 通过 `AD_HOME=<tempdir>` 隔离 Claude/Codex fixtures；
- backup-before-write、fault injection、compensation 和 rollback；
- source unchanged、target changed、plan expiry/replay；
- sensitive/runtime file exclusion。

### TypeScript and UI tests

- IPC zod schema、AgentContext migration 和 store isolation；
- capability gating、structured errors 和 editor registry；
- conversion preview/confirm/receipt/rollback；
- zh/en key parity。

### macOS behavior verification

- 双窗口状态同步；
- Claude/Codex 同时存在及多个 Codex config home；
- Ghostty、cmux、Terminal.app、Custom launcher；
- 隔离临时 home/project 完成 inspect → preview → apply → rollback；安装包在真实 macOS 上完成只读启动与导航验收。

## Boundaries

### Always

- 由 adapter 解析 Agent-specific 路径和格式。
- 由 shared ExecutionEngine 统一写盘。
- 对外返回结构化错误和 schema-validated IPC view。
- 保持旧 Claude profile 和 IPC 行为兼容，直到迁移测试通过。

### Ask first

- 新增依赖或 Tauri 权限；
- 修改现有 Claude 文件语义；
- 自动删除用户文件；
- 改变默认 Agent 或迁移存储位置；
- 扩大 managed resource allowlist 到认证或运行时状态。

### Never

- 用户自定义或动态加载 adapter；
- 未备份覆盖目标；
- conversion 修改 source；
- 由前端提交任意目标路径或 mutation content；
- 把不支持或 partial failure 当成成功；
- 在日志、错误或 fixture 中写入 secret/token。

## Success Criteria

- Claude Code 和 Codex 五类 P0 能力通过共享 operation-level contract suite。
- 同一 Agent 多来源 discovery 只返回 canonical installation；不同 config home 不被错误合并。
- profile、project state、history、backup 和 receipt 不跨 AgentContext 串写。
- Claude → Codex source 字节保持不变；已有 target 在确认前不写入。
- backup、digest conflict、compensation、partial receipt 和 rollback 行为均有故障注入测试。
- 新增一个 built-in stub adapter 时，只需在后端和集中前端 registry 注册，不修改通用业务路径。
- 全量 typecheck、lint、frontend tests、Rust tests 和 build 通过。

## Open Questions

当前无阻塞性开放问题。后续可独立规划 Claude/Codex Plugin acquisition 安全执行与 legacy façade 清理；在完成前维持当前 degraded/compatibility 状态。Codex 具体 resource path、scope 和 operation 随官方版本变化时只更新 Codex adapter，不改变通用 contract。

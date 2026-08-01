# 多 Agent 支持产品规格

> 状态：已实现并通过 Project Agent Workspace 发布验证（2026-08-01）
>
> 设计依据：`docs/design-docs/multi-agent-architecture.md`
>
> 项目级产品合同与证据矩阵：`docs/product-specs/project-agent-workspace.md`

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

- Claude Code 与 Codex 的 Settings、Profiles、Skills、Plugins、进程探测和终端入口已统一到 AgentContext/capability-driven UI；`ProjectDetail` 是 Settings、Skills、Plugins、conversion 和 scoped History 的唯一项目工作区。
- Profile 创建、编辑、应用、history 和 rollback 使用 adapter-owned payload 与共享安全执行路径。
- Claude Code → Codex 转换按 Settings、Permissions/Rules、Skills、Plugins/Marketplaces 分组展示真实 source/target 路径、无法转换、需确认和冲突项；source 保持只读，Settings/Skills target 支持 backup 和 rollback。
- 单一配置实例不再显示误导性的 `~/.claude` / `~/.codex` 下拉框；危险的 `never + danger-full-access` 必须经过独立确认和后端 plan-bound acknowledgement。
- Plugin install 不做能力伪装：Claude 当前无 install operation；Codex Project install 已把 marketplace/cache/config 纳入安全 MutationPlan，并在 AD Managed Project Codex Home 中自动完成。Codex User install 仍依赖官方 marketplace flow，因此 capability 保持 degraded。列表和 enable/disable 已实现。
- Project Codex runtime 只读继承 Base config，隔离 Plugin 状态，并通过 AD scoped launch 注入 `CODEX_HOME`。文件型用户登录跨项目复用；Keychain-only 明确阻塞；API Key 等项目差异由可选 Profile 负责。
- legacy Claude template/import/shortcut façade 仅保留 Profile 兼容入口；legacy Project Skill 直写与旧 ProjectState 直写命令已退役，不能绕过统一 project operation contract。

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
3. 普通计划确认时提交 planId；转换计划同时提交后端 plan view 声明的 typed acknowledgements。
4. shared ExecutionEngine 重新检查 digest，先完成全部备份，再逐文件原子写。
5. UI 展示 complete、compensated 或 partial failure receipt。

### 4. Rollback

1. 用户从 operation receipt 选择恢复。
2. ExecutionEngine 检查当前目标是否仍等于 apply 后 digest。
3. 未被外部修改时恢复 backup；已被修改时拒绝静默覆盖并要求重新审阅。

### 5. Claude Code → Codex

1. 用户明确选择 User 或 Project；每次只转换一个 scope。
2. User 只处理用户配置；Project 绑定当前项目，将 Claude shared/local 项目层转换到同项目 `.codex/config.toml`，不改动 Claude/Codex 用户配置。
3. 分别快照所选 scope 的 source 和 target setup。
4. 按 artifact 分类转换，不把所有内容压成一个 TOML 文档。
5. 每项标记 exact、mapped、partial、requires_input、unsupported、conflict 或 unchanged。
6. `maxContextTokens` 自动转换为 `model_context_window`；Claude model 和 permissions 不猜测，分别通过内置 Codex model 输入和权限预设解决。
7. 无安全可写项时明确提示 source 已读取并汇总受阻项，不显示成执行成功。
8. source resource 只能进入 read-set，永远不能进入 write-set。
9. Project Skill 经用户确认本地来源后写入同项目 `.agents/skills`，并与 Settings 变更组成同一个可回滚计划。
10. Project-only Plugin 和每个 marketplace 来源必须可见；兼容 package 自动写入派生 Project Codex Home，未验证组件标为 partial/unsupported，不伪造成功。
11. 细粒度 permission rules 与权限预设分开报告；危险预设必须经过独立确认，后端拒绝缺失或错误 acknowledgement。
12. 用户确认后通过相同 ExecutionEngine 写入 Codex target；Claude setup 保持不动。

## Capability Parity

| 用户能力          | Claude Code | Codex  | 验收要求                                                                                      |
| ----------------- | ----------- | ------ | --------------------------------------------------------------------------------------------- |
| Settings          | P0          | P0     | user/project 读取、编辑、预览、apply、backup、history、rollback，未知字段保留                 |
| Profiles          | P0          | P0     | `(agentId, profileId)` 隔离，adapter-owned payload；创建、编辑、plan/apply、history、rollback |
| Skills            | P0          | P0     | 列表、来源识别、安装/启用/禁用及真实 scope                                                    |
| Plugins           | P0          | P0     | 列表和启用/禁用对等；Project install 使用隔离 runtime，User install 仍 degraded/unsupported   |
| Process detection | P0          | P0     | 不误报另一个 Agent 或 config instance                                                         |
| Terminal launch   | P0          | P0     | 正确 launcher、env、cwd 和 terminal backend                                                   |
| Conversion source | P0          | 不适用 | Claude setup 只读                                                                             |
| Conversion target | 不适用      | P0     | user/project 单作用域隔离、preview、conflict、backup、apply、rollback                         |

“对等”表示用户任务和安全保证对等，不表示字段或底层文件结构相同。真实平台缺少某项 operation 时必须报告 degraded/unavailable，不能伪造成功。

这里的对等是 **managed-Agent configuration parity**，不是 **automation-access parity**。当前所有风险确认都由第一方桌面 UI 完成；AD 不提供公共 CLI、MCP、deep-link 或远程 approval API，也不把第三方 Skill、Plugin、hook 或 MCP 的执行环境宣称为隔离沙箱。

## UI Requirements

- 主窗口提供 Agent installation selector；多个 installation 必须可区分。
- capability descriptor 控制入口、操作状态和 disabled reason。
- profile、project state、history 和 receipt 按 AgentContext 隔离。
- conversion 允许明确选择 User / Project；Project 绑定当前选中项目并显示路径，切换 scope 或项目后旧预览失效。
- conversion preview 按 carrier 分组展示 artifact、真实源/目标位置、summary、disposition、risk 和所需 typed resolution。
- 单 installation 时隐藏配置实例选择器；多 installation 收入高级区并说明它不代表实际写入路径。
- conversion 提供内置 Codex model、权限和本地 Skill 决策；默认保留目标权限，危险的无审批完全访问必须显式选择并通过独立 alert dialog 确认。
- 所有用户文案进入 zh/en i18n；后端错误 message 保持英文，前端按 error code 映射可操作提示。
- Agent-specific editor 只能在集中 registry 注册，通用组件和 store 不散布 agentId 业务分支。

## Data Safety

- 用户文件写入必须经过 MutationPlan + explicit confirmation；危险 conversion acknowledgement 必须与 backend-owned plan 精确匹配。
- 写入前保存所有目标 backup 和 manifest；任一 backup 失败时不得开始写入。
- 每个文件使用 APFS atomic rename；多文件更新使用补偿恢复，不宣称整体原子事务。
- source/target 外部修改通过 digest 检测；过期 plan 必须重新预览。
- rollback 不覆盖 apply 后的用户新修改。
- auth/token/session/history/log/非 Plugin cache/database 不读取、不展示、不备份、不记录日志；Project runtime 只管理 auth symlink 路径元数据。

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
- Project conversion 的 artifact、read-set、write-set、真实写盘和 rollback 均不跨入 user scope；
- Project local fixture 覆盖 Claude 原生 model、`maxContextTokens` 和 bypass permissions：默认预览只安全映射 context window，显式决策后才覆盖 Codex model/permissions；
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

当前无阻塞性开放问题。后续可独立规划 Codex User Plugin acquisition、Keychain 共享 Home 路线、完整 Profile 凭据管理、LSP→MCP bridge 与 legacy façade 清理。Codex 具体 resource path、scope 和 operation 随官方版本变化时只更新 Codex adapter，不改变通用 contract。

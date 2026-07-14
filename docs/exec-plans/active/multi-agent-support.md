# Multi-Agent 支持 ExecPlan

本 ExecPlan 是一个活文档。当前处于 Stage 3 执行；已冻结的 HTML 仍是首次批准基线，实时设计与进展以本 MD 和 `docs/design-docs/multi-agent-architecture.md` 为准。

## 目的 / 全局视角

将 AD 从 Claude Code 专用配置管理器演进为 Agent-neutral 的多 Agent 管理器。第一阶段支持 Claude Code 与 Codex 的能力对等：配置、Skills、Plugins、进程探测、终端启动、profile、备份、历史和恢复。

跨 Agent 转换必须是非破坏性的：Claude Code → Codex 只生成 Codex 目标配置，源配置保持不动。目标已存在时，先预览差异和无法转换字段，用户确认后备份目标并写入，失败可回滚。

## 确认状态

- [x] **用户已确认** — 评审 HTML 路径：docs/exec-plans/active/multi-agent-support.html
- [x] 用户已确认，开始执行（2026-07-14 Asia/Shanghai）
- [x] **设计重开已批准，恢复实施** — `docs/design-docs/multi-agent-architecture.html`（2026-07-15 Asia/Shanghai，用户 LGTM）

## 假设

- macOS only；继续使用 APFS rename 和现有 AD_HOME 测试隔离。
- Agent 只通过 AD 内置 Rust adapter 接入，不支持用户通过配置声明 adapter。
- Agent discovery 可能从多个来源返回同一个实际安装；统一层只保留 canonical installation，不保留 aliases。
- profile 的持久化身份为 agentId + profileId；旧 Claude profile 文件需要兼容读取。
- Codex 的具体路径、格式和可管理能力以官方资料和本机实际安装发现结果为准，不在通用层预设 Claude 字段。
- 目标配置存在时，转换是显式操作，不自动覆盖。

## 影响范围

预计会触及以下区域，实际以任务执行中的最小 diff 为准：

- src-tauri/src/agents/：adapter trait、registry、capability、discovery、converter
- src-tauri/src/commands/：Agent discovery/state、apply/convert IPC 命令
- src-tauri/src/models.rs：agent-neutral persisted models
- src-tauri/src/fs/：Agent path、backup、atomic write 的复用或扩展
- src/lib/：Agent 类型、schema、IPC 封装、转换预览模型
- src/store/：Agent 选择、项目状态、profile、转换状态
- src/components/：Agent selector、能力驱动 UI、转换预览/确认
- src/i18n/locales/{zh,en}.json：同步 UI 文案
- docs/design-docs/architecture.md：批准后的架构更新
- tests/ 与 src-tauri/tests/：适配器、转换、回滚和行为测试

## 关键决策

1. 身份分层：AgentDefinition 表示内置产品；AgentInstallation 表示 canonical config instance；AgentContext 表示 installation + 可选项目。
2. Capability 来自可调用端口，descriptor 由端口推导并声明 scopes、operations、availability、limitations。
3. 通用层只消费 ResourceRef/ResourceSnapshot，不包含 Claude/Codex 专属 schema。
4. profile 保持 `(agentId, profileId)` 复合身份，使用稳定 envelope + adapter-owned payload。
5. adapter 只 inspect/plan；共享 ExecutionEngine 唯一负责 digest 检查、备份、原子写、receipt 和补偿回滚。
6. conversion 是 source/target adapter 之间的 artifact route；source 永远不进入 write-set。
7. canonical installation 由 adapter 按有效 config home 判定；registry 只暴露去重结果，不保留 aliases。

## 进展

- [x] (2026-07-14) 评审多 Agent 产品规格和本 ExecPlan
- [x] (2026-07-14) 建立 agent-neutral model、registry、capability、内置 Claude/Codex discovery 和 IPC 基础；旧 profile migration 仍在后续 Claude adapter slice 完成
- [x] (2026-07-15) 多 Agent v1 架构获用户 LGTM；当前 foundation 归类为 v0 兼容性实验，恢复实施但不继续扩展 v0 conversion apply
- [x] (2026-07-15) 建立 v1 typed identity、AgentContext、capability ports、ResourceSnapshot、MutationPlan/Receipt 与结构化错误契约
  - [x] (2026-07-15) AgentId、InstallationId、ProfileId、PlanId、ReceiptId 已改为透明 newtype；AgentDefinition/AgentContext serde contract 已通过新增测试，v0 JSON 形状不变
  - [x] (2026-07-15) 完成 Agent-neutral ResourceRef/Snapshot、MutationPlan、OperationReceipt 与 AgentError 契约；只读资源不变量和部分失败 IPC contract 已通过测试
  - [x] (2026-07-15) AgentAdapter 已改为五类可调用 capability ports；registry 只从实际返回的 port 推导 scope、operation、availability、limitations descriptor，并同步生成 legacy metadata façade
  - [x] (2026-07-15) TypeScript 已增加 branded identities、context/resource/descriptor/plan view/receipt/error 严格 Zod schemas；Phase 1 Rust、schema、旧 profile/store 回归全部通过且无 v1 写盘路径
- [x] (2026-07-15) canonical installation identity 已移入 adapter discovery candidate；真实路径规范化覆盖默认 home、CODEX_HOME、尾斜杠与 macOS path alias，registry 仅保留 canonical installation，不持久化 evidence/aliases
- [x] (2026-07-15) AgentContext 已接入 IPC 与 Zustand；前端从 legacy activeAgentId 迁移到 v2 installation/project context，支持持久化恢复与失效 installation 回退，Tauri Agent 边界使用严格 Zod 解析
- [ ] (进行中) 将现有 Claude Code 行为包入 Claude adapter，保持回归兼容；当前已完成 Agent-scoped profile identity、Agent-specific profile storage 与 built-in discovery
- [ ] (待开始) 实现 Codex adapter 的 discovery、配置、Skills、Plugins、进程探测和终端启动
- [ ] (进行中) 实现 Claude Code → Codex 转换预览、冲突、备份和回滚；当前已完成 TOML preview contract、model 映射和 unsupported 字段报告，目标写入/回滚仍待实现
- [ ] (进行中) 接入 Agent-aware store、UI、i18n 和 IPC；当前已完成 Agent store、selector、双语文案、discovery IPC 与按 Agent profile 加载/保存
- [ ] (待开始) 完成单元、集成、行为测试及架构文档更新
- [ ] (待开始) 完成发布前 build 和人工验收

## 意外发现

- 当前 Claude Code 逻辑分布在 models.rs、commands/settings.rs、commands/skills.rs、commands/activate.rs、fs/paths.rs、多个 store 和 UI 组件中，不是单一模块。
- 本机同时存在 ~/.claude 和 ~/.codex 配置根；Codex 目录包含 config.toml、AGENTS.md、hooks.json 等文件。实现前必须逐项确认哪些文件属于可管理配置，避免误读 state、auth 或日志文件。
- 当前产品规格目录要求 HTML 规格；因此本计划同时提供 MD 活文档和 HTML 评审基线。
- 当前 `AgentAdapter` 只有 metadata/discover，capability 声明与 settings/skills/plugins/process/launch 的可调用实现没有类型关系；两个 capability set 相等不能证明能力对等。
- 当前 `ProfileFile` 增加 agentId 后仍持有 ClaudeSettings/ProfileLayers，Codex profile 会被迫伪装成 Claude payload。
- Codex 支持 `CODEX_HOME`，当前 `~/.codex` 固定路径与 `agentId + rootPath` lexical 去重无法表达多个配置实例；canonical identity 应由 adapter 按有效配置 home 判定。
- 多文件转换不能承诺原子事务；正确保证是写前全量备份、逐文件原子写、digest 并发检查和带 partial 状态的补偿式回滚。

## 决策日志

- 决策：暂停在 v0 foundation 上继续实现 Codex apply/rollback，重新评审多 Agent 核心抽象。
  理由：当前通用模型仍泄漏 Claude schema，capability 声明不可由实现验证，继续扩展会放大迁移成本。
  日期/作者：2026-07-14 / Codex

- 决策：以 AgentDefinition / AgentInstallation / AgentContext、capability ports、ResourceSnapshot、MutationPlan/Receipt 和共享 ExecutionEngine 作为 v1 核心。
  理由：把 Agent-specific 解析与通用数据安全执行分离，并让 parity 在 operation contract 层可验证。
  日期/作者：2026-07-15 / 用户批准，Codex 记录

- 决策：只保留 canonical installation，不保留 aliases。
  理由：用户明确要求发现结果去重后只保留 canonical installation。
  日期/作者：2026-07-14 / Codex

- 决策：Claude Code → Codex 保持源配置不动。
  理由：转换是生成目标配置的操作，不是跨 Agent 覆盖。
  日期/作者：2026-07-14 / Codex

- 决策：未来 Agent 只支持内置代码 adapter。
  理由：用户明确不支持用户配置式扩展。
  日期/作者：2026-07-14 / Codex

## 上下文和方向

当前 profile 模型以 ClaudeSettings、ProfileLayers 和 Claude-specific activation 语义为中心。实施时先抽出稳定的 Agent-neutral contract，再把 Claude 现有行为迁入 Claude adapter；不要直接把 Codex 分支塞进现有 Claude commands。

当前 Skill 管理通过 ~/.claude/skills 和项目 .claude/skills symlink 工作。Codex 的 Skill/Plugin 语义必须由 Codex adapter 实现，不能假设路径或格式与 Claude 相同。

## 工作计划

### Phase 1 — v1 contract（当前）

1. 用 newtype 定义 AgentId、InstallationId、ProfileId、PlanId、ReceiptId。
2. 定义 AgentDefinition、AgentInstallation、AgentContext、ResourceRef、ResourceSnapshot、MutationPlan、OperationReceipt、AgentError。
3. 定义 Settings/Skills/Plugins/Process/Launch capability ports；capability descriptor 从端口存在性和 port descriptor 推导。
4. 保持 v0 IPC 和旧 ProfileFile 可编译、可读取；v1 暂不写用户文件。

验证：先写失败测试，证明 typed IDs 不可混用、capability 不能脱离端口自报、schema 可 round-trip；再使 targeted Rust tests 和现有 suite 通过。

### Phase 2 — canonical discovery + AgentContext IPC

1. adapter 返回 InstallationCandidate 和内部 canonical key；registry 确定性合并。
2. Claude 按有效配置 home 识别 installation；Codex 同时支持默认 home 和 CODEX_HOME。
3. IPC 返回 canonical installations 和 capability descriptors；前端 schema 校验。
4. active state 从 agentId 扩展为 AgentContext，同时兼容旧 localStorage 选择。

验证：同 home 多来源只返回一个 installation；不同 CODEX_HOME 返回不同 installation；不持久化 aliases。

### Phase 3 — Claude vertical + ExecutionEngine

1. 用 v1 settings/skills/plugins/process/launch ports 包装现有 Claude 行为。
2. 建立 backend-owned plan store、digest preconditions、backup manifest、atomic mutation、receipt。
3. 先迁移一个 Claude settings apply 路径，验证单文件和多文件补偿行为。
4. 保持旧 IPC façade、旧 profile reader 和当前 UI 无回归。

验证：故障注入覆盖 target changed、backup failed、second write failed、compensation partial；旧 Claude 流程测试通过。

### Phase 4 — Codex parity vertical

1. 用 allowlist 枚举 Codex settings、instructions、skills、plugins 等可管理 resource，排除 auth/session/log/cache/database。
2. 实现 user/project settings inspect/edit/plan，并保持未知 TOML 字段。
3. 实现 Skills、Plugins、Process、Launch ports；terminal backend 执行 adapter 生成的 LaunchRecipe。
4. operation-level parity contract tests 比较 Claude/Codex 用户任务，不比较 enum set。

验证：AD_HOME fixtures + 真实 macOS 检查覆盖五类 P0 能力、作用域、权限错误和外部修改。

### Phase 5 — profile envelope + artifact conversion

1. 引入 AgentProfile envelope 和 payloadSchema；旧 Claude profile 通过 compatibility reader 映射。
2. Claude/Codex adapter 分别验证自己的 payload；前端通过集中 editor registry 选择编辑器。
3. 将现有 TOML preview 重构为 artifact route，逐项输出 exact/mapped/requires_input/unsupported/conflict/unchanged。
4. target plan 经 ExecutionEngine 备份/写入/receipt/rollback；source 只存在于 read-set。

验证：source 字节不变、target digest 变化使 plan 失效、已有目标冲突可见、rollback 不覆盖 apply 后用户新修改。

### Phase 6 — UI integration + release gate

1. UI 使用 AgentContext、capability descriptors、plan views 和结构化错误；移除散落 agentId 业务分支。
2. settings/skills/plugins/process/terminal 与 conversion 流程达到 Claude/Codex 对等。
3. 同步 architecture、product spec 和 as-built design；运行全量检查和 macOS 人工验收。
4. 填写结果回顾并归档 ExecPlan MD + 冻结 HTML。

## 验证和验收

最低命令集：

- pnpm typecheck
- pnpm lint
- pnpm test
- cargo test --manifest-path src-tauri/Cargo.toml
- pnpm build

核心行为验收：

- Claude Code 与 Codex 同时存在时，二者配置、profile、history 不串写。
- 同一 Agent 被多个 discovery 来源发现时只出现一个 canonical installation。
- Claude Code → Codex 不修改任何 Claude 文件。
- Codex 目标不存在时生成目标配置；目标存在时必须先显示 diff 和无法转换字段。
- 用户确认后才备份并写入 Codex；写入失败可恢复目标原状。
- Claude/Codex 的五类能力均可通过同一套 UI 操作，但由各自 adapter 实现。

## 幂等性和恢复

- Discovery、读取和预览是幂等的。
- apply 使用现有 atomic write；每个目标文件写入前生成带 agentId、project、timestamp 的 backup。
- conversion preview 不写盘，可重复执行。
- conversion apply 使用 operation ID 防止重复确认造成二次覆盖。
- 目标写入失败时按 backup manifest 恢复；源文件不参与恢复写入。
- 若 adapter 版本或配置格式变化，拒绝静默转换，返回 requires-confirmation。

## 接口和依赖

- 复用现有 fs::atomic::write_atomic、fs::paths、backup/history 机制。
- 优先使用现有 serde、serde_json、zod、Zustand 和 Tauri invoke，不新增数据库。
- 继续使用已获用户批准的 `toml` 依赖；不新增数据库或动态插件系统。
- Phase 1 结束时必须存在：typed IDs、AgentDefinition/Installation/Context、五类 capability ports、ResourceSnapshot、MutationPlan/Receipt、AgentError。
- IPC 只返回 plan view；apply 输入只接受 planId 和确认意图，不接受前端回传目标路径或 mutation content。

## 里程碑规划

- M1 v1 Contract：typed identity、context、ports、resource、plan/receipt/error。
- M2 Canonical Context：config-home identity、dedup、AgentContext IPC/store。
- M3 Safe Execution：Claude vertical + shared ExecutionEngine + fault injection。
- M4 Codex Parity：Codex 五类能力通过 operation-level contracts。
- M5 Conversion：profile envelope + artifact route + backup/rollback。
- M6 UI & Release：统一 UI、全量测试、build、手工验收和文档归档。

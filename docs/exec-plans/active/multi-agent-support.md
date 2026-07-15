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
- [x] (2026-07-15) 将现有 Claude Code settings、skills、plugins、process、launch 行为包入五类 v1 capability ports；端口只 inspect/plan、不写用户文件，旧 settings/skills/activate IPC 回归测试保持通过
  - [x] settings snapshot 使用 SHA-256 digest，user scope ResourceRef 不携带项目路径，project scope 才绑定 canonical project
  - [x] skills 的本地来源安装和 user/project symlink 启停可生成 MutationPlan；Git acquisition 暂保留旧流程并通过 degraded limitation 明示
  - [x] plugins 暴露现有列表与 project override 启停；AD 原本不管理插件安装，因此不声明 Install operation，并返回结构化 Unsupported
  - [x] process 输出通用 ProcessObservation，launch 只生成 LaunchRecipe，不直接启动进程
- [x] (2026-07-15) 建立 backend-owned PlanStore 与 settings preview IPC；IPC 仅返回 MutationPlanView，真实 mutation content/read-set 留在后端，执行侧 claim 只接受 PlanId
  - [x] plan 过期、未知 id、重复消费均返回结构化错误；claim 前同时重新校验 read-set 与 write-set digest，冲突后 plan 立即失效
  - [x] 新增 6 个 PlanStore 单元测试，并通过 36 个 Agent 定向回归测试与 cargo check；真正备份、写入和 receipt 留在下一 ExecutionEngine 切片
- [x] (2026-07-15) 实现共享 ExecutionEngine；adapter capability port 通过 allowlist resolver 将 ResourceRef 解析为后端物理目标，IPC apply 只接收 PlanId
  - [x] 所有目标在写入前完成 file backup 与 manifest 持久化；每个文件使用 atomic write，Skill symlink 使用临时 sibling + rename
  - [x] 第二次写失败时逆序补偿；补偿成功返回 compensated receipt，补偿失败返回 partial_failure receipt；receipt 持久化到 AD history
  - [x] 5 个 execution 测试覆盖 complete、backup failure、second write failure、compensation failure 和 symlink apply；43 个 Agent 测试、前端契约测试、typecheck、cargo check 通过
- [x] (2026-07-15) 基于当前 Codex 官方手册实现 Codex Settings Port：支持 CODEX_HOME/default home 下的 user `config.toml` 与 canonical project 下的 `.codex/config.toml`
  - [x] TOML 作为完整文本 snapshot/edit 进行语法验证，保留未知字段和表；preview 不写盘，apply 复用共享 ExecutionEngine
  - [x] allowlist 只接受 user-config/project-config，明确排除 auth.json、history、logs、sessions、cache/database 等认证与运行时状态
  - [x] 4 个 Codex 测试及 45 个 Agent 回归测试、typecheck 通过；user/project apply 互不串写
- [x] (2026-07-15) 实现 Codex Skills/Plugins Ports
  - [x] Skills 按官方位置扫描 `$HOME/.agents/skills` 与项目 `.agents/skills`，支持本地目录 symlink 安装；enable/disable 通过用户 `config.toml` 的 `[[skills.config]]` 规划并由 ExecutionEngine 应用
  - [x] Plugins 从用户 `config.toml` 的 `[plugins."<id>"]` 列表与规划 enabled 状态，完整保留其他 TOML 字段
  - [x] Plugin install 涉及 marketplace snapshot、cache 和可能的工作区授权，当前 port 标记 degraded 并返回结构化 Unsupported，不模拟不完整安装
  - [x] 6 个 Codex 测试及 47 个 Agent 回归测试、cargo check 通过
- [x] (2026-07-15) 通用化进程探测与终端启动，并完成 Codex runtime ports
  - [x] ProcessPort 由 adapter 返回 executable match spec，共享 runtime service 统一扫描并输出 ProcessObservation；Claude 与 Codex 不再各自维护扫描逻辑
  - [x] Codex 提供 `codex`/`codex-cli` 进程规格和 adapter-owned LaunchRecipe；终端 IPC 改为 AgentContext，不再接收 projectPath/claudeBin 等 Claude 专属参数
  - [x] Ghostty、cmux、Terminal.app 和 custom backend 统一消费 program/args/env/cwd，逐项 shell quote；旧 Claude binary UI 设置已移除并从 localStorage hydration 过滤
  - [x] 49 个 Agent 测试、15 个终端测试、27 个前端测试、typecheck、cargo check 和变更文件 ESLint 通过；全仓 lint 仅剩既有 ProjectSidebar/SkillSources 错误
- [x] (2026-07-15) 实现 Codex adapter 的 discovery、配置、Skills、Plugins、进程探测和终端启动
- [x] (2026-07-15) 完成 Claude/Codex operation-level parity contract 与 Checkpoint D
  - [x] 同一集成 contract 分别执行两种 adapter 的 settings edit、Skill install/disable、Plugin toggle、process detect 和 launch recipe；实际 mutation 均经过 PlanStore 与 ExecutionEngine
  - [x] 两种 Plugin install 均通过 degraded descriptor 和结构化 Unsupported 表达限制，不用 capability enum 数量伪造对等
  - [x] Codex fixture 同时放置 auth、history、session、log 文件，并验证 settings/skills/plugins 的非空 snapshots 从不包含这些运行时或敏感文件
  - [x] parity 测试发现并修复 macOS `/var` 与 `/private/var` alias 导致 Claude Skill symlink 被误判为非 AD-managed 的问题
  - [x] 移除六个 capability port 对尚未实现的用户 `Rollback` operation 的超前声明；Task 16 提供带 digest 保护的真实 rollback API 后再恢复
- [x] (2026-07-15) 引入稳定 AgentProfile envelope 与 adapter-owned payload schema
  - [x] profile 以 `(agentId, profileId)` 为复合身份，Claude 与 Codex 可使用同名 profile 且分别持久化到 Agent 目录
  - [x] Claude payload 保留 layers/settings，Codex payload 使用独立 configToml；两种 adapter 在后端分别校验 payload schema，Codex 同时校验 TOML 语法
  - [x] compatibility reader 将旧 Claude ProfileFile 映射为 envelope，读取过程不改写源文件；首次保存写入 canonical 目录，并保留时间戳冲突与 APFS case-collision 保护
  - [x] 新增独立 envelope CRUD 和前端严格 Zod schema；旧 ProfileFile façade 仅继续服务 Claude，拒绝把 Codex 伪装成 Claude payload
  - [x] 197 个 Rust lib 测试（4 ignored）、双 Agent parity 测试、30 个前端测试、typecheck、cargo check 和变更文件 ESLint 通过
- [x] (2026-07-15) 将 Claude Code → Codex 配置转换重构为 artifact-level route
  - [x] 内置 ConversionRoute 通过 Claude/Codex Settings Port 获取 source/target snapshots；按字段输出 exact、mapped、requires_input、unsupported、conflict、unchanged
  - [x] 已启用 Skills 和已安装 Plugins 同样进入 artifact inventory；未确认的本地来源、marketplace 身份和授权显式标记 requires_input/conflict，不静默安装或遗漏
  - [x] Codex 目标采用 merge/skip/conflict：未知目标 TOML 和冲突值原样保留，只为无冲突 artifact 生成后端 MutationPlan
  - [x] Claude source snapshots 仅以 ReadOnly precondition 进入 read-set，所有 mutation 必须属于 Codex installation；伪造 source mutation 会被 route invariant 拒绝
  - [x] source/target fixtures 验证预览不写盘、六种 disposition、未知字段保留和 target-only write-set；199 个 Rust lib 测试（4 ignored）、parity、30 个前端测试、typecheck 和 cargo check 通过
- [x] (2026-07-15) 完成 conversion 显式确认、共享执行和 digest-protected rollback，并通过 Checkpoint E
  - [x] PlanStore 区分普通 plan 与 confirmation-required conversion plan；通用 apply 不能绕过确认，未确认失败不会消费 plan
  - [x] receipt 持久化 manifest digest、post-apply resource kind 和 digest；rollback 重新通过 adapter allowlist 解析目标，不信任 manifest 中的物理 target path
  - [x] rollback 将原状态构造成新的 MutationPlan 交给 ExecutionEngine，因此同样具备写前备份、原子写和失败补偿；旧 receipt 缺少新状态时明确拒绝
  - [x] source/target integration 覆盖未确认拒绝、确认 apply、source 字节不变、目标字节级恢复和外部修改拒绝；200 个 Rust lib 测试（4 ignored）、parity、31 个前端测试、typecheck、cargo check 和变更文件 ESLint 通过
  - [x] Settings/Skills/Plugins 的 Claude/Codex descriptor 重新声明真实可调用 Rollback operation
- [x] (2026-07-15) 接入 Agent-aware store、UI、i18n 和 IPC
  - [x] Agent store 同时维护 canonical installation context 与 adapter-derived capability descriptors；项目 Settings、Skills、Plugins、进程探测和终端入口按 operation/scope/availability 驱动
  - [x] Settings 编辑器支持存在和待创建的 JSON/TOML target；所有编辑与资源启停先预览 MutationPlan，经确认后 apply，并提供结构化错误与 receipt rollback
  - [x] Profile UI 以 `(agentId, profileId)` envelope 和集中 adapter payload editor 工作；Claude legacy template/import/shortcut 入口仅由内置 profile policy 暴露
  - [x] Claude Code → Codex 转换 UI 固定使用内置 route，展示逐 artifact disposition 与原因，明确保持 source 不变，并提供目标备份和回滚
  - [x] History UI 展示 installation-scoped operation receipts 并支持确认回滚；Claude 旧激活历史作为受 policy 控制的兼容区保留
  - [x] 双语文案、严格前端 schema、Tauri IPC 和 capability limitation 展示已同步；48 个前端测试、60 个 Agent 定向 Rust 测试、typecheck、ESLint 与前端 production build 通过
- [ ] (待开始) 完成单元、集成、行为测试及架构文档更新
- [ ] (待开始) 完成发布前 build 和人工验收

## 意外发现

- 当前 Claude Code 逻辑分布在 models.rs、commands/settings.rs、commands/skills.rs、commands/activate.rs、fs/paths.rs、多个 store 和 UI 组件中，不是单一模块。
- 本机同时存在 ~/.claude 和 ~/.codex 配置根；Codex 目录包含 config.toml、AGENTS.md、hooks.json 等文件。实现前必须逐项确认哪些文件属于可管理配置，避免误读 state、auth 或日志文件。
- 当前产品规格目录要求 HTML 规格；因此本计划同时提供 MD 活文档和 HTML 评审基线。
- 当前 `AgentAdapter` 只有 metadata/discover，capability 声明与 settings/skills/plugins/process/launch 的可调用实现没有类型关系；两个 capability set 相等不能证明能力对等。
- 当前 `ProfileFile` 增加 agentId 后仍持有 ClaudeSettings/ProfileLayers，Codex profile 会被迫伪装成 Claude payload。
- Codex 支持 `CODEX_HOME`，当前 `~/.codex` 固定路径与 `agentId + rootPath` lexical 去重无法表达多个配置实例；canonical identity 应由 adapter 按有效配置 home 判定。
- 当前 Codex 官方配置层级为 CLI override → trusted project `.codex/config.toml` → profile file → user `config.toml` → system config；项目配置不能覆盖认证/provider/telemetry 等受限键，但 AD 仍应保留这些未知文本并由 Codex 自身决定是否生效。
- 当前 Codex 官方 Skill authoring/discovery 位置为 `$HOME/.agents/skills` 和从 CWD 到 repo root 的 `.agents/skills`；`CODEX_HOME/skills` 不作为 AD 新建用户 Skill 的目标。
- Codex Plugin 的 marketplace 可位于 repo/user `.agents/plugins/marketplace.json`，安装副本进入 `CODEX_HOME/plugins/cache/...`，启停状态进入用户 `config.toml`；单纯复制插件目录不能形成对等安装。
- 多文件转换不能承诺原子事务；正确保证是写前全量备份、逐文件原子写、digest 并发检查和带 partial 状态的补偿式回滚。
- 共享 parity contract 首次执行 Claude Skill disable 时暴露 macOS 临时目录的 `/var` → `/private/var` canonical alias；symlink target 与 managed root 必须按 canonical identity 比较，同时兼容 dangling target。

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

- 决策：Claude Skills Port 的 Git acquisition 在共享执行引擎接管目录级副作用前继续使用旧流程，并将能力标记为 degraded。
  理由：当前 MutationPlan 只表达受 allowlist 约束的资源变更，Git 网络拉取与目录落盘不能被安全地伪装成普通原子文件写入。
  日期/作者：2026-07-15 / Codex

- 决策：Claude Plugins Port 不声明 Install operation。
  理由：现有 AD 只读取插件并管理项目级 enabledPlugins override，不具备插件安装行为；capability descriptor 必须来自真实可调用能力。
  日期/作者：2026-07-15 / Codex

- 决策：物理 resource path 由 capability port 的内置 allowlist resolver 提供，MutationPlan、PlanView 和 apply IPC 均不携带可由调用方指定的路径。
  理由：ExecutionEngine 需要访问真实目标完成 digest、backup 和 write，但路径解释仍必须属于 Agent-specific port，避免通用层拼接 Claude/Codex 路径或信任前端输入。
  日期/作者：2026-07-15 / Codex

- 决策：在可调用 receipt rollback API 完成前，Settings/Skills/Plugins descriptor 不声明 Rollback operation。
  理由：ExecutionEngine 当前只有 apply 失败时的内部补偿；把它表述为用户可调用 rollback 会破坏“capability 来自真实端口”的核心约束。Task 16 实现 digest-protected rollback 后再恢复声明。
  日期/作者：2026-07-15 / Codex

- 决策：新增 AgentProfile envelope CRUD，并暂时保留只支持 Claude 的旧 ProfileFile façade。
  理由：旧 UI 在 Task 17 迁移前仍需读取 Claude profile；让 Codex 继续复用 ClaudeSettings/ProfileLayers 会污染 adapter 边界，因此旧 façade 对 Codex 返回空列表并拒绝写入。
  日期/作者：2026-07-15 / Codex

- 决策：conversion plan 在 PlanStore 中标记为 confirmation-required，只有专用 confirmed claim 路径可以消费。
  理由：仅依赖前端确认对话框无法阻止调用方拿 planId 走通用 apply；确认意图必须由后端强制执行，且未确认尝试不能使 plan 失效。
  日期/作者：2026-07-15 / Codex

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

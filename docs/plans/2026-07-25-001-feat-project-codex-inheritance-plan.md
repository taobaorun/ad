---
title: 项目 Codex 配置继承开关
type: feat
date: 2026-07-25
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
---

# 项目 Codex 配置继承开关

## Goal Capsule

让每个 AD 项目可以独立决定派生 `CODEX_HOME` 是否继承 Base `~/.codex/config.toml`。开关默认开启；切换只改变“期望策略”，必须经过 Preview → Apply 才改变派生运行时。配置继承与 Base 认证共享保持解耦，Codex 原生 `<project>/.codex/config.toml` 始终不由 AD 修改并继续拥有更高优先级。

成功时，用户可以在项目详情中清楚看到期望策略、已应用策略和待刷新状态；关闭继承后，派生配置只包含 AD 必需字段与 Project Plugin overlay，不再携带 Base 的模型、MCP、agents、features、marketplaces 或 enabled plugins；打开后恢复现有结构化合成与插件镜像行为。

---

## Product Contract

### Actors

- **A1 项目使用者**：为不同项目选择隔离或继承的 Codex 配置，并通过 Preview → Apply 安全切换。
- **A2 AD 运行时管理器**：生成、验证、应用和回滚项目派生 `CODEX_HOME`。
- **A3 Codex CLI**：从派生 `CODEX_HOME` 与项目原生 `.codex/config.toml` 读取最终有效配置。

### Requirements

- **R1** 每个项目持久化 `inheritBaseConfig` 期望值，旧项目和缺省调用默认 `true`。
- **R2** 项目详情提供本地化开关和说明；切换后明确显示“需要 Preview → Apply”，不得把期望值误报为已生效。
- **R3** Preview 接收期望策略并生成标准 `MutationPlan`；Preview 不得修改派生配置、运行时清单或已应用状态。
- **R4** 开启继承时继续以 Base 配置为起点做结构化合成，并镜像 Base 已启用插件的 marketplace/package。
- **R5** 关闭继承时以空 TOML 为起点，仅写入 AD 必需字段与 Project Plugin overlay；不继承 Base 的 model、MCP、agents、features、marketplaces、plugins 或 profiles。
- **R6** Base 认证共享独立于配置继承；两种模式都维持现有 `auth.json`/Keychain 检测、复用和阻塞语义。
- **R7** 持久化已应用策略与明确的 Project Plugin overlay。on→off 后既有 Base 镜像作为不活跃缓存保留，但生成配置不得再引用它们，也不得继续新增 Base 镜像。
- **R8** freshness 与启动门禁区分“期望策略”和“已应用策略”：策略不一致、生成配置或清单漂移时阻止启动；继承关闭时 Base 配置普通变化不应使运行时过期。
- **R9** Apply 保持现有 read-set、digest、backup、compensation 和 rollback 保护；失败后不得留下半应用策略。
- **R10** `<project>/.codex/config.toml` 在 Preview、Apply、切换和回滚中保持字节不变。
- **R11** 关闭继承时不允许继续使用只存在于 Base 配置的 profile；UI 清除/禁用该选择，后端拒绝矛盾请求。
- **R12** 首次准备、无可转换 artifact 的 bootstrap、Project Plugin 安装与后续重装都遵循同一继承策略和 Project overlay manifest。
- **R13** 插件来源或包内容变化沿用现有语义：在下一次 Preview 发现并收敛，不在 Launch 阶段自动刷新。
- **R14** Preview 与 Apply 都显示应用内 progress/loading；执行期间禁用重复提交和会破坏当前操作的关闭动作，不能只依赖系统忙碌光标。
- **R15** 项目运行时卡片必须区分检查中、未准备、已就绪、策略待应用、磁盘漂移、认证阻塞和检查失败，并提供可访问的主操作与恢复路径。

### Key Flows

- **F1 首次隔离准备**：用户关闭继承 → 状态显示待应用 → Preview 展示仅含运行时清单、隔离配置和独立认证绑定的变更 → Apply → 状态显示已隔离且可启动。
- **F2 从继承切到隔离**：用户关闭继承 → Preview 读取已应用 Project overlay → 计划停止引用 Base 配置与镜像 → Apply 原子提交策略、配置和清单 → Launch 不再受 Base 配置变化影响。
- **F3 从隔离恢复继承**：用户打开继承 → Preview 合成当前 Base 配置并准备 Base 已启用插件 → Apply → 状态显示继承已应用。
- **F4 安装 Project Plugin**：在任一模式 Preview/Apply 安装 Project Plugin；后续切换继承模式仍保留它及其来源信息。
- **F5 冲突与恢复**：Preview 后 Base/目标/清单发生变化或 Apply 中途失败 → Apply 拒绝或回滚 → 已应用策略和有效运行时保持原状。

### Acceptance Examples

- **AE1** 给定旧 `projects.json` 没有新字段，加载后开关为开启，行为与当前版本一致。
- **AE2** 给定 Base 配置含 model、MCP、features 和 enabled plugin，关闭继承并 Apply 后，派生 `config.toml` 不含这些 Base 值，但仍含 `cli_auth_credentials_store = "file"` 和已有 Project Plugin。
- **AE3** 给定关闭继承且 Base `config.toml` 随后变化，运行时保持 fresh；若派生 `config.toml` 或运行时清单被外部修改，运行时变 stale 并阻止启动。
- **AE4** 给定共享认证为文件模式，关闭继承后派生 `auth.json` 仍按现有规则复用；给定 Keychain 模式，仍返回既有阻塞提示。
- **AE5** 给定继承模式曾镜像 Base package，切到隔离后派生配置不再引用该 package，后续 Preview 不再复制它；已有缓存保留为不活跃文件，Project package 不被破坏。
- **AE6** 给定用户只切换开关或只完成 Preview，终端启动仍被“策略待应用”门禁阻止，不能使用未应用的新策略。
- **AE7** 给定 `<project>/.codex/config.toml` 存在，完整 on→off→on 循环后其 digest 不变。
- **AE8** 给定用户点击 Apply，应用内立即显示与 Preview 一致的 progress；执行期间不能重复提交，成功、冲突、补偿成功和回滚失败都有明确结果与下一步。

### Product Key Decisions

- **PKD1 — 项目级、默认开启。** `(session-settled: user-approved — chosen over 全局开关或默认隔离: 保持现有项目行为并允许逐项目控制)` Governs R1, R2, R8.
- **PKD2 — 配置继承与认证共享解耦。** `(session-settled: user-approved — chosen over 关闭继承同时断开 Base auth: CODEX_HOME 隔离配置不等于要求重复登录)` Governs R5, R6.
- **PKD3 — 切换必须 Preview → Apply。** `(session-settled: user-approved — chosen over 点击开关立即重写运行时: 延续 AD 可审计、可回滚的转换模型)` Governs R2, R3, R8, R9.
- **PKD4 — 关闭继承即排除全部 Base 配置内容。** `(session-settled: user-approved — chosen over 只排除部分键或继续继承 enabled plugins: 隔离语义需要清晰且可预测)` Governs R4, R5, R7.
- **PKD5 — 原生项目配置继续由 Codex 管理。** `(session-settled: user-approved — chosen over 把项目 .codex/config.toml 合入 AD 生成文件: 保持 Codex 原生优先级和所有权边界)` Governs R10.

### Scope Boundaries

- **本次包含**：项目期望设置、已应用运行时策略、Project overlay manifest、结构化合成、镜像启停、策略感知 freshness/launch、UI/i18n、迁移与测试。
- **后续再做**：独立的 `shareBaseAuth` 用户开关；不依赖 Base 的 Project Profile 管理。
- **明确不做**：直接写 `~/.codex`；在 Launch 时自动 Apply；新建聊天/MCP 控制面；删除无法证明来源的旧缓存。
- **缓存策略**：本次不物理清理继承插件缓存；“关闭继承”的保证是有效配置不引用 Base 内容且不再新增镜像。缓存清理可在有独立价值和可靠所有权数据后另行设计。

---

## Planning Contract

### Key Technical Decisions

- **KTD1 — 期望策略存在 Project，已应用策略存在派生运行时清单。** Project 的 `inheritBaseConfig` 表示用户期望；版本化清单（`<derived CODEX_HOME>/.ad/runtime-manifest.json`）只保存已应用策略、已应用 profile、明确的 Project overlay 与必要 digest，不保存 auth 内容或 credential-bearing URL。状态比较两者，不以 UI 开关替代磁盘事实。 Governs R1, R2, R3, R7, R8, R12.
- **KTD2 — 运行时清单是事务 mutation。** 通过 `resolve_project_resource` 增加 `runtime-manifest` 资源并进入 `MutationPlan`，让现有 ExecutionEngine 自动提供 precondition、backup、compensation 和 rollback。 Governs R3, R7, R9.
- **KTD3 — 配置合成复用唯一 seam。** 开启时调用 `synthesize_project_codex_config(Some(base), ...)`；关闭时调用 `synthesize_project_codex_config(None, ...)`。Project overlay 从清单读取，不再从已合并的派生配置反推。 Governs R4, R5, R7, R12.
- **KTD4 — 关闭继承不删除缓存。** on→off 只重写 config/manifest 并停止准备 Base Plugin mutations；旧 marketplace/package 文件不再被配置引用并保留为不活跃缓存。这样无需从历史合并结果猜测资源所有权，也不会把可编辑 manifest 变成删除授权来源。 Governs R7, R9.
- **KTD5 — freshness 策略感知。** 首次成功 Apply 产生 manifest 后，生成配置 digest 与清单 digest 始终校验；仅在“已应用继承=true”时比较 Base 配置 digest。旧 runtime 尚无 manifest 时是唯一兼容例外：按 applied-on legacy 状态验证既有 config/Base digest，并在成功 Apply 前不因 manifest 缺失判 stale。期望策略与已应用策略不一致单独产生 `needs_refresh`。插件 package 快照变化仍在下一次 Preview 发现，UI 将状态描述为“已应用”而非“与 Base 实时同步”。 Governs R8, R13.
- **KTD6 — profile 随应用策略事务化。** 清单保存 applied profile；关闭继承时 Preview 要求 profile 为空并在 Apply 后清除，后端拒绝 `inheritBaseConfig=false` 且 `profileId` 非空的请求。 Governs R5, R8, R11.
- **KTD7 — auth 依赖保持独立 read precondition。** 无论继承策略如何都执行 `SharedAuthBinding::detect`；需要读取 Base credential-store 时仍把 Base config digest 加入 Preview read-set，但该依赖不进入 off 模式的运行时 freshness。 Governs R6, R9.
- **KTD8 — 兼容迁移默认 true，绝不猜测启用中 Plugin 的所有权。** Rust/TypeScript 输入缺省均解释为 `true`；旧 runtime 没有清单时视为 applied-on。首次继续 on 的 Apply 只把本次 Preview 明确选择的 Project artifacts 写入 overlay；切 off 时，planner 必须从当前 conversion selection 或可验证的 AD operation record 重建 Project overlay。若启用中同名条目的历史所有权仍不可证明，Preview 返回 typed migration conflict，要求用户明确重新选择 Project Plugin 后再 Apply，不得自动把条目标成 Base 或 Project。旧配置中 `enabled = false` 的无效引用不承载项目行为，迁移时连同仅由它引用的 marketplace 收敛清理；旧缓存始终保留。 Governs R1, R7, R9.
- **KTD9 — Preview 使用纯内存 runtime descriptor。** derived identity/home 可由 Project 和 Base installation 确定性计算；Preview 直接把 descriptor 传给 adapter/resource discovery，不先创建目录或写 registry。Apply 写入 config/manifest 后，status/launch 可从 descriptor 与 manifest 同步重建；registry state 只作可恢复缓存。 Governs R3, R8, R9, R12.
- **KTD10 — conversion route 单点完成 config 与 manifest。** bootstrap 和单个 Plugin planner 只返回 config/overlay 增量；route 合并全部增量后恰好追加一个最终 config mutation 和一个 manifest mutation，避免多个 Plugin 子计划竞争同一资源。 Governs R3, R7, R9, R12.

### System-Wide Impact

- **数据流**：Project desired setting → conversion options → 内存 runtime descriptor → Preview planner → route finalization 的 config/manifest/auth/cache mutations → Apply → status/terminal 从 manifest 重建 → registry cache refresh。
- **错误传播**：策略矛盾、外部 digest 漂移、Base 同名 marketplace 冲突和认证阻塞继续通过现有 typed error/MutationPlan 失败路径返回；UI 使用现有 conversion error 映射。
- **状态生命周期**：Preview 不创建 runtime home、不写 registry、不持久化 `profile_id` 或 applied policy；Apply 后 status/launch 直接读取已写清单，registry refresh 失败只影响缓存且不得改变成功结果。rollback 恢复清单后同样可从磁盘同步重建。
- **API 表面**：Project schema/setter、conversion options 和 runtime status 新增字段；Tauri command、Zod strict schema、fixtures 与 i18n 同步。
- **可观测性**：状态暴露 desired/applied inheritance 和 needs refresh；日志只记录策略与资源计数，不输出配置内容或认证信息。
- **并发绑定**：Preview 把 Project desired policy 与 `projects.json` digest 纳入 read precondition；开关在 Preview 后再次变化时，旧 plan 的 Apply 必须冲突并要求重新 Preview。

### Assumptions

- Base Plugin 源或包内容变化在下一次 Preview 检测，不扩展 Launch 为扫描器；这是现有生命周期的延续。
- 关闭继承时 Base profile 不可用；本次不引入独立 Project Profile，因此 UI 清空/禁用、后端拒绝矛盾输入。
- 旧缓存一律保留为不活跃文件，不做物理删除或所有权猜测。
- 不新增依赖；TOML、serde、Zod、ExecutionEngine 与现有测试工具足以实现。

### Risks and Mitigations

- **来源误判导致 Project Plugin 丢失**：不从相等快照推断同名条目所有权；有歧义时 Preview fail closed，要求显式选择 Project artifact；不物理删除缓存。
- **Preview 意外改变 applied 状态**：移除 Preview 对 runtime `profile_id` 的直接持久化；测试 Preview 前后状态文件字节一致。
- **旧运行时迁移不完整**：默认 applied-on；只有本次明确选择或可靠 operation record 能建立 Project overlay；歧义通过 typed conflict 暴露。
- **off 模式仍被 Base 变化阻塞**：区分 Apply read-set 与长期 freshness；状态矩阵测试覆盖。
- **脏工作树覆盖既有修复**：只做增量 patch，实施前后检查 diff，提交时按本任务文件/代码块精确暂存。
- **可编辑 manifest 消耗资源或复制敏感值**：严格 schema/version、`deny_unknown_fields`、固定字节/条目/字符串上限；禁止 auth 数据与 URL userinfo；未知版本或损坏内容 fail closed。

---

## Implementation Units

`docs/exec-plans/active/codex-config-inheritance-toggle.{md,html}` 已在编码前建立并由本次 LFG 授权确认；各单元执行时持续更新 MD Progress。U6 只补结果并归档，不负责事后创建 ExecPlan。

### U1 — 项目期望设置与类型化 IPC

- **Goal**：持久化默认开启的 per-project 期望策略，并让前后端读取/更新一致。
- **Requirements**：R1, R2.
- **Files**：
  - `src-tauri/src/models.rs`
  - `src-tauri/src/commands/projects.rs`
  - `src-tauri/src/lib.rs`
  - `src/lib/projectTypes.ts`
  - `src/lib/tauri.ts`
  - `src/store/projects.ts`
  - 相关 Rust/TypeScript 单元测试
- **Approach**：
  1. Rust Project 字段使用 `#[serde(default = "default_true")]`；setter 复用现有原子项目存储路径。
  2. TypeScript strict schema 和 store action 同步；setter 成功后 reload，失败保持原值。
  3. 先写旧 JSON 缺字段与 setter round-trip 测试。
- **Test scenarios**：旧数据默认 true；新项目 true；切换持久化；命令失败不产生 UI 假状态。
- **Verification**：`pnpm typecheck`；相关前端测试；Rust project command/model 测试。

### U2 — 版本化运行时清单与资源解析

- **Goal**：建立 applied policy、profile 与明确 Project overlay 的事务真源。
- **Requirements**：R3, R7, R8, R9, R12.
- **Depends on**：U1.
- **Files**：
  - `src-tauri/src/agents/project_codex_runtime.rs`
  - `src-tauri/src/agents/codex_ports.rs`
  - `src-tauri/src/agents/codex_plugins.rs`
  - `src-tauri/src/agents/execution_fs.rs`
  - `src-tauri/tests/project_codex_runtime.rs`
- **Approach**：
  1. 定义严格版本化、bounded、`deny_unknown_fields` 的 manifest 类型及稳定编码；未知版本、超限和损坏内容返回 typed error。
  2. 增加 `runtime-manifest` 资源解析与纯内存 runtime descriptor；复用既有 canonical containment/no-follow 路径保护。
  3. runtime state 缓存 manifest digest、applied inherit 与 applied profile；status/launch 以磁盘 manifest 为同步真源，旧 state 缺字段默认 applied-on。
  4. freshness 在非 legacy 状态验证 config/manifest，按 applied policy 决定是否验证 Base digest。
- **Test scenarios**：legacy deserialize；首次 Preview 零写盘；manifest path/size/version；off 忽略 Base drift；manifest/generated drift 均 stale；registry refresh 失败可重建；rollback 后一致。
- **Verification**：相关 Rust 集成测试。

### U3 — 策略化配置合成、插件镜像与迁移

- **Goal**：让 bootstrap 与 Project Plugin 安装在两种策略下都可收敛且不丢 Project overlay。
- **Requirements**：R3-R7, R9, R10, R12, R13.
- **Depends on**：U2.
- **Files**：
  - `src-tauri/src/agents/project_codex_config.rs`
  - `src-tauri/src/agents/codex_plugins.rs`
  - `src-tauri/src/agents/conversion_route.rs`
  - `src-tauri/src/commands/agents.rs`
  - `src-tauri/tests/project_codex_config.rs`
  - `src-tauri/tests/project_codex_plugin_install.rs`
  - `src-tauri/tests/project_plugin_conversion_route.rs`
- **Approach**：
  1. 把 `inheritBaseConfig` 贯穿 conversion options、bootstrap 与 install planner，并把 desired policy/`projects.json` digest 加入 plan precondition。
  2. Project overlay 优先来自 manifest；legacy runtime 只接受显式 artifacts 或可靠 operation record，歧义 fail closed。
  3. on 用 `Some(base)` 和 inherited plugin mutations；off 用 `None` 且跳过继承准备。
  4. bootstrap/install planner 返回增量，conversion route 合并后统一生成唯一 config/manifest mutations；off 不删除既有缓存。
  5. 盘点所有 derived config 写入口：Plugin install/reinstall/enable/disable 必须走同一 config+manifest finalizer；通用 Settings edit 不允许编辑 AD 生成的 derived config，只保留原生 project config 编辑。
  6. auth 绑定和 credential-store read precondition 始终独立执行；manifest 不保存 auth、token 或含 userinfo 的 URL。
  7. Preview 不保存 runtime state/profile；Apply 后 status/launch 从 manifest 重建，registry refresh 仅更新缓存。
- **Test scenarios**：纯 Base 字段排除；Project overlay 保留；on→off→on；legacy 同名歧义；多 Plugin 仅一个 manifest mutation；enable/disable/reinstall 同步；Base 插件移除/改变；重复 Apply 幂等；desired policy/外部编辑冲突；失败 rollback；原生 project config digest 不变；两种 auth 模式。
- **Verification**：三个 Rust 集成测试目标与全量 `cargo test`。

### U4 — 状态、终端门禁与 Profile 边界

- **Goal**：只允许与期望策略一致且磁盘未漂移的运行时启动。
- **Requirements**：R2, R6, R8, R11, R13.
- **Depends on**：U2, U3.
- **Files**：
  - `src-tauri/src/agents/project_codex_runtime.rs`
  - `src-tauri/src/agents/codex_runtime.rs`
  - `src-tauri/src/commands/terminal.rs`
  - `src-tauri/src/commands/agents.rs`
  - `src-tauri/tests/project_codex_runtime.rs`
- **Approach**：
  1. status 同时返回 desired/applied inherit、fresh 与 `needs_refresh`，并从 manifest 同步读取 applied 状态。
  2. terminal launch context 比较 Project desired 与 manifest applied，并复用 freshness gate。
  3. off 模式清空 applied profile；API 拒绝 off+profile。
- **Test scenarios**：仅 toggle/Preview 不能启动；Apply 后启动；off 不受 Base drift；generated/manifest drift 阻止；profile 矛盾拒绝。
- **Verification**：Rust status/launch 测试。

### U5 — 项目详情交互与转换反馈

- **Goal**：提供清晰、连续、可访问的设置体验，并在真实 Apply 期间显示应用内 loading。
- **Requirements**：R2, R3, R8, R11, R12, R14, R15.
- **Depends on**：U1, U4.
- **Files**：
  - `src/components/ProjectDetail.tsx`
  - `src/components/AgentConversionDialog.tsx`
  - `src/components/AgentConversionProgress.tsx`
  - `src/lib/agentTypes.ts`
  - `src/i18n/locales/zh.json`
  - `src/i18n/locales/en.json`
  - `tests/components/ProjectDetail.test.tsx`
  - `tests/components/AgentConversionDialog.test.tsx`
  - `tests/lib/agentTypes.test.ts`
- **Approach**：
  1. Project runtime 卡片增加开关、当前/待应用标签与解释文案。
  2. 待应用卡片提供 Preview 主操作；打开 conversion dialog 时锁定/预选当前 Project scope 并带入 desired policy。取消保留待应用状态并回焦触发控件；Apply 成功刷新卡片并提供 Launch 出口。
  3. conversion dialog 从 active Project 取得 desired policy；off 时清空并禁用 profile，并解释原因。
  4. Apply 复用 Preview 的应用内 progress/loading 状态；执行时禁用重复提交/破坏性关闭，资源变化引导 Re-preview，失败保留输入并给出 Retry/Close 或 rollback 结果。
  5. 使用键盘可聚焦的 switch/checkbox，暴露可访问名称、checked、described-by、focus 与适度 live status；窄窗口纵向堆叠且主操作不截断。
  6. strict Zod schema、fixtures、zh/en 同步。
- **状态矩阵**：

  | 状态 | 开关 | 主操作 | 反馈 / 恢复 |
  |---|---|---|---|
  | checking | disabled | 无 | 应用内 loading |
  | not prepared | enabled | Preview | 说明将首次准备 |
  | applied + ready | enabled | Launch | 显示 applied 策略 |
  | desired ≠ applied | enabled | Preview | 显示待应用 |
  | runtime stale | enabled | Re-preview | 说明磁盘或输入已变化 |
  | auth blocked | enabled | 打开认证指引 | 保持配置策略 |
  | inspection error | enabled | Retry | 不伪装成 not prepared |

- **Test scenarios**：默认开启；完整状态矩阵；切换→预选 Preview→取消/Apply→Launch 闭环；Preview/Apply 参数；Apply loading/锁定；expired plan Re-preview；补偿/rollback 结果；失败后不误报；off profile；键盘/读屏；中英文窄窗口。
- **Verification**：相关 Vitest/RTL 测试；`pnpm typecheck`。

### U6 — 文档、全量验证与本地安装

- **Goal**：同步架构契约，证明兼容性并交付可运行的 macOS 应用。
- **Requirements**：R1-R15.
- **Depends on**：U1-U5.
- **Files**：
  - `docs/design-docs/architecture.md`
  - `docs/design-docs/codex-plugin-conversion.md`
  - `docs/exec-plans/active/codex-config-inheritance-toggle.md`
  - `docs/exec-plans/active/codex-config-inheritance-toggle.html`
- **Approach**：
  1. 记录 desired/applied/manifest/auth/project-native 四层边界与迁移策略。
  2. 跑前端、Rust、生产构建；在真实 app 中验证 toggle→Preview→Apply→Launch。
  3. 汇总此前持续更新的 ExecPlan progress/result，完成后 MD+HTML 一起移到 `completed/`。
- **Test scenarios**：真实 on/off 循环、终端打开、已有 Project Plugin、Base auth 复用、重启后状态保持。
- **Verification**：见 Verification Contract。

---

## Verification Contract

### Automated Gates

1. `pnpm typecheck`
2. `pnpm test`
3. `cd src-tauri && cargo test`
4. `pnpm tauri build`

### Focused Gates

1. `cd src-tauri && cargo test --test project_codex_config`
2. `cd src-tauri && cargo test --test project_codex_plugin_install`
3. `cd src-tauri && cargo test --test project_codex_runtime`
4. `cd src-tauri && cargo test --test project_plugin_conversion_route`
5. `pnpm vitest run tests/components/ProjectDetail.test.tsx tests/components/AgentConversionDialog.test.tsx tests/lib/agentTypes.test.ts`

### Manual / Browser Gates

- 项目详情开关默认开启，切换后出现待应用而非立即生效。
- Preview 与 Apply 都显示应用内 progress；Apply 期间按钮、关闭和重复提交行为一致。
- Apply 后状态、终端按钮和 derived home 内容与所选策略一致。
- off 模式修改 Base `config.toml` 不使运行时 stale；修改派生 config 会使其 stale。
- `<project>/.codex/config.toml` 前后 digest 不变。
- 重启 AD 后 desired/applied 状态仍一致。

### Regression Matrix

| 模式 | Base config drift | Generated drift | Manifest drift | Auth | Launch |
|---|---|---|---|---|---|
| applied on | stale | stale | stale | 现有语义 | 阻止 |
| applied off | fresh | stale | stale | 现有语义 | 仅磁盘/策略一致时允许 |
| desired ≠ applied | 不适用 | 不适用 | 不适用 | 不改变 | 阻止并要求 Preview → Apply |

---

## Definition of Done

- [ ] R1-R15 均有实现与自动化或人工验收证据。
- [ ] 旧 Project/runtime 数据无需手工迁移且默认保持现有继承行为。
- [ ] on/off 两种模式的配置、插件镜像、auth、profile、freshness 和 launch 语义一致。
- [ ] Preview 无副作用；Apply 和 rollback 后 runtime state 与磁盘一致。
- [ ] 不修改 Base `~/.codex` 与项目原生 `<project>/.codex/config.toml`。
- [ ] 前端 i18n、strict schemas 与后端类型同步。
- [ ] 所有自动化 gate 通过，生产 `.app`/`.dmg` 构建成功。
- [ ] 浏览器/真实 Tauri 流程验证通过，Apply loading 体验已确认。
- [ ] 架构文档与 ExecPlan 完成归档。

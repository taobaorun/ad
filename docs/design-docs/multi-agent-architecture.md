# 多 Agent 抽象架构

> 状态：已实现（2026-07-15；设计已获用户 LGTM）
>
> 范围：Claude Code、Codex，以及未来由 AD 内置代码接入的 Agent
>
> 关联计划：`docs/exec-plans/completed/multi-agent-support.md`

## 结论

设计重开时的 v0 实现不能作为多 Agent 架构继续扩展。它虽然证明了 `agentId` 隔离、内置 registry 和 discovery IPC 可以落地，但核心抽象仍然是 Claude Code 模型：

- `ProfileFile` 即使增加了 `agentId`，payload 仍是 `ClaudeSettings + ProfileLayers`。
- `AgentAdapter` 只有 `metadata()` 和 `discover()`，却能在 metadata 中声明 settings、skills、plugins、进程探测和终端启动；声明与实现没有类型约束。
- `Capability` 只表达功能名称，不表达作用域、可执行操作、运行时可用性和限制，无法证明“能力对等”。
- `AgentInstallation` 只有 `agentId + rootPath`，把 Agent 产品、本机配置实例和配置目录混成一个概念。
- 转换直接接受 Claude profile 并生成 TOML 字符串，绕过了目标快照、并发修改检查、多文件备份和可恢复执行。
- 前端只保存 `activeAgentId`，无法表达同一个 Agent 的多个配置 home 或当前项目上下文。

该判断已落实：`src-tauri/src/agents/` 现在以 v1 contract 为核心，Claude 和 Codex 都通过 capability ports、ResourceSnapshot、MutationPlan 和共享 ExecutionEngine 工作。旧 Claude IPC/ProfileFile façade 仅作为受 policy 控制的兼容层保留，不再作为新 Agent 的扩展点。

## As-built 状态

- typed IDs 和 `AgentContext` 已贯穿 Rust、IPC、Zustand 与前端严格 schema；Agent discovery 按 adapter-owned canonical key 去重，仅返回 canonical installation。
- Claude Code 与 Codex 已实现 Settings、Skills、Plugins、Process、Launch ports，descriptor 从真实 port operation 推导，并由共享 parity contract 验证。
- `AgentProfile` envelope 已按 `(agentId, profileId)` 持久化；adapter 负责 payload 验证和 Profile→Settings 内容转换，应用仍经过安全 plan、receipt 和 rollback。
- `ExecutionEngine` 提供 digest precondition、写前全量备份、APFS 单文件原子写、失败补偿、history 和防覆盖 rollback。
- Claude Code → Codex 已实现 artifact route；source 只进入 read-set，target plan 必须显式确认，冲突和无法映射字段逐项展示。
- Plugin install 仅在真实实现存在时声明。当前 Claude 不声明 install；Codex 因 marketplace/cache/授权流程未纳入安全执行而标记 degraded，并返回结构化 `Unsupported`。

## 目标与非目标

### 目标

1. Claude Code 和 Codex 对用户呈现配置、Skills、Plugins、进程探测、终端启动的能力对等。
2. Agent-specific 的路径、格式、作用域、优先级和转换规则只存在于内置 adapter。
3. 通用层负责身份、快照、预览、备份、写入、审计和恢复，不理解 Claude JSON 或 Codex TOML 字段。
4. 新增未来 Agent 时，不在通用 command、store 和组件中散布 `agentId` 分支。
5. 任何写操作都可预览、可检测外部修改、有备份、有明确回执，并能执行补偿式回滚。

### 非目标

- 不支持用户通过 manifest、脚本或配置文件定义 adapter。
- 不追求不同 Agent 的字段一一对应；“能力对等”指用户任务和安全保证对等。
- 不读取或管理 auth、token、session、history、logs、cache、SQLite 等运行时或敏感状态。
- 不为 Linux 或 Windows 预留平台层。
- 不把多个文件的更新描述为数据库式原子事务；APFS 原子性只覆盖单文件 rename。

## 设计原则

### 1. Agent 类型、配置实例和项目上下文分离

`AgentDefinition` 是编译进 AD 的产品类型，例如 `claude-code`、`codex`。`AgentInstallation` 是 AD 在本机管理的一个有效配置实例。`AgentContext` 是当前操作目标：某个 installation，加上可选项目。

同一个 Codex 二进制使用不同 `CODEX_HOME` 时，是两个 installation；多个 Codex 启动入口共享同一个有效 `CODEX_HOME` 时，是同一个 installation。这样 identity 随被管理数据稳定，而不会因二进制升级或 symlink 目标变化而漂移。

```rust
struct AgentDefinition {
    id: AgentId,
    display_name_key: String,
    adapter_version: u32,
}

struct AgentInstallation {
    id: InstallationId,
    agent_id: AgentId,
    config_home: PathBuf,
    preferred_launcher: LaunchRecipe,
    status: InstallationStatus,
}

struct AgentContext {
    installation_id: InstallationId,
    project_path: Option<PathBuf>,
}
```

`AgentId`、`InstallationId`、`ProfileId`、`PlanId`、`ReceiptId` 使用 Rust newtype，不再使用可互换的 `String` alias。

### 2. canonical installation 由 adapter 判定

通用 registry 不再用字符串去尾斜杠冒充 canonicalization。流程是：

1. adapter 从默认位置、环境、PATH、运行进程和用户确认位置产生 discovery candidates；
2. adapter 校验 candidate，并计算不暴露到 IPC 的 `CanonicalInstallationKey`；
3. registry 按 key 确定性合并 candidates；
4. UI 和持久化层只看到一个 canonical installation，不保留 aliases；
5. discovery evidence 只用于本次诊断，不作为 alias 持久化。

用户修正配置 home 属于“确认一个 installation”，不是“配置一个 adapter”。修正后的路径仍须经过对应 adapter 校验和 canonicalization。

### 3. Capability 必须由可调用端口推导

adapter 不能手写一个与实现脱节的 capability set。它通过是否返回能力端口来表达支持，capability descriptor 从端口推导：

```rust
trait AgentAdapter: Send + Sync {
    fn definition(&self) -> &'static AgentDefinition;
    fn discover(&self, ctx: &DiscoveryContext) -> Result<Vec<InstallationCandidate>, AgentError>;

    fn settings(&self) -> Option<&dyn SettingsPort> { None }
    fn skills(&self) -> Option<&dyn SkillsPort> { None }
    fn plugins(&self) -> Option<&dyn PluginsPort> { None }
    fn processes(&self) -> Option<&dyn ProcessPort> { None }
    fn launcher(&self) -> Option<&dyn LaunchPort> { None }
}
```

每个端口同时报告：

- `scopes`：user、project；
- `operations`：inspect、edit、preview、apply、rollback，以及集合能力所需的 list/install/enable/disable；
- `availability`：available、degraded、unavailable；
- `limitations`：结构化原因和 i18n key，而不是自由文本成功假象。

“Claude/Codex 能力对等”由 operation-level contract test 判断，不能通过两个 `BTreeSet<Capability>` 相等来判断。

### 4. 通用层管理 Resource，不管理 settings JSON

所有可管理对象都以 resource 表达，但内容由 adapter 所有：

```rust
struct ResourceRef {
    installation_id: InstallationId,
    project_path: Option<PathBuf>,
    kind: ResourceKind,
    scope: ResourceScope,
    logical_id: String,
}

struct ResourceSnapshot {
    resource: ResourceRef,
    location: ResourceLocation,
    media_type: String,
    content: SnapshotContent,
    digest: ContentDigest,
    observed_at: DateTime<Utc>,
}
```

`ResourceKind` 首期包含 settings、instructions、skills、plugins、hooks、mcp、agents 和 rules。用户要求的 P0 能力仍是 settings、skills、plugins、process detection、terminal launch；其余 kind 用于正确描述 Codex/Claude 的实际配置表面和转换报告，不等于首期都提供完整编辑 UI。

`ResourceRef` 不接受前端传入任意绝对路径。路径由 adapter 根据已验证的 `AgentContext` 解析；IPC 只返回可展示的 location。

### 5. Profile 是稳定 envelope，payload 归 adapter 所有

复合身份保持为 `(agentId, profileId)`，但 common model 不再包含 Claude 字段：

```rust
struct AgentProfile {
    schema_version: u32,
    key: AgentProfileKey,
    metadata: ProfileMetadata,
    payload_schema: String,
    payload: serde_json::Value,
}
```

规则：

- `payload_schema` 例如 `ad.profile/claude-code.v2`、`ad.profile/codex.v1`；
- adapter 在保存、预览和应用边界验证 payload；
- 旧 `~/.ad/profiles/<id>.json` 继续按 Claude profile 读取，通过 compatibility reader 映射为 envelope；
- Codex profile 不再伪装成 `ClaudeSettings`，也不要求 TOML 内容塞进 Claude `layers`；
- 前端按 `payload_schema` 从一个集中 editor registry 选择编辑器。

### 6. adapter 规划变更，执行引擎唯一写盘

adapter 负责理解格式和生成变更计划，但不能直接写用户文件。共享 `ExecutionEngine` 负责：

1. 保存 backend-owned plan，IPC 只返回 plan view；
2. 用户确认时只提交 `planId`，不回传可伪造的路径或目标内容；
3. 对所有 read-set 和 write-set 重新校验 digest；
4. 在任何写入前完成全部目标备份并写 backup manifest；
5. 每个文件使用现有 `write_atomic`；
6. 记录 operation receipt 和 history；
7. 多文件中途失败时按 manifest 逆序补偿，明确报告 complete、compensated、partial_failure；
8. rollback 同样检查当前 digest，防止覆盖用户在 apply 后的新修改。

```rust
struct MutationPlan {
    id: PlanId,
    agent_id: AgentId,
    context: AgentContext,
    read_set: Vec<ReadPrecondition>,
    mutations: Vec<PlannedMutation>,
    issues: Vec<PlanIssue>,
    expires_at: DateTime<Utc>,
}
```

这不是数据库事务。安全保证是“写前全量备份 + 单文件原子写 + 明确补偿状态”，不能承诺多文件同时原子提交。

### 7. 转换是 artifact route，不是字段复制函数

Claude Code → Codex 转换由独立 `ConversionRoute` 协调 source adapter 和 target adapter：

```text
source context
  → derive one requested scope (user or project)
  → immutable source snapshots
  → artifact-level mapping
  → target snapshots + conflict analysis
  → digest-bound mutation plan
  → explicit confirmation
  → shared execution engine
  → receipt / rollback
```

一次转换只处理一个作用域。`AgentContext.projectPath` 为空时 route 只处理 user resources；存在时只处理该 canonical project 的 project resources。Project Settings 将 Claude `.claude/settings.json` 与 `.claude/settings.local.json` 按 shared → local 优先级合并，再写入同项目 `.codex/config.toml`。另一个 scope 的 artifact 不进入预览、read-set 或 write-set，避免 Project 转换隐式修改用户配置。

字段映射也遵循“确认过的语义才自动转换”：`maxContextTokens` 映射为 Codex `model_context_window`；Claude 原生 model 名不直接写入 Codex，用户可通过 route 的内置 `targetModel` 决策显式选择目标模型。Claude permissions 只有在用户选择内置预设后才生成 `approval_policy` + `sandbox_mode`，其中 bypass 对等预设必须明确显示 `never` + `danger-full-access` 风险。用户不能通过配置扩展或替换这些规则。

每个转换项必须标记：

- `exact`：语义和内容可无损迁移；
- `mapped`：通过已声明规则转换；
- `requires_input`：需要用户选择或补充；
- `unsupported`：目标没有已确认等价能力；
- `conflict`：目标已有内容且不能自动合并；
- `unchanged`：目标已等价，无需写入。

转换计划的 write-set 永远不能包含 source resource。目标已存在时默认是 merge/skip/conflict 计划，不把整份 `config.toml` 当作可直接覆盖的字符串。源和目标任何 digest 变化都会使 plan 失效，要求重新预览。
当 plan 为空时，UI 必须明确说明 source 已成功读取，并汇总 requires-input、unsupported 和 conflict 数量，不能把“没有安全可写项”表现成执行成功或静默无效。

### 8. 运行时能力复用 macOS 服务

进程探测和终端启动不应各自复制 OS 代码：

- adapter 的 `ProcessPort` 提供可验证的 matcher specification；共享 macOS process service 执行并返回标准 process view；
- adapter 的 `LaunchPort` 生成 launch recipe（command、args、env、cwd）；现有 terminal backend 只负责在 Ghostty、cmux、Terminal.app 或 Custom 中执行；
- `claude_bin` 等 Agent-specific 命名从通用 terminal command 中移除。

### 9. 前端选择的是 AgentContext

前端状态从单一 `activeAgentId` 改为：

```typescript
interface ActiveAgentContext {
  agentId: AgentId;
  installationId: InstallationId;
  projectPath?: string;
}
```

通用 UI 根据 capability descriptor 渲染 action 和 disabled reason。编辑器按 `editorKind/payloadSchema` 从一个内置 registry 选择，允许内置注册点出现一次 Agent-specific 代码，但禁止在 command、store 和组件业务路径中散布 `if (agentId === ...)`。

### 10. 安全边界是 allowlist，不是目录遍历

adapter 必须显式列出可管理 resource。即使文件位于配置 home 内，以下类别也永不进入 snapshot、profile、diff、backup 或日志：

- auth、access token、credential store；
- session、chat history、prompt history；
- logs、telemetry、cache；
- SQLite、索引和其他运行时数据库；
- adapter 未识别的可疑文件。

可编辑配置自身可能引用环境变量或包含敏感值；UI 可以按产品需要展示，但错误、日志和 history 必须红线过滤，backup 权限不得扩大。

## 目标分层

```text
React UI / Zustand
  └─ AgentContext + capability descriptors + plan views
       └─ Tauri Agent Service
            ├─ Built-in Adapter Registry
            │    ├─ Claude Code adapter
            │    └─ Codex adapter
            ├─ Profile Repository
            ├─ Conversion Coordinator
            └─ Execution Engine
                 ├─ snapshot/digest validation
                 ├─ backup manifest
                 ├─ atomic file writer
                 ├─ operation receipt/history
                 └─ compensating rollback
```

边界约束：

- adapter 读取和解析 Agent 文件、生成 snapshots 和 mutation plan；
- execution engine 是唯一用户文件写入入口；
- profile repository 只持久化 envelope，不解释 payload；
- IPC 返回 schema-validated view，不暴露 trait object 或内部 canonical key；
- UI 不自行拼接 Agent 路径，不自行推断 capability。

## 结构化错误

所有 Agent IPC 使用一致错误 envelope：

```rust
struct AgentError {
    code: AgentErrorCode,
    message: String,
    agent_id: Option<AgentId>,
    installation_id: Option<InstallationId>,
    resource: Option<ResourceRef>,
    retryable: bool,
    details: Option<serde_json::Value>,
}
```

Rust `message` 保持英文且可诊断；前端按 `code` 映射 zh/en 操作提示。解析失败、目标变化、权限不足、不支持、plan 过期和 partial rollback 必须是不同 code。

## 能力对等的验收定义

“对等”不再比较 enum，而比较用户任务：

| 能力 | 两个 Agent 都必须满足 |
|---|---|
| Settings | 识别 user/project scope；读取、编辑、预览、apply、备份、history、rollback；保留未知字段 |
| Skills | 列表、来源识别、安装/启用/禁用，以及 user/project scope 中目标 Agent 实际支持的等价行为 |
| Plugins | 列表、来源识别、安装/启用/禁用；若产品表面或授权流程不同，必须明确状态而非假成功 |
| Process detection | 识别当前 installation 的运行进程，不误报另一个 Agent 或配置实例 |
| Terminal launch | 使用选定 installation 的 launcher、环境和项目 cwd，在现有四种 terminal backend 中启动 |

若目标 Agent 在某个 operation 上不存在真实等价能力，状态必须是 `degraded/unavailable` 并解释差异；不能为了通过 parity test 伪造成功。

## Codex 事实约束

Codex adapter 必须遵守以下已核验事实，且将版本变化隔离在 adapter：

- `CODEX_HOME` 可改变 Codex 本地配置和状态根，默认是 `~/.codex`；因此不能把 `~/.codex` 硬编码成唯一 installation。
- durable settings 使用 TOML；用户级和项目级配置具有不同允许字段与信任语义。
- 当前 Codex profile 是独立的 `<name>.config.toml` 层，不能继续假设旧的 `[profiles]` table。
- Skills 的当前用户级标准位置是 `$HOME/.agents/skills`，repo scope 是 `.agents/skills`；不能把 `~/.codex/skills` 当成唯一真理源。
- Plugins、AGENTS、hooks、MCP、rules 各有独立结构和作用域，不能折叠为一个 `config.toml` 字段转换。
- 官方 import 也是按 instructions、settings、skills、plugins、MCP、hooks、subagents 等 item 分类，并保持源 Agent 不变；AD 的 conversion 采用相同的 artifact-level 思路。

参考：

- [Codex environment variables](https://learn.chatgpt.com/docs/config-file/environment-variables)
- [Codex advanced configuration](https://learn.chatgpt.com/docs/config-file/config-advanced)
- [Build Codex skills](https://learn.chatgpt.com/docs/build-skills)
- [Build Codex plugins](https://learn.chatgpt.com/docs/build-plugins)
- [Import from another agent](https://learn.chatgpt.com/docs/import)

## 已执行的迁移策略

迁移遵循“不回滚既有提交、不破坏用户数据”的并行替换方式，已完成以下阶段：

1. 冻结 v0，不在 Claude 专属模型上继续扩展 Codex 写入。
2. 引入 v1 typed contract、canonical discovery 和 AgentContext。
3. 迁移 Claude vertical，建立共享 ExecutionEngine。
4. 实现 Codex vertical，并通过 operation-level parity contract。
5. 引入 Profile envelope、artifact conversion 和 digest-protected rollback。
6. 前端切换为 AgentContext、capability descriptors 和 plan views。
7. 保留仍被 legacy Claude 流程调用的 façade；待独立迁移任务证明无调用后再删除，不在本次架构落地中冒险清理兼容路径。

## 被否决的方案

### 在 `ProfileFile` 上继续增加 Agent 字段

否决。它会让 Codex payload 继续伪装成 Claude JSON/layers，后续每个 Agent 都把专属字段塞进 `extra`。

### 一个包含所有操作的巨型 `AgentAdapter`

否决。未来能力差异会产生大量默认 no-op 或 `Unsupported` 分支，声明与实现仍可漂移。显式 capability ports 更容易测试。

### adapter 自己负责 backup 和写盘

否决。每个 adapter 会重复数据安全逻辑，转换中的跨 resource 写入也无法得到统一 receipt 和补偿状态。

### 用 `rootPath` 字符串在 registry 统一去重

否决。不同 Agent 对“同一安装”的定义不同，且 symlink、环境 home 和不存在路径不能靠 lexical trim 正确处理。

### 动态加载用户 adapter

否决。当前需求明确只允许内置 adapter；动态代码、权限和 schema 兼容成本不符合 AD 的“简单 > 灵活”。

## 实施状态

本设计已于 2026-07-15 获用户 LGTM，并在同日完成首期实现。持续约束：

- 新 Agent 从 v1 contract 和 contract tests 接入，不扩展 v0 compatibility façade；
- 不删除仍有调用的 v0 类型或兼容行为，清理必须有独立迁移测试；
- 不修改已冻结的 ExecPlan HTML，执行进展只更新 ExecPlan MD；
- 结构性实现偏差必须先回写本设计文档，再继续编码。

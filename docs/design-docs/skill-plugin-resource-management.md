# Technical Design: Skill / Plugin 资源中心与项目安装

Design identity: `TD-skill-plugin-resource-management/v1`

Decision state: accepted under the delegated engineering defaults in the Product Contract

Product Contract: `docs/product-specs/skill-plugin-resource-management.md`

Requirements covered: `R1`–`R12`

Authority: 用户于 2026-08-13 确认的产品契约，以及其中对 Agent adapter、身份、路径安全、持久化、并发与恢复的工程授权

## Current behavior, constraints, and invariants

### Current seams

当前实现已经有一部分可以直接复用的基础：

- `skill_catalog.rs`、`skill_source_bindings.rs`、`skill_catalog_plans.rs` 和 `skill_catalog_execution.rs` 管理 Skill source、Git generation / `current` 稳定视图、计划、receipt 和崩溃恢复。
- `claude_ports/skills.rs` 与 `codex_skills.rs` 已经把 Project Skill 安装为指向 source binding 的软链接。
- `collection_inventory.rs`、`collection_skills.rs`、`collection_management.rs` 和 `collection_actions.rs` 已经提供项目 + Agent 作用域的统一 inventory/action 边界。
- `ExecutionEngine` 已经具有 read set、受管目标、ownership、journal、补偿和 receipt；它仍应是文件系统和 Agent 配置写入的唯一执行入口。
- `App.tsx` 目前只有项目主视图；Skill source 管理仍位于 Settings，Project Collection 也没有完整的“已安装 / 可安装”产品结构。

当前 Plugin 路径与新契约存在直接冲突：

- Claude Plugin port 只管理既有声明的启用、禁用和移除，不负责从资源中心安装。
- Codex Plugin install 目前会把 marketplace 与 package 复制到 project-derived `CODEX_HOME`，Agent Conversion 还可能把 Claude Plugin 转换、裁剪并生成 Codex manifest。这些路径不能继续作为新资源安装路径。
- Plugin inventory 主要来自 Agent 配置或 runtime declaration，没有资源中心身份，也没有可用于全局影响枚举的项目安装账本。
- `ResourceOwnershipRecord` 目前只足以证明一个主要物理目标；它没有表达一个资源安装、其启用状态、adapter contract 与多个附加状态之间的高层关系。

### Agent capability evidence

本设计针对实现时已验证的 Agent contract，而不是假设两个 Agent 能力对等：

- Claude Code 的官方 CLI 提供可重复的 `--plugin-dir <path>`，会话可以直接从目录加载 Plugin；marketplace install 则会复制到 Claude cache。AD 因此可以通过项目/Agent 专属软链接和 AD 的 LaunchRecipe 直接加载原始 Plugin，而不走 marketplace copy。[Claude Code CLI reference](https://code.claude.com/docs/en/cli-usage)；[Claude Code Plugins reference](https://code.claude.com/docs/en/plugins-reference)
- 当前 Codex `rust-v0.147.0` 的 Plugin loader 只加载 PluginStore 中的 active installation；active version 枚举要求 version entry 本身是目录，原生 install 又会复制 source tree。当前没有可证明满足“完整原始目录通过一个稳定软链接自动传播”的直接加载协议。[Codex PluginStore](https://github.com/openai/codex/blob/rust-v0.147.0/codex-rs/core-plugins/src/store.rs)；[Codex Plugin loader](https://github.com/openai/codex/blob/rust-v0.147.0/codex-rs/core-plugins/src/loader.rs)

因此，本期能力矩阵为：Project Skill 支持 Claude Code 与 Codex；符合 Claude native Plugin 规范的 Project Plugin 支持 Claude Code；当前 Codex Plugin 明确为 unsupported。后续 Codex 若提供原生 direct-reference contract，只需增加 adapter 实现，不改变资源中心模型。AD 不通过复制、生成 manifest、拆出 Plugin 内 Skill 或创建不完整链接目录来伪造支持。

### Invariants

1. 资源中心是所有 AD Project Skill / Plugin 安装的唯一入口；public IPC 不接受 frontend 提供的任意 source path。
2. source 原始内容永不被 AD 转换、改写、裁剪、补全或重新打包。扫描只产生 metadata、digest 和诊断。
3. Local source 是外部拥有的目录，AD 永不修改或删除；Git source 使用 AD-owned generation 和稳定 `current` view。
4. Project action 只修改当前 workspace 与明确选择的 Agent。资源中心移除是单独的全局 lifecycle operation，必须先展示影响并确认。
5. 不同 source 的同名资源拥有不同 `ResourceId`，在中心共存；冲突只根据目标 Agent adapter 返回的实际 target claim 判断。
6. 只有同时存在有效 InstallationRecord 与物理 ownership evidence 的目标才允许 AD 删除或替换。其他观察项一律是 external / 非 AD 托管。
7. inventory、应用启动、source refresh 和项目扫描都是只读行为，不能自动接管 external、解除 suppressed 状态或迁移物理安装。
8. Git source 更新必须在切换 `current` 前证明所有现有受管安装在新 generation 中仍然存在且仍受相同 Agent adapter 支持；否则 fail closed。Local source 外部漂移无法被 AD 阻止，只能如实降级 health。
9. 操作状态、ownership 与 installation ledger 必须由同一个 journaled execution boundary 提交；不能报告“安装成功但没有账本”或“已移除资源但仍有受管链接”。
10. 正常 UI 不暴露 symlink、adapter、ownership、journal 等实现术语。

## Decision summary and active design dimensions

### D1 — Generalize the existing source binding; do not create a content model

`SkillSourceBinding` 演进为资源级 `SourceBinding`，继续表达 source 的 stable root、physical root、revision 和 validated manifest。Resource scanner 只识别资源根、原生安装标识、类型和描述符摘要；Skill / Plugin 文件仍由目标 Agent adapter 原样读取。

这是一层薄资源协议，不是 Skill / Plugin 内容抽象：core 不把 Claude manifest 映射成 Codex manifest，也不抽取 Plugin component。它只把同一个物理资源根交给 adapter 做 native compatibility 判断。

### D2 — Add a durable ResourceCatalog and a durable InstallationLedger

ResourceCatalog 负责“什么可以被 AD 安装”；InstallationLedger 负责“哪个资源由 AD 安装到了哪个项目和 Agent”。低层 ResourceOwnershipRecord 继续证明具体物理目标，InstallationRecord 通过 ownership record IDs 把一个逻辑安装与其物理目标、启用状态和 adapter contract 关联起来。

资源移除的影响列表只来自 InstallationLedger 和可证明的 legacy ownership，不从项目路径、Agent 配置内容或同名关系猜测。

### D3 — Agent adapters return compatibility and sealed install/uninstall plans

现有 SkillsPort / PluginsPort 保留为 Agent 差异边界，但安装输入从自由 JSON `source` 收紧为 backend-resolved catalog resource。adapter 必须先返回 `supported` 或带稳定 reason code 的 `unsupported`；supported 结果同时签发 target claim 与 adapter contract。UI 只消费结果，不生成 Agent 路径或安装策略。

### D4 — Claude Plugin uses a project/Agent link plus LaunchRecipe; Codex Plugin is blocked

Claude Plugin install 在 AD 的 workspace-scoped runtime root 下创建一个指向 catalog resource stable path 的软链接。InstallationRecord 保存启用状态；Claude LaunchPort 对当前 workspace 的 enabled Plugin 追加一个 `--plugin-dir <managed-link>` 参数。多个 Plugin 使用多个 flag。卸载删除该 link 和 record；禁用只改变当前安装的启用状态，使 LaunchRecipe 不再加入对应 flag。

该 contract 的运行边界是“通过 AD 打开的当前项目 Claude Code 会话”。中心更新后，新会话直接读取新内容；已有会话按 Claude native contract 使用 `/reload-plugins` 或重启会话重新加载非 Skill component。AD 不写 user/global Plugin cache。

当前 Codex Plugin adapter 对 catalog Plugin 返回 `unsupported_agent_capability`。既有 Codex copy/transform 安装保持可读、可证明时可卸载，但不能作为新资源安装或更新路径。

### D5 — Resource removal is a durable coordinator over the standard project uninstall

单个中心资源移除不是直接删 catalog row。它先冻结资源 lifecycle、枚举 active installations，经一次影响确认后，按确定顺序逐个调用与项目页相同的 standard uninstall planner/executor。成功项不因其他项失败而回滚；资源只在 active installation 数量为零后转为 suppressed。

### D6 — Product information architecture separates library from project state

主窗口增加与项目并列的“资源”顶级入口。资源中心默认围绕 Skill / Plugin 展示受管库；source 是二级管理对象。项目页围绕当前 Agent 展示“已安装”和“可安装”，external 只显示“非 AD 托管”及重新检查。机制说明、完整路径和诊断细节只在必要的按需详情中出现。

## Proposed structure and responsibilities

### Resource catalog domain

新增 `resource_catalog` domain，逐步取代仅 Skill 的 catalog public surface：

- `ResourceCatalogRepository`：唯一读写 `resource_catalog.json`，维护 revision、dual-read migration 和 CAS。
- `SourceAcquirer`：复用 Local validation 与 Git generation / `current` 机制，绝不发布转换后的 resource tree。
- `ResourceScanner`：在 selected source root 内发现 Skill / Plugin 候选并做 source-local uniqueness 校验。
- `CompatibilityInspector`：把 resolved resource 交给已发现的 Agent adapter，返回 frontend-safe compatibility。
- `ResourceCatalogPlanStore` / executor：负责 source add/refresh/update/remove、resource re-add/suppress 计划、journal、receipt 和 recovery。

Source scan 先由 native descriptor readers 提议 Plugin roots，再发现不位于任何 Plugin root 内的 standalone Skill：

- Standalone Skill 必须有合法 `SKILL.md`，其 native install ID 来自现有 Skill 规范解析结果。
- Plugin root 由 `.claude-plugin/plugin.json`、`.codex-plugin/plugin.json` 或对应 Agent 已验证的原生无 manifest 规则识别。若多个 native descriptor 指向同一 subpath，它们只有在声明的 install ID 一致时才合并为一个 Plugin resource；不一致则该 source scan 非法。
- Plugin 内部的 Skill 不作为独立 Skill 候选；那会等价于裁剪 Plugin。
- marketplace manifest 中指向其他 Git/NPM/URL 的条目不递归获取。只有当前 Git/local source checkout 内的物理 resource root 可被纳管。
- 同一 source 中 `(ResourceKind, install_id)` 必须唯一。重复时整个新 scan 结果不发布；Git 不切换 `current`，Local source 标记 invalid 并暂停新安装。不同 source 不做这项冲突判断。

### Installation and ownership domain

新增 `resource_installations` domain：

- `InstallationRepository`：按 installation ID 保存记录，提供 workspace、resource 和 target claim 索引。
- `ManagedCollectionService`：从 catalog resource + workspace + Agent 生成 preview，检查 compatibility、target occupancy 和 current installation state。
- `StandardUninstallService`：只接受 InstallationRecord 或可证明的 legacy ownership，委派记录中的 adapter contract 生成 cleanup plan。
- `ResourceLifecycleCoordinator`：枚举影响、持久化移除 operation、顺序执行 standard uninstall、发送进度并最终 suppress resource。

InstallationRecord 是全局影响枚举和高层生命周期的权威；ResourceOwnershipRecord 仍是物理 mutation authority。一个 InstallationRecord 可以引用多个 ownership record，以容纳未来 Agent 需要 link 与附加注册目标的情况，但本期 Skill 和 Claude Plugin 的内容目标都是一个 symlink。

### Agent adapters

现有 AgentRegistry 继续发现 Claude Code / Codex installations。SkillsPort / PluginsPort 的职责变为：

1. 读取 resolved catalog root 与 native descriptors，判断特定 Agent installation 是否支持。
2. 返回精确、backend-owned target claim。
3. 为 install、set-enabled、uninstall 生成 MutationPlan 与 InstallationRecord delta。
4. 只写当前 workspace / effective Agent runtime 内的目标。
5. 在 LaunchPort 需要时，根据 enabled InstallationRecord 装饰 LaunchRecipe。

adapter 不拥有 catalog lifecycle，不可以自行获取 source，也不能把 external declaration 自动变成 AD-owned。

### Frontend

- `App` / `useUiState`：增加 `projects | resources` 顶级 workspace mode。
- `ResourceCenter`：资源库、类型/source filter、resource detail、source drawer、导入 source 和更新入口。
- `Project Resource Panel`：按当前显式 Agent 选择显示 installed / available；candidate 显示 source type 与可区分的 source label。
- `ResourceRemovalDialog`：影响确认、overall progress、per-installation progress、partial failure 与 retry。
- `resourceCatalog` store：严格解析 backend schema；任何 action 完成后按受影响范围刷新 catalog / project inventory，不在 frontend 推断状态。

## Interfaces and data/control flow

### Durable catalog schema

```rust
struct ResourceCatalogDocumentV1 {
    schema_version: u32,                 // exactly 1
    revision: u64,                       // monotonic CAS revision
    sources: BTreeMap<SourceId, CatalogSource>,
    resources: BTreeMap<ResourceId, CatalogResource>,
}

struct CatalogSource {
    id: SourceId,
    display_name: String,
    source_type: SourceType,             // git | local
    location: String,
    branch: Option<String>,
    subdirectory: Option<String>,
    binding: Option<SourceBinding>,
    health: SourceHealth,
}

struct SourceBinding {
    binding_id: String,
    source_revision: String,
    stable_root: String,
    physical_root: String,
    tree_digest: ContentDigest,
    manifest_digest: ContentDigest,
}

struct CatalogResource {
    id: ResourceId,                      // opaque(source_id, kind, install_id)
    source_id: SourceId,
    kind: ResourceKind,                  // skills | plugins
    install_id: String,
    display_name: String,
    description: Option<String>,
    subpath: String,                     // safe relative path
    descriptor_digest: ContentDigest,
    compatible_agents: BTreeSet<AgentId>,
    present: bool,
    lifecycle: ResourceLifecycle,        // managed | suppressed
    suppressed_at_revision: Option<u64>,
    last_diagnostic_code: Option<String>,
}
```

Rules:

- `ResourceId` includes source identity, so cross-source same-name candidates are distinct.
- `resources` has a unique logical constraint on `(source_id, kind, install_id)` enforced before write.
- `subpath` must be normalized, relative, no traversal and must resolve within the current physical root without escaping symlinks.
- `suppressed_at_revision` is required exactly when lifecycle is suppressed. Source scan can update `present` and diagnostics but cannot change suppressed to managed.
- Re-add is an explicit CAS mutation allowed only when the live source scan contains exactly one valid matching resource. It changes lifecycle only; it does not create an InstallationRecord.
- `compatible_agents` is a scanner/adapter capability summary for product display. This release validates standard Skill layout for both Agents, Claude native Plugin descriptor presence for Claude Code, and reports Codex Plugin as unsupported. Install planning independently revalidates the same native contract; Agent binary-version probing is deferred until an adapter needs a version-gated capability.

### Durable installation schema

```rust
struct ResourceInstallationRecordV1 {
    schema_version: u32,                 // exactly 1
    id: ResourceInstallationId,
    resource_id: ResourceId,
    source_id: SourceId,
    resource_kind: ResourceKind,
    install_id: String,
    workspace_key: WorkspaceKey,
    agent_id: AgentId,
    effective_installation_id: InstallationId,
    canonical_project_path: String,
    adapter_contract: AdapterContractRef,
    target_claim_id: PhysicalTargetId,
    state: InstallationState,            // enabled | disabled
    ownership_record_ids: Vec<OwnershipRecordId>,
    created_by_receipt_id: ReceiptId,
    updated_by_receipt_id: ReceiptId,
}
```

Rules:

- active records are unique by `(workspace_key, agent_id, target_claim_id)`；另一个 source 的同名资源因此在安装目标处冲突，而不是在 catalog 冲突。
- referenced ownership records保存 source/binding/resource subpath identity；InstallationRecord以`resource_id`关联catalog，不把可变tree digest当作ownership authority。
- every referenced ownership record must belong to the same workspace and installation operation. A missing/corrupt record makes mutation unavailable and never grants cleanup authority.
- install、enable/disable、uninstall 与 record delta 通过同一个 ExecutionEngine journal 提交。
- standard uninstall 删除所有可证明的 owned targets 和 AD 添加的当前 Agent registration，然后删除 InstallationRecord；source 不在 mutation set 中。

### Backend-only adapter contracts

```rust
struct ResolvedCatalogResource {
    resource_id: ResourceId,
    source_id: SourceId,
    kind: ResourceKind,
    install_id: String,
    stable_path: PathBuf,
    physical_path: PathBuf,
    descriptor_digest: ContentDigest,
}

enum AgentCompatibility {
    Supported {
        adapter_contract: AdapterContractRef,
        target_claim: PhysicalTargetId,
    },
    Unsupported {
        code: String,
        message_key: String,
    },
}

trait ManagedCollectionPort {
    fn compatibility(&self, context: &AgentContext,
                     resource: &ResolvedCatalogResource)
        -> Result<AgentCompatibility, AgentError>;
    fn plan_install(&self, context: &AgentContext,
                    resource: &ResolvedCatalogResource)
        -> Result<ManagedInstallationPlan, AgentError>;
    fn plan_set_enabled(&self, context: &AgentContext,
                        installation: &ResourceInstallationRecordV1,
                        enabled: bool)
        -> Result<ManagedInstallationPlan, AgentError>;
    fn plan_uninstall(&self, context: &AgentContext,
                      installation: &ResourceInstallationRecordV1)
        -> Result<ManagedInstallationPlan, AgentError>;
}
```

`ManagedInstallationPlan` 包含私有 MutationPlan、InstallationRecord delta 和 adapter contract；frontend 只能得到 sanitized plan view。现有 SkillsPort / PluginsPort 可以实现或委派这条共享 contract，但不需要把两类资源内容统一。

Public project action 继续提交 backend-issued `ResourceKey`、workspace revision、action 和用户选择的 Agent installation。backend 从 ResourceKey 反查 managed CatalogResource；不存在、suppressed、stale 或 frontend 试图提交 path 时拒绝。

### Catalog commands

Public IPC 最小集合：

- `list_resource_catalog()`：返回 managed resources、source summaries、health 和按已发现 Agent 计算的 compatibility summaries。
- `inspect_resource_source(source_id)`：返回 source detail，以及当前 scan 中 suppressed resources 的“可重新添加”列表。
- `preview/apply_resource_source_action(...)`：add、refresh/update 与 source remove。
- `preview_readd_catalog_resource(resource_id)` / `apply...`：只改变 catalog lifecycle。
- `preview_remove_catalog_resource(resource_id)`：返回 plan ID、catalog revision、resource summary、affected project count、affected Agent installation count 和 per-installation summaries。
- `apply_remove_catalog_resource(plan_id, risk_fingerprint, confirmed, progress_channel)`：开始或恢复一个 durable lifecycle operation。
- `retry_remove_catalog_resource(operation_id, progress_channel)`：重新检查真实 ledger，只处理仍存在的 active installations。

Progress event 是有序、可丢失但可从 durable state 重建的 observation：

```rust
struct ResourceRemovalProgress {
    operation_id: ResourceLifecycleOperationId,
    sequence: u64,
    phase: RemovalPhase,                 // uninstalling | suppressing | complete | partial_failure
    completed: usize,
    total: usize,
    item: Option<RemovalItemProgress>,
}

struct RemovalItemProgress {
    installation_id: ResourceInstallationId,
    workspace_key: WorkspaceKey,
    agent_id: AgentId,
    state: RemovalItemState,             // pending | running | succeeded | failed
    diagnostic_code: Option<String>,
}
```

Channel disconnect 不取消已确认的 destructive operation。UI 重连后读取 operation snapshot；event 不是结果权威。

### Install flow

```text
project + explicit Agent
  -> list managed catalog candidates
  -> adapter compatibility + target claim
  -> inspect exact target / installation index
  -> unsupported OR external/managed conflict OR sealed install preview
  -> user confirmation
  -> ExecutionEngine writes content link + Agent state + ownership + installation record
  -> refresh current project inventory and catalog usage count
```

冲突分支不提供“切换来源”。如果 target claim 已属于另一份 AD installation，UI 显示其 source type 并只提供标准卸载；卸载完成后用户重新发起 install。如果 physical target 存在但无完整 ownership/installation evidence，inventory 标记“非 AD 托管”，AD 只允许 recheck。

### Git source update flow

```text
stage new generation
  -> scan exact original tree; reject duplicate IDs/non-standard resources
  -> join every active installation against new resource subpath
  -> rerun its adapter compatibility contract
  -> if any installed resource disappears/becomes unsupported: blocked preview
  -> publish generation and atomically switch source current
  -> commit catalog revision + receipt
  -> every project link resolves the new source content without link rewrite
```

Source update never rewrites InstallationRecords solely because content digest changes. Descriptor/health snapshots update. Local source external drift follows the same validation on next scan, but AD cannot roll back or prevent the already-visible change; affected installations become degraded and remain removable.

### Resource removal flow and state machine

```text
managed
  -> preview (impact snapshot, no writes)
  -> confirmed/running (resource lifecycle lease held)
       -> uninstall installation 1: succeeded|failed
       -> ... sequentially
       -> any active/failed remains: partial_failure -> retry -> running
       -> no active remains: suppressing -> catalog CAS -> complete/suppressed
```

Apply 在第一项写入前重新校验 catalog revision 与 exact installation ID set；如果期间新增安装或资源发生变化，整个 apply 返回 stale，不产生 mutation。进入 running 后，resource lifecycle lease 阻止新 install、re-add、source switch 和 child receipt rollback 破坏影响集合。

每个 uninstall 都是独立 receipt-backed transaction。某项 failure 不伪造成功，也不补偿已成功的其他项目；resource 保持 managed，项目页继续可见，retry 重新枚举剩余记录。catalog CAS 在所有 uninstall 后失败时，项目已经安全卸载但 resource 仍 managed；重试只完成 suppress。

Crash recovery 对 `running` item 检查 receipt、InstallationRecord 与 ownership：能证明 uninstall complete 则标 succeeded；仍完整则回到 failed/retryable；不确定或损坏证据则 fail closed，保留 resource managed。startup 不自动继续 destructive writes。

Source remove 是同一 coordinator 的组合操作：它对 source 下每个 managed resource 执行相同 lifecycle，并在全部资源无 active installation 后移除 source catalog entry。Local directory 永不删除；Git generations 在本期保留供 recovery/history，物理 GC 不在范围内。任一资源 partial failure 时 source 仍存在。

## State, failure, compatibility, migration, security, and operations

### Suppression lifecycle

- `managed + present`：出现在资源中心和项目 available candidates。
- `managed + absent/invalid`：保留管理与卸载能力，但不能新安装；source detail 显示简洁 health。
- `suppressed`：不出现在普通资源库和项目 candidates；source scan、refresh、Git update、restart 都只更新其 present/diagnostic，不自动恢复。
- `suppressed -> managed`：仅由资源中心 source detail 中的明确 re-add preview/apply 触发；之后仍需在具体项目另行 install。

### Migration and compatibility

迁移采用 read-compatible、write-on-explicit-action，不在启动或 inventory 静默改写：

1. 当 `resource_catalog.json` 尚不存在时，reader 把 `skill_catalog.json` v2 投影为 ResourceCatalog view，保留 source ID、现有 stable/physical roots 和 Skill subpaths。第一次明确的 catalog mutation 在 journal 中写入新 document；旧文件保留为 recovery evidence，不 dual-write。
2. 新 source 使用 general resource library path；既有 Git source 继续使用其已记录的 `skill-library` root，避免移动或复制 checkout。物理目录名不是产品语义。
3. 现有 Skill ownership v2 在 inventory 中继续被识别为 AD-managed。第一次 install-state mutation 可以在同一 receipt 中 backfill InstallationRecord；资源移除也可直接把可证明的 legacy ownership 作为 impacted installation 并调用 legacy standard uninstall，不要求先迁移。
4. 既有 AD 复制/转换产生的 Codex Plugin 不自动映射到资源中心、不重新链接、不删除。可证明 ownership 时显示为既有 AD 安装并保留 uninstall；无法证明时按 external。要使用新模型，用户先卸载，再把原 source 加入资源中心并重新安装。
5. Agent Conversion 不再为新操作准备、转换或安装 Plugin package。Plugin 只报告为未迁移/unsupported residual；Skill 和其他已确认 conversion behavior 不因此重构。
6. 历史 receipts 和 ownership schema 继续可读；本期不删除旧 staging、artifact、catalog 或 Plugin cache。破坏性 cleanup 另行设计。

旧版应用不会理解新的 resource catalog，因此迁移后的 rollback 是“恢复应用版本并恢复迁移前 catalog backup”，不是让新旧 writer 并发。journal 和 migration receipt 必须保存 exact before bytes。

### Failure and concurrency

- Catalog writes 使用 process-wide catalog writer lock + revision CAS；source operation 还锁定 source root，resource lifecycle operation 锁定 resource ID。
- Project install/uninstall 使用 workspace/Agent target locks；同一 physical target 不能并发创建或移除。
- catalog preview、project inventory revision、target digest、source binding identity 和 adapter capability result 都是 apply preconditions。
- plan expiration 或 stale precondition 返回结构化 retryable error，不复用旧 mutation authority。
- Git publish/current switch、catalog write、receipt write继续使用 existing source journal 的 finish-or-compensate 规则。
- multi-project removal 不追求跨项目原子性；它追求每项原子、整体 truthful、可重试且最终只在零引用时 suppress。

### Security and trust boundary

- Git URL/local path 只在 source add plan 进入 backend；project install public request永远不携带 path。
- scanner 使用现有 tree entry/depth/file/total-size limits、no-follow traversal 和 UTF-8/safe-relative-path 规则；不执行 Plugin hook、script、package manager 或 Agent-provided validation command。
- Local source 与 Git checkout 被视为可执行 Agent 内容。UI 显示 source type 和可辨识 source，确认安装包含 code-execution impact，但不在常态页铺陈实现机制。
- symlink lexical target 与 canonical validation 分离：写入 link 的是 stable resource path，apply 前 canonicalize physical root 并验证 confinement。人工 retarget 立即使 ownership 失效。
- lifecycle coordinator 只能使用 InstallationRecord + ownership evidence；同名、相同内容 digest 或 Agent declaration 都不能授予删除权。
- UI/logging 默认使用 source label、project display name 和 opaque IDs；完整本地 path 只在按需诊断中显示，不进入普通 progress copy。

### UX states and accessibility

资源中心必须覆盖 loading、empty、healthy library、source degraded、resource unsupported、operation error 和 partial failure。项目页覆盖无项目、Agent unavailable、无 installed、无 available、managed conflict、external conflict 与 action pending。每个主区域只有一个视觉主操作；高级 source 字段、完整诊断和历史信息按需展开。

安装流程必须在确认面板中显式显示目标 Agent，即使它默认来自 titlebar selection。unsupported candidate 保持可见但 disabled，附一条简洁原因。不同 source 的同名 candidate 同时显示 source type badge 与 source label。

键盘 focus、屏幕阅读器 label、状态非仅靠颜色表达、窄窗口滚动/截断和 destructive dialog focus trap 沿用现有 desktop design system。最终验收必须在真实 production-like Tauri desktop window 进行，不能只用组件快照代替。

## Alternatives and rejected approaches

### Build a universal Skill / Plugin content protocol

Rejected。它会迫使 AD 定义 component 映射、丢失语义或生成 Agent-specific content，成本高且直接违反“只管理资源、原始内容不能转换”。本设计只统一资源 lifecycle 与 adapter contracts。

### Keep Plugin conversion/copy and call it an adapter detail

Rejected。复制后的目录不会自动跟随中心 source；转换、生成 manifest 或抽取 portable components也违反 R3/R6。现有路径只作为 legacy uninstall compatibility 存在。

### Create a real Codex cache directory whose children are symlinks

Rejected。它不能保证 source 新增顶层文件或改变任意结构后自动出现，且某些 native loader 会对 canonical containment 做校验；这不是完整原始 Plugin root 的稳定链接。当前 Codex Plugin 应如实 unsupported。

### Treat Agent declarations as install sources

Rejected。它会绕过资源中心、要求推断 external 来源，并可能把用户自行安装的内容接管为 AD-owned。Agent declarations只用于项目 inventory。

### Remove a catalog resource immediately and leave dangling project links

Rejected。它破坏 standard uninstall、ownership 与项目可理解性。资源只能在所有可证明的安装清理完成后 suppressed。

### Roll back every successful uninstall when one project fails

Rejected。跨项目补偿会把一次可重试的 partial failure 扩大为更多写入和不确定结果。逐项原子、整体进度和保留 catalog resource 更符合 R10。

### Auto-re-add a resource when source scan sees it again

Rejected。source update 与 restart 会绕过用户明确的资源中心生命周期选择，违反 R11。

## Risks and verification approach

| Risk | Design mitigation | Required proof seam |
|---|---|---|
| Local source live edit立即改变所有项目行为 | 明示 Local source identity；ownership 不绑定可变 digest；invalid drift 降级 health | 两项目 live-link fixture，link bytes不变、内容变化、ownership仍可卸载 |
| Git update破坏已安装资源 | generation staging；join all installations；重新跑 adapter compatibility；原子 current switch | missing/renamed/unsupported resource update均在switch前阻塞 |
| 错把 external 当成 AD-owned | InstallationRecord + ownership 双证据；target drift fail closed | external file/dir/symlink/config fixture 全程零写入 |
| 资源移除中途崩溃或 UI 断连 | durable operation journal、sequence、reconciliation、explicit retry | crash at every item boundary；重启后进度与真实ledger一致 |
| 同名资源冲突规则错误 | catalog identity含source；adapter target claim唯一 | 跨source同名可共存，同source重复被scan拦截，目标冲突需先卸载 |
| Agent升级改变 Plugin contract | compatibility绑定 adapter contract；install/apply重验原生 descriptor；未来出现version-gated contract时再引入版本probe并fail closed | pinned adapter fixtures + real Claude/Codex contract smoke |
| Claude Plugin只在AD launch生效导致误解 | Installation与LaunchRecipe同一workspace contract；project page从ledger展示；真实launch验收 | production app从项目打开Claude，Plugin可见；disable/uninstall后不可见 |
| legacy Plugin被静默删除或重新包装 | dual-read、无startup mutation、仅可证明uninstall、no new conversion | legacy copied Plugin fixture在upgrade后仍可见/可卸载，bytes未被改写 |
| UI重新堆满机制说明 | library-first IA、progressive disclosure、one-primary-action | Product/Design在真实桌面窗口完成任务和视觉评审 |

## Scope deltas and specialist evidence

- `technical-necessary` — ResourceCatalog schema、suppression tombstone 与 InstallationLedger。没有它们无法证明唯一安装来源、跨项目影响、external 边界或移除后的持续排除。Authority: Product Contract delegated engineering defaults.
- `technical-necessary` — Claude LaunchRecipe direct-reference adapter 与 Codex Plugin unsupported capability。没有它就只能复制/转换或虚假声明能力对等。Authority: R3、R5、R6 and current Agent contracts.
- `technical-necessary` — durable multi-project removal coordinator。没有它无法提供 R10 的逐项进度、partial failure、crash recovery 和零悬空保证。Authority: R10.
- `technical-necessary` — legacy read compatibility and retirement of new Plugin conversion/copy。没有它会破坏既有用户数据或继续违反新契约。Authority: repository baseline and superseded semantics.
- `adjacent/deferred` — resource versioning/pin/rollback、OSS provider、marketplace discovery、Git generation GC、external takeover、Codex future direct Plugin support。Disposition: defer per Product Contract.

Implementation should use `ad-frontend` for the resource-center and project-resource experience, `ad-security` for source/path/ownership/lifecycle authority, and `ad-migrate` for catalog/ownership/legacy Plugin compatibility. A separate `ad-doc-review` is not required at planning time: migration is dual-read and fail-closed, the destructive operation is explicitly confirmed and journaled, and no irreversible data cleanup is part of this design.

## Open technical decisions

None. Private helper names, exact React component splits, non-safety-critical progress scheduling and cosmetic layout values remain local reversible implementation details.

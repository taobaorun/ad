# Implementation Plan: Skill / Plugin 资源中心与项目安装

Plan identity: `IP-skill-plugin-resource-management/v1`

Product Contract: `docs/product-specs/skill-plugin-resource-management.md`

Technical Design: `TD-skill-plugin-resource-management/v1` at `docs/design-docs/skill-plugin-resource-management.md`

Requirements: `R1`–`R12`

Commit policy / authority: `none`。用户本次只调用 `ad-plan`，授权写入计划/设计文档，不授权为后续产品实现创建 commit、push、PR、merge 或部署。后续 `ad-work` 可在其自身权限范围内修改工作树，但不能从本计划获得 Git 或远端写权限。

## Goal and execution boundary

交付一个独立的 Skill / Plugin 资源中心，并让所有 AD 项目安装都从该中心选择资源和目标 Agent。内容安装保持指向 Local 原始目录或 Git managed checkout 的稳定软链接；项目 action 只影响当前项目与 Agent。外部安装只标记为“非 AD 托管”。移除中心资源前，按项目标准卸载全部受管安装并展示可恢复的逐项进度。

开发与验证必须使用临时 `AD_HOME`、临时项目、临时 Agent home 和本地 Git fixture，不得导入、更新、删除或重写用户真实资源中心、Agent home、项目 Skill / Plugin 与 Git checkout。计划不授权清理现有 legacy artifact/cache。

## Implementation decisions

### I1 — Preserve the current execution engine as the only mutation authority

Resource Catalog、Installation Ledger 与 lifecycle operation 都接入现有 preview/apply、risk fingerprint、target lock、journal、ownership、receipt 与 recovery 约束。不得另写即时 Tauri command 直接创建链接、修改 Agent 配置或删除项目目标。

### I2 — Evolve public collection actions without exposing source paths

以 backend-issued `ResourceId` / `ResourceKey` 取代 `CollectionInstallRequest.source: Value` 的 managed install 用法。旧字段只在 legacy reader/内部迁移路径暂存，不允许新的 frontend caller 填写 path、marketplace staging 或转换结果。

### I3 — Use stable error and limitation codes as the UI contract

至少固定这些 frontend-safe codes：`unsupported_agent_capability`、`nonstandard_duplicate_install_id`、`catalog_resource_suppressed`、`managed_target_conflict`、`external_target_conflict`、`installation_evidence_invalid`、`source_update_breaks_installation`、`resource_removal_partial_failure`。UI 根据 code + `messageKey` 呈现简洁下一步，不解析 Rust message。

### I4 — Keep old physical roots and state as compatibility evidence

既有 `skill-library` stable roots、`skill_catalog.json`、Skill ownership v2、Codex copied Plugin runtime 与历史 receipts 不搬移、不改写、不删除。新 catalog mutation完成迁移后，旧 catalog仅作为只读 backup/evidence；新 source 使用 general resource library root。

### I5 — Source removal composes resource removal

“删除 source”不拥有第二套清理规则。它枚举 source 下所有 managed resources，逐个调用同一个 resource lifecycle coordinator；全部完成后才删除 catalog source entry。Local directory不删，Git generations本期不做物理 GC。

### Rejected implementation shortcuts

- 不把 Plugin 内容复制到 Agent cache 后再保存 source pointer；复制内容本身已经违反更新传播。
- 不由 frontend 将同名候选合并、判断冲突或推断 external 来源。
- 不在 source scan、app startup 或 project inventory 中自动写入 InstallationRecord、接管 external 或解除 suppression。
- 不为 Codex Plugin 构造“真实目录 + 若干子项软链接”的不完整 package。
- 不把多项目移除做成一次跨项目大 MutationPlan；每个标准卸载必须保留独立 receipt 和真实结果。

## Scope deltas

- `technical-necessary` — 新 ResourceCatalog、InstallationLedger、suppression lifecycle 与 resource removal journal。它们是 R6–R11 的最小持久化前提。Authority: accepted Technical Design D1、D2、D5。
- `technical-necessary` — Claude project Plugin launch decoration 与 Codex Plugin unsupported capability。它们是当前 Agent contract 下不复制、不转换的唯一诚实实现。Authority: accepted Technical Design D3、D4。
- `technical-necessary` — legacy Plugin conversion/copy 的新入口退役和只读/卸载兼容。继续创建这些安装会与 R3/R6 冲突，直接删除又会破坏已有状态。Authority: Product Contract repository baseline and Technical Design migration contract。
- `technical-necessary` — 顶级资源中心与 project installed/available 信息架构。单纯把 source 表从 Settings 搬出不能满足 R1/R12。Authority: R1、R12 and Technical Design D6。
- `adjacent` — version pin/rollback、OSS provider、marketplace discovery、Git generation GC、external takeover、Codex future direct Plugin adapter、Windows/Linux link variants。Disposition: defer。

## Implementation units

### U1 — Deliver a managed resource library from Git and local sources

- Requirements: `R1`, `R2`, `R3`, `R5`, `R11`
- Dependencies and accepted-design pointers: Technical Design D1、D2、catalog schema、suppression lifecycle 与 migration step 1–2。
- Observable slice: 用户从主窗口进入“资源”，导入 Git 或本地目录后看到其中原生 Skill / Plugin；同 source 重复 install ID 被拦截，不同 source 同名资源并存且显示 source type。未安装资源可以从中心移除为 suppressed，并从 source detail 主动重新添加；刷新、更新、重启不会自动恢复。
- Affected modules and mutation:
  - 新增 `src-tauri/src/agents/resource_catalog.rs`、`resource_scanner.rs`、`resource_source_bindings.rs`、`resource_catalog_plans.rs`、`resource_catalog_execution.rs`，从现有 `skill_catalog*` 和 `skill_source_bindings.rs` 提取/复用 source acquisition、generation/current、journal 和 recovery，不复制 resource content。
  - 扩展 `src-tauri/src/agents/execution_state.rs`、`src-tauri/src/fs/paths.rs`、`src-tauri/src/agents/mod.rs`、`src-tauri/src/commands/mod.rs`、`src-tauri/src/lib.rs`，注册新 state directories、plan store、startup recovery 与 resource catalog commands。
  - 保留 `skill_catalog.json` v2 dual reader；新增 `resource_catalog.json` v1 writer、migration backup/journal 和旧 physical binding reuse。
  - 新增 `src/lib/resourceCatalogTypes.ts`、`src/store/resourceCatalog.ts`、`src/components/ResourceCenter.tsx` 及 Tauri wrappers；在 `src/App.tsx`、`src/store/ui.ts`、`ProjectSidebar.tsx` 增加顶级 Projects / Resources 导航。source fields 收到 resource detail/drawer，不再以 Settings source 表作为主入口。
  - 更新 `src/i18n/locales/zh.json`、`en.json`，只呈现资源、来源类型、兼容性与可执行下一步，不暴露 checkout/journal terminology。
- Entry / exit conditions:
  - Entry: 当前 Skill catalog v2、Git current binding 与 source tests保持可读；记录 baseline state fixtures。
  - Exit: Git/local source导入→scan→resource list→refresh/update→suppress→restart→re-add 全链路可用；scan失败不发布新 catalog/current；source 原始 bytes不变。
- Focused verification:
  - Rust fixtures覆盖 standalone Skill、Claude Plugin、Codex descriptor、Plugin-contained Skill不重复导入、同 source重复 `(kind, install_id)`、跨source同名、escaping symlink、limits、Git update、Local drift、suppression persistence与v2 migration。
  - React tests覆盖顶级导航、loading/empty/error/degraded、source type辨识、suppressed re-add 和一个主操作。
  - Focused commands: `cargo test --manifest-path src-tauri/Cargo.toml resource_catalog`；`cargo test --manifest-path src-tauri/Cargo.toml resource_scanner`；`pnpm vitest run tests/components/ResourceCenter.test.tsx tests/lib/resourceCatalogTypes.test.ts`。
- Recovery checkpoint: catalog migration前保存 exact v2 bytes；任何新 writer/journal recovery失败都恢复旧 bytes或完整新 document，禁止 half-migrated state。U1不删除旧 catalog或checkout。

### U2 — Install and uninstall managed Skills for both Agents with an installation ledger

- Requirements: `R4`, `R6`, `R7`, `R8`, `R9`
- Dependencies and accepted-design pointers: U1；Technical Design D2、D3、InstallationRecord schema、install flow 与 external ownership invariant。
- Observable slice: 在 Project A 选择 Codex 或 Claude Code，从资源中心安装 Skill；目标是指向 catalog stable path 的软链接。Project A 的另一个 Agent与Project B不变。中心更新后两个安装无需重新安装即可读取新内容。卸载只清理当前项目/Agent；外部占位显示“非 AD 托管”且无卸载/接管动作。
- Affected modules and mutation:
  - 新增 `src-tauri/src/agents/resource_installations.rs`、`managed_collection.rs`，实现 InstallationRepository、target-claim index、typed resolved resource、install/uninstall plan delta 和 legacy Skill ownership adapter。
  - 将 `capabilities.rs` managed collection request从free-form `source: Value`收紧为backend-only resolved resource/installation contract；更新 `claude_ports/skills.rs`、`codex_skills.rs` 复用 stable lexical target + canonical validation。
  - 将 `resource_ownership.rs` 升级为可引用 generic catalog source binding 的新 reader/writer，同时继续读取 v2 Skill records；`execution.rs`、`execution_recovery.rs` 在同一 receipt提交 ownership与InstallationRecord delta。
  - 更新 `collection_skills.rs`、`collection_inventory.rs`、`collection_management.rs`、`collection_actions.rs` 和 workspace contracts：available只来自 managed catalog；installed由Agent observation与ledger/ownership合并；external保持只读；target conflict返回来源信息和标准卸载下一步。
  - 更新 `AgentCollectionPanel.tsx`、workspace operations store、types/i18n，加入显式 Agent target、Installed / Available 与“非 AD 托管”状态。
- Entry / exit conditions:
  - Entry: U1能backend-resolve唯一 ResourceId 到 stable/physical path；现有 ExecutionEngine recovery green。
  - Exit: 两项目 × 两Agent install/uninstall/update propagation隔离矩阵通过；managed conflict必须先标准卸载，external conflict零写入；不存在“link成功但ledger缺失”。
- Focused verification:
  - 扩展 `collection_actions_tests.rs`、`resource_ownership` / execution recovery tests与 `tests/project_agent_workspace.rs`；验证source content digest变化不撤销ownership，link retarget/目录占位/损坏ledger fail closed。
  - React tests覆盖Agent选择、同名异source候选、managed conflict→uninstall→用户另行install、external只读与busy/error。
  - Focused commands: `cargo test --manifest-path src-tauri/Cargo.toml managed_collection`；`cargo test --manifest-path src-tauri/Cargo.toml collection_actions`；`cargo test --manifest-path src-tauri/Cargo.toml resource_ownership`；`pnpm vitest run tests/components/AgentCollectionPanel.test.tsx tests/lib/agentTypes.test.ts`。
- Recovery checkpoint: InstallationRecord write failure必须触发现有link/config compensation；legacy ownership只读可用，backfill仅发生在显式 mutation receipt内。测试不接触真实Agent目录。

### U3 — Support Claude Code Plugins through project-scoped links and launch recipes

- Requirements: `R3`, `R4`, `R5`, `R6`, `R7`, `R9`
- Dependencies and accepted-design pointers: U1、U2；Technical Design D3、D4、Claude Plugin capability evidence与LaunchRecipe contract。
- Observable slice: 支持的 catalog Plugin可为当前项目的Claude Code安装/启用/禁用/卸载；内容link指向catalog stable path，通过AD打开该项目Claude会话时Plugin被原样加载。另一个项目/Agent不受影响；source update后新会话或native reload读取新内容。
- Affected modules and mutation:
  - 实现 `claude_ports/plugins.rs` 的 compatibility/install/toggle/uninstall：校验 native Plugin root与本期adapter contract，只在 workspace-scoped AD runtime root创建一个content symlink，写InstallationRecord，不调用marketplace install/cache。
  - 扩展 Claude LaunchPort 或增加通用 LaunchRecipe decorator：读取当前workspace的enabled Plugin InstallationRecords，按稳定顺序追加重复 `--plugin-dir <managed-link>`；检测重复native Plugin name并在安装preview前阻塞。
  - 更新 `commands/terminal.rs` 使effective context与managed Plugin launch decoration使用同一canonical workspace identity；不写user/global Claude settings。
  - 扩展 Plugin inventory，把ledger-backed项标为AD-managed，把settings/cache/手工目录等其他观察项标为external/Agent-observed且不提供AD清理。
  - Project UI复用U2的Installed / Available模型，显示Claude compatibility、enable/disable和简洁native reload提示。
- Entry / exit conditions:
  - Entry: U2 ledger与standard uninstall可表达symlink + enabled state；local/CI能够提供pinned Claude contract fixture。
  - Exit: AD launch recipe使用精确managed link；disable后flag消失但link/record可保留，uninstall后link/record/AD state清理；原始Plugin tree bytes不变且无AD cache copy。
- Focused verification:
  - Unit/integration tests覆盖manifest/no-manifest native layouts、invalid paths、duplicate Plugin names、multi-plugin flag order、disable/uninstall、source update、AD restart、external declaration与manual retarget。
  - 在临时project和temp `AD_HOME` 下记录 `LaunchRecipe.args`，并用已发现的Claude binary执行低风险contract smoke，证明`--plugin-dir`仍受支持；测试不进入交互账号流程。
  - Focused commands: `cargo test --manifest-path src-tauri/Cargo.toml claude_plugin`；`cargo test --manifest-path src-tauri/Cargo.toml terminal`；`pnpm vitest run tests/components/AgentCollectionPanel.test.tsx`。
- Recovery checkpoint: launch decoration是read-only projection；失败只阻止launch或该次mutation，不改用户Claude home。uninstall/disable receipt可按现有rollback恢复record、link和enabled state。

### U4 — Make Codex Plugin capability truthful and retire new copy/transform routes

- Requirements: `R3`, `R4`, `R5`, `R6`, `R8`, `R9`
- Dependencies and accepted-design pointers: U1、U2；Technical Design D4、migration step 4–6 与 Codex current contract evidence。
- Observable slice: catalog Plugin在Codex下清楚显示“当前Agent不支持”并不可安装；AD不会复制、生成manifest、裁剪或把Plugin内Skill单独安装。既有AD copied/converted Codex Plugin在升级后不被改写，可证明时仍能标准卸载；其他既有项标为“非 AD 托管”。
- Affected modules and mutation:
  - 在 `codex_plugins.rs` 的 managed catalog compatibility返回稳定 `unsupported_agent_capability`，阻断新的Resource Center install source；保留不涉及新复制的inspect/toggle/legacy remove所需最小路径。
  - 从 `collection_actions.rs`、conversion route和public commands中移除/禁用调用 `prepare_project_plugin_install`、staged marketplace/package copy、generated Codex manifest 的新操作入口；不在本单元大规模删除历史reader与rollback代码。
  - 更新 `plugin_conversion.rs` / conversion contracts：Plugin转换结果只报告unsupported residual，不发布PreparedProjectPluginInstall；Skill等其他conversion行为保持现状。
  - 为legacy Codex runtime建立只读分类：完整旧ownership/manifest时是`legacy_ad_managed`且只允许标准uninstall；证据不完整时external。
  - UI显示disabled Agent compatibility，不显示机制解释或“稍后自动转换”承诺。
- Entry / exit conditions:
  - Entry: U1能识别Plugin但不预设其Agent支持；保留legacy fixture与old receipts。
  - Exit: code search与测试证明所有新的catalog/conversion流程都不会执行Plugin tree copy/transform；旧安装升级零写入且可安全卸载。
- Focused verification:
  - Plugin-only、mixed Plugin/Skill、old copied package、corrupt ownership与external Codex cache fixtures；断言资源中心Codex preview无MutationPlan，source bytes和runtime bytes在只读inventory后不变。
  - 更新 conversion tests，证明Plugin residual不再生成stage/package install，而其他项目配置转换结果不回退。
  - Focused commands: `cargo test --manifest-path src-tauri/Cargo.toml codex_plugin`；`cargo test --manifest-path src-tauri/Cargo.toml plugin_conversion`；`cargo test --manifest-path src-tauri/Cargo.toml conversion`。
- Recovery checkpoint: 不删除旧staging/cache/manifest；若退役入口影响未识别legacy卸载，保留旧reader并只收紧new-install guard，直到fixture覆盖完整。

### U5 — Remove a catalog resource through durable multi-project uninstall progress

- Requirements: `R9`, `R10`, `R11`
- Dependencies and accepted-design pointers: U2–U4 standard uninstall稳定；Technical Design D5、removal state machine、progress event、concurrency/recovery rules。
- Observable slice: 资源中心移除一个已安装到多个项目/Agent的资源时，先显示项目数和Agent安装数；取消零变更。确认后展示整体和逐项进度，逐个调用standard uninstall。全部成功后resource变为suppressed且同source兄弟资源不变；部分失败时成功项保留成功、失败项可重试、resource仍可管理。
- Affected modules and mutation:
  - 新增 `resource_lifecycle.rs` / `resource_removal_execution.rs` 和 durable operation repository/journal；在 `execution_state.rs` 增加operation state目录和startup reconciliation。
  - 扩展 `resource_installations.rs` 的resource/workspace索引、standard uninstall entry point与legacy ownership adapter；corrupt/unknown evidence形成failed item，不授予删除权。
  - 新增Tauri preview/apply/retry/snapshot commands和typed `Channel<ResourceRemovalProgress>`；channel发送失败非fatal，sequence与operation snapshot提供重连恢复。
  - catalog resource lifecycle lease阻止removal期间新install/re-add/source switch；所有uninstall完成后CAS suppress。source remove调用同一coordinator并在所有resource完成后只删除catalog source entry。
  - 新增 `ResourceRemovalDialog.tsx` 与store state，覆盖impact confirm、pending/running/succeeded/failed、partial summary、retry和close/reopen恢复。
- Entry / exit conditions:
  - Entry: 每种supported/legacy managed install都有standard uninstall；InstallationRepository可完整枚举resource references。
  - Exit: cancel、zero-install、multi-project success、partial failure、retry、UI disconnect、crash/restart、stale preview、concurrent install与catalog CAS failure场景全部truthful；未到零active installation时resource绝不suppressed。
- Focused verification:
  - 故障注入覆盖每个item mutation前/后、receipt后/operation-state前、all-uninstalled/catalog-CAS前后；重启snapshot必须与ledger/ownership一致。
  - 两项目 × 两Agent fixture验证standard uninstall isolation；同source兄弟resource与source tree digest始终不变。
  - React tests覆盖impact counts、cancel、live progress、partial failure、retry、reopen和防重复确认。
  - Focused commands: `cargo test --manifest-path src-tauri/Cargo.toml resource_lifecycle`；`cargo test --manifest-path src-tauri/Cargo.toml resource_removal`；`pnpm vitest run tests/components/ResourceRemovalDialog.test.tsx`。
- Recovery checkpoint: operation只做per-item commit，不全局rollback；任何uncertain item进入failed/fail-closed。startup只reconcile，不自动继续destructive writes。Git/local source content永不进入remove mutation set。

### U6 — Consolidate the resource-center and project experience into a designed product surface

- Requirements: `R1`, `R2`, `R4`, `R5`, `R7`, `R8`, `R10`, `R12`
- Dependencies and accepted-design pointers: U1–U5 behavior完整；Technical Design D6、UX states/accessibility contract。
- Observable slice: 真实桌面产品中，资源中心是与项目并列、以资源为主的库；项目页聚焦当前Agent的已安装/可安装与一个下一步。正常状态不铺陈机制说明；阻塞只显示简洁原因和一个可执行下一步；所有空/加载/错误/部分失败/确认状态具有完整视觉层级。
- Affected modules and mutation:
  - 使用 `ad-frontend` 对 `App.tsx`、`ProjectSidebar.tsx`、`ProjectDetail.tsx`、`AgentCollectionPanel.tsx`、`ResourceCenter.tsx`、source detail/removal dialogs 和design tokens进行production-grade整合，而不是只删文案。
  - 将 `SettingsApp.tsx` / `SkillSources.tsx` 的主资源管理入口退役或改为指向资源中心；高级Settings不重复维护第二份source action surface。
  - 建立resource card/list的稳定信息层级：name/kind为主，source type/label和compatibility为次，诊断与path按需；同名候选可区分。
  - 审核 `zh.json` / `en.json`，删除Plugin copy/conversion、per-project revision和机制解释的旧承诺；保证所有action/status/aria strings双语完整。
  - 补齐键盘导航、focus、非颜色状态、窄窗口、长source/name截断和destructive dialog focus管理。
- Entry / exit conditions:
  - Entry: frontend schemas只消费U1–U5稳定backend codes和states；不允许用mock增加新产品语义。
  - Exit: Product/Design能在代表性桌面尺寸完成 source导入→辨识同名资源→项目选Agent安装→处理冲突→卸载→中心移除/partial retry 的任务走查，无机制术语主导页面。
- Focused verification:
  - Testing Library覆盖键盘、焦点、aria、窄容器、长文本、所有state variants与每页主操作；locale contract拒绝缺key/硬编码用户文案。
  - 运行production-like Tauri app，至少在紧凑和常规窗口、light/dark、中英文完成截图/任务证据；使用真实backend temp state，不以静态mock截图替代。
  - Focused commands: `pnpm vitest run tests/components/ResourceCenter.test.tsx tests/components/AgentCollectionPanel.test.tsx tests/components/ResourceRemovalDialog.test.tsx tests/i18n/locales.test.ts`；`pnpm build`。
- Recovery checkpoint: UI重构按可回退component boundary进行，不改变backend mutation semantics；如visual iteration引入behavior diff，先恢复最近通过的interaction tests再继续。

### U7 — Complete migration, security, end-to-end proof, canonical docs, and release gates

- Requirements: `R1`–`R12`
- Dependencies and accepted-design pointers: U1–U6；Technical Design全部invariants、migration/security/risks。
- Observable slice: 在隔离生产构建中完成 Local/Git source→resource→两项目/两Agent Skill安装、Claude Plugin安装、Codex Plugin unsupported、source live update、managed/external conflict、标准卸载、resource removal/re-add闭环；升级fixture保持旧Skill和Plugin状态安全。
- Affected modules and mutation:
  - 增加cross-unit Rust/Tauri integration fixtures，覆盖catalog migration、ledger、Agent adapter、source update和resource lifecycle；把真实Agent contract probe与fixture version记录进测试证据。
  - 更新 `docs/product-specs/project-agent-workspace.md`、`docs/design-docs/skill-source-checkout-links.md` 的当前权威指针/被新契约取代说明，以及README/当前用户文档中的source/Plugin语义。历史completed plans不改写为新事实。
  - 实现完成后若as-built偏离accepted design，先走设计revision，不在代码或计划中静默发明新contract。
  - 运行下方full verification、production bundle、隔离desktop task walkthrough、security/migration/review specialists；所有tree-changing fix使旧final evidence失效并重跑对应checks。
- Entry / exit conditions:
  - Entry: 每个单元focused evidence通过，所有迁移fixtures可从clean temp home重复构造。
  - Exit: requirement matrix全绿；无新copy/transform path；full CI/release build与production desktop experiential acceptance通过；final verification和review绑定同一exact tree。
- Focused verification: 下方完整 Verification contract。
- Recovery checkpoint: release/smoke始终使用temp resources与projects；不清理用户`.agents/`或其他pre-existing dirty files。若migration/recovery finding未解决，停止为needs-work，不通过删除legacy state绕过。

## Dependency order

```text
U1 resource catalog + library
  -> U2 Skill install ledger + project lifecycle
       -> U3 Claude Plugin direct-reference
       -> U4 Codex truthful unsupported + legacy boundary
            -> U5 multi-project resource removal
                 -> U6 product UX consolidation
                      -> U7 migration/security/release proof
```

U3 与 U4 在 U2 contract 冻结后可以并行实现；U5 必须等待所有可管理安装都有 standard uninstall。U6 可以提前探索布局，但不得以 mock 或前端分支定义 U1–U5 尚未接受的状态语义。

## Verification contract

### Repository baseline

执行前记录 `git status --short --branch`、HEAD和用户已有untracked/dirty文件。至少运行一次对应语言的baseline，用于区分既有失败：

```text
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
```

如果baseline已有失败，WorkReceipt记录exact command、test与tree identity；本任务不得隐藏、重置或顺手清理无关用户修改。

### Acceptance scenarios

| Scenario | Requirements | Required evidence |
|---|---|---|
| Resource library/source identity | R1–R3 | Git/local可导入；跨source同名并存且可辨识；same-source duplicate scan不发布 |
| Original content invariant | R3, R6 | install前后source tree digest/bytes相同；Agent content target是stable link；无copy/transform output |
| Project/Agent isolation | R4, R9 | Project A × selected Agent install/toggle/uninstall不改变A另一个Agent或Project B target/config/ledger |
| Agent compatibility | R5 | Claude Plugin supported fixture可安装；当前Codex Plugin显示unsupported且无plan；Skills两Agent可用 |
| Shared update | R6 | 两项目link bytes不变；Local edit/Git current switch后读取新内容；breaking Git update被阻塞 |
| Managed conflict | R7 | installed source/target可辨识；只提供standard uninstall；不会隐式安装另一source |
| External inventory | R8 | file/dir/symlink/Agent declaration显示“非 AD 托管”；零删除、备份、接管、source推断动作 |
| Standard uninstall | R9 | link、当前Agent附加状态和ledger/ownership一致清理；source和其他workspace不变 |
| Resource removal success/cancel | R10 | impact count与逐项progress准确；cancel零写入；全成功后才suppressed，siblings/source bytes不变 |
| Partial removal/recovery | R10 | 部分成功不回滚；失败项/整体truthful；retry完成；crash/UI disconnect后snapshot可重建 |
| Suppress/re-add lifecycle | R11 | refresh/update/restart/scan不恢复；center re-add后仅回到candidate，不自动安装项目 |
| Product experience | R12 | 真实desktop任务走查覆盖全部state；正常界面无symlink/adapter/ownership等机制说明堆叠 |
| Legacy migration | R3, R6, R8, R9 | v2 Skill catalog/ownership可读；old copied Plugin只读不改写，可证明时可uninstall；无startup mutation |

### Required automated checks

Focused filters以实现后的实际test name为准；若filter匹配0项，不能作为成功证据。最终至少运行：

```text
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
pnpm tauri build
git diff --check
```

Production bundle smoke使用隔离temp `AD_HOME`、Claude/Codex homes、Project A/B、Local source和local Git remote。不得用用户真实catalog、project或Agent settings作为验收fixture。

### Evidence roles and delivery consequence

- `V1 — Required`: 所有acceptance scenarios的Rust/TypeScript自动化证据。任一required scenario失败，结果为needs-work。
- `V2 — Required`: format、lint、typecheck、full tests、strict Clippy、frontend build与Tauri production build。没有预授权的低保真fallback。
- `V3 — Required`: `ad-security` final-tree review，覆盖source input/confinement、symlink race、target claim、ownership/ledger authority、lifecycle lease、journal recovery和external零写入。高置信 finding必须回到实现修复。
- `V4 — Required`: `ad-migrate` review，覆盖Skill catalog/ownership dual read、write migration、legacy Codex Plugin、rollback bytes、幂等与失败恢复。不得通过删除旧state解决finding。
- `V5 — Required experiential`: Product/Design在真实production-like Tauri app完成U6任务走查和light/dark、中英文、代表性窗口视觉评审。组件测试、Story/mock或浏览器截图不能替代；缺失时工程可继续，但不得宣称merge-ready或R12完成。
- `V6 — Preferred`: 已安装Claude/Codex本地binary的隔离contract smoke。若binary缺失或登录/交互限制阻塞，primary为pinned official contract fixture + LaunchRecipe/inventory integration；fidelity loss是未证明该机器上的实际Agent版本，authority为本计划，结果必须记录residual且Plugin adapter默认fail closed。
- `V7 — Required`: `ad-verify` 汇总requirement/unit/evidence；`ad-code-review`在同一exact final tree检查design→plan→diff、correctness、security、migration和evidence sufficiency。

### Specialists and proportional skips

- `ad-frontend`: required during U6 because R12 is an experiential product-design requirement, not text cleanup。
- `ad-security`: required after U5 transaction/authority implementation and on final tree。
- `ad-migrate`: required after U4/U5 schemas and legacy behavior stabilize。
- `ad-verify` and `ad-code-review`: required on final exact tree。
- `ad-test-browser`: not primary；产品运行在Tauri且关键状态依赖native filesystem/IPC。React interaction由Vitest覆盖，真实体验由production desktop walkthrough覆盖。
- `ad-performance`: skip unless representative large-source / large-installation fixtures show measurable regression；当前contract没有独立latency SLO，不允许无基线调优。
- `ad-doc-review`: skip at plan stage；设计没有不可逆cleanup或silent cutover。若实现把migration改为eager destructive或引入新的外部Agent protocol，应先回到design并触发review。

## Risks and recovery

| Risk | Recovery contract |
|---|---|
| 新catalog writer使旧state不可读 | dual reader先落地；migration journal保存exact before bytes；不删除旧file/root |
| scanner误把Plugin内部Skill变成独立资源 | descriptor-root exclusion + fixture；scan失败不发布catalog/current |
| link写入正确但ledger失败 | ownership/ledger纳入同一ExecutionEngine commit；失败补偿link/config |
| Git update使installed Plugin/Skill失效 | join every installation + adapter compatibility before current switch；old generation保持active |
| Local source外部变化无法rollback | 如实health；ownership仍允许standard uninstall；不伪造内容rollback |
| external因同名或相同path被接管 | 只有ledger+ownership授予authority；任何缺失/漂移都fail closed |
| resource removal部分完成 | per-item receipts不回滚成功项；resource保持managed；snapshot/retry处理剩余项 |
| lifecycle UI断连后误报 | event仅observation；durable operation snapshot是权威；重连按sequence刷新 |
| Agent升级破坏direct Plugin contract | install/apply按adapter contract重验原生descriptor；出现version-gated contract时再加入版本probe并fail closed |
| UI重构掩盖诊断或堆满文案 | stable code→progressive disclosure；真实desktop task/visual review是required evidence |
| 用户dirty tree被覆盖 | 每单元status检查和specific-file edits；不清理`.agents/`或无关文件；禁止destructive reset |

如果实现失败，停止在最近完成单元边界，保留可诊断diff和focused red/green evidence。恢复只能使用journal/receipt、temp fixture重建或具体可逆patch；不得删除用户state、重置整个worktree或修改真实source来制造通过。

## Definition of done

- `TD-skill-plugin-resource-management/v1` 的schema、adapter、state machine、migration和security contracts均有实现与证据，无未授权产品delta。
- 资源中心是AD安装Skill/Plugin的唯一候选池，Git/local、同名跨source、source type辨识和same-source duplicate拦截完整。
- 所有新安装保持原始内容不变并通过stable symlink读取；Skill支持Claude/Codex，Claude Plugin通过project launch direct-reference支持，当前Codex Plugin明确unsupported且无copy/transform fallback。
- Project × Agent isolation、managed conflict先卸载、external只读、standard uninstall与source update传播均通过自动化验收。
- resource removal具有影响确认、durable逐项进度、partial failure/retry/crash recovery，且只在零active installation后suppressed；re-add与project install是两个动作。
- v2 Skill状态与legacy Codex Plugin升级安全；startup/inventory零静默mutation；旧state/content未被清理或重写。
- 顶级资源库和项目Installed/Available界面通过真实desktop Product/Design验收，正常流程不由机制说明主导。
- Required automated、security、migration、verification、review和production build evidence全部绑定同一exact tree；任何fallback/residual按本计划的delivery consequence处理。
- Commit policy保持`none`；未获得新的明确授权前，不创建commit、不push、不创建PR、不merge、不部署。

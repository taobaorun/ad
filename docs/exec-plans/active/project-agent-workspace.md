# 收敛 Project Agent Workspace，完成项目级 Skills、Plugins 与配置转化管理

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

本计划的产品与实施细节真理源是 `docs/plans/2026-08-01-001-feat-project-agent-workspace-plan.md`。本 ExecPlan 负责审批基线、执行顺序、验证和恢复；若两者出现冲突，执行前必须先同步修订两份评审文档。

## 目的 / 全局视角

完成后，AD 的主产品不再是散落的 Claude settings/profile 页面，而是“一个项目的 Coding Agent 配置工作区”。用户选择项目和 Claude Code/Codex installation 后，可以在同一处看到最终生效的 Settings、Skills、Plugins，理解来源与覆盖关系，完成真实支持的安装、启停、更新、移除，并将 Claude Code 的有效项目环境安全转化为隔离的 Codex Project Runtime。

用户可以通过三类可观察结果判断功能真的完成：项目 A 的操作不会改变项目 B 或未确认的用户资源；所有写入都有预览、回执和受保护回滚；不支持或必须外部完成的动作会明确显示，绝不伪装成功。

## 确认状态

- [x] **用户已确认** — 评审 HTML：`docs/exec-plans/active/project-agent-workspace.html`
- [x] HTML 基线已冻结并开始执行（2026-08-01 10:43+08:00）

批准后 HTML 不再修改；执行进展、发现、决策和结果只更新本 MD。

## 假设

- Claude Code 与 Codex 是本轮唯一验收 Agent；未来 Agent adapter 不在本 ExecPlan 中实现。
- “项目级隔离”要求项目动作默认不修改用户层；继承用户资源在项目页是可观察输入，只有存在真实项目 override 时才可操作。
- Skill source catalog 仍是全局 acquisition 入口，但项目安装固定到不可变 artifact revision；刷新 source 不自动升级任何项目。
- 只移除 AD 拥有的项目 link、manifest declaration 或 runtime package；用户继承和外部资源永不被项目 remove 删除。
- Claude Plugin install 与 Codex User Plugin marketplace acquisition 继续以 external/degraded 呈现，不作为假实现补齐。
- 本轮只允许第一方 UI 完成显式风险确认，不新增 CLI、MCP、deep-link或Agent自动批准入口；这不声称抵抗已被攻陷的renderer或同一macOS用户自动化。
- legacy 数据必须兼容读取、显式 preview、幂等迁移；新状态和回执未持久化前不清理旧状态。
- Settings 中的credential、token、MCP env与未知疑似敏感值默认遮罩且不自动从user scope复制到project scope；AD不因此扩展为密钥管理器。
- 项目级隔离只约束AD的配置读取、写入、归属和转换落点，不等同于第三方Skill/Plugin/hook/MCP的运行时沙箱或发布者认证。
- 只有已验证Agent版本/schema/location set可以得到complete coverage；未知future版本或新层级必须降为partial/failed。
- 真机 macOS release workflow 仍需人工执行并记录证据；此前文档最多声明达到实际完成的 evidence level。

## 影响范围

实施会跨越以下边界；精确文件和测试映射见统一实施计划的 U1–U10。

- **Rust domain / IPC contract：** `src-tauri/src/agents/capabilities.rs`、`operations.rs`、`types.rs`、`commands/agents.rs`、`plan_store.rs`、`execution.rs`、`execution_fs.rs`与Tauri startup `src-tauri/src/lib.rs`。
- **Agent adapters：** `src-tauri/src/agents/claude_ports/`、`codex_ports/`、`codex_skills.rs`、`codex_plugins.rs`。
- **Project Codex ownership：** `project_codex_manifest.rs`、`project_codex_runtime.rs`、`project_codex_config.rs`。
- **Skill acquisition / migration：** `commands/skills.rs`、`fs/paths.rs`、`fs/git.rs`、`models.rs`，以及新的 migration integration tests。
- **Conversion：** `conversion.rs`、`conversion_route.rs`、`plugin_conversion.rs` 与现有 conversion integration suites。
- **Frontend workspace：** `ProjectDetail.tsx`、`AgentSettingsEditor.tsx`、`AgentCollectionPanel.tsx`、`AgentPlanDialog.tsx`、Conversion components、`HistoryPanel.tsx`，以及必要的新子组件。
- **Shared TypeScript boundary：** `src/lib/agentTypes.ts`、`agentCapabilities.ts`、`agentResourceViews.ts`、`tauri.ts`、`projectCodexRuntime.ts`。
- **Legacy cleanup：** `ProjectSkills.tsx`、`store/skills.ts`、`skillTypes.ts`、旧 Tauri IPC registration 和 project state models。
- **i18n / docs：** zh/en locales、README、package metadata、PRODUCT_SENSE、product/design specs 及索引。
- **主要新增测试：** `src-tauri/tests/project_agent_workspace.rs`、`src-tauri/tests/skill_catalog_migration.rs`、`tests/components/ProjectAgentWorkspace.test.tsx`。

已有 active ExecPlan `docs/exec-plans/active/bundle-slim-codemirror.md` 主要修改 editor bundle/lazy loading。执行前要检查两者工作区状态；若它同时触达 `AgentSettingsEditor.tsx` 或 build配置，先完成/重基线该计划，再开始本计划对应文件。

## 关键决策

1. **后端拥有 effective/provenance 语义。** Raw snapshot 留在 adapter 内部；项目 UI 只消费严格 typed inventory envelope 与 resource view。
2. **“全部”必须有 coverage。** Inventory category 输出 complete/partial/failed 和 item diagnostics；partial 不能显示完整清单声明。
3. **项目页 fail closed。** Inherited user resource 默认 inspect-only；Codex runtime 未准备时禁止 project Plugin mutation回退到base config。
4. **Skill source 与项目 revision 解耦。** Source checkout可刷新，项目 link只能指向immutable content-addressed artifact。
5. **生命周期是 item-level policy。** Add/install、toggle、sync/update、remove/reset override、external steps 由每项 ownership/context决定。
6. **Apply 绑定 workspace。** Backend claim校验expected canonical `AgentContext`、source/target digest、expiry、replay和plan-bound acknowledgement。
7. **执行回执与domain report分层。** Receipt只记录attempted mutation；Workspace/Conversion report记录external、unsupported、conflict、no-change与residual。
8. **Conversion 转的是有效项目环境。** Claude user/shared/local/project层都作为只读输入，结果只写Codex project runtime/overlay。
9. **显式 reconciliation 代替自动迁移。** Legacy source/project state先盘点、预览、应用并留回执，之后才退役旧写路径。
10. **外部 Agent 自动化延后。** 未来 inspect/preview可复用同一workspace；human-only风险批准不可由被管理Agent自批。
11. **AD-owned state不是伪Agent资源。** MutationPlan使用sealed target enum区分Agent resource与allowlisted AD state；frontend永远不能提交catalog/artifact/archive物理路径。
12. **Target confinement是fd-relative。** Project、Runtime和`~/.ad`管理根都持有受信directory descriptor，后续遍历与rename使用macOS no-follow relative操作，抵抗验证后的ancestor race。
13. **Crash与并发是正常失败模型。** 写盘前sync operation journal及父目录；physical target与startup recovery使用跨进程锁，rollback必须生成fresh inverse plan。
14. **Source输入默认不可信。** Git用固定、无用户输入的login-shell bootstrap恢复可信可执行文件和最小认证环境，实际调用使用结构化argv；network/time/disk/tree预算与artifact digest都受控。
15. **敏感配置默认不跨scope复制。** Settings inventory/diff/error/history遮罩敏感值，journal/receipt不存正文，backup仅当前用户可读。
16. **完整性绑定Agent兼容合同。** Unknown future version/schema/layer/location会降级coverage；只扫描完已知目录不能宣称“全部”。

## 进展

- [x] (2026-08-01 10:43+08:00 开始，14:33+08:00 完成) M0：冻结 workspace contract 并加固ExecutionEngine（验证标准：Rust/TS contract、AD-state targets、fd-relative confinement、跨进程锁、synced crash recovery、legacy receipt与rollback-plan tests通过）
  - [x] (2026-08-01 11:06+08:00) U1：Rust/TypeScript workspace、resource/declaration/target identity、coverage/provenance/ownership/item action、sanitized plan与domain report契约完成；25个前端契约测试、Rust parity/operations/capability测试及typecheck通过。
  - [x] (2026-08-01 14:33+08:00) U10：sealed target接入执行、fd-relative confinement、跨进程锁、journal/startup recovery、versioned receipt、ownership与inverse rollback plan完成。
    - [x] (2026-08-01 11:22+08:00) 跨进程target lock与durable journal基础：真实子进程争用3/3、lock 2/2、journal 3/3、execution 14/14、PlanStore 11/11及严格Clippy通过；receipt file/parent sync后才提交journal，补偿和repair-required路径有持久状态。
    - [x] (2026-08-01 12:20+08:00) fd-relative/no-follow confinement、unsafe root拒绝与ancestor-swap测试完成；Agent target与AD transaction artifacts都在held descriptors下执行。
      - [x] (2026-08-01 11:37+08:00) Project/user/runtime regular-file target已使用held parent fd执行observe、temp create、rename、unlink和directory sync；project/user ancestor symlink、active swap、0777 AD root及symlinked backup root sentinel测试通过。
      - [x] (2026-08-01 12:01+08:00) Skill/Plugin symlink与directory storage已统一使用held parent fd执行observe、digest、backup、temp publish、remove、compensation、rollback与receipt observe；directory digest保持旧合同，symlink/directory active-swap sentinel、ExecutionEngine补偿/rollback与Rust全量267/267测试通过。
      - [x] (2026-08-01 12:20+08:00) AD lock/journal/backup/manifest/history/cleanup已共享一次操作持有的state directory descriptors；整个`.ad`或journal child被swap成outside symlink时，事务artifact仍写入原目录且outside为空。初始journal和receipt使用no-replace发布，lock拒绝hard link；Rust全量272/272与严格Clippy通过。
    - [x] (2026-08-01 12:34+08:00) startup recovery/global recovery lock、ExecutionEngine mutation gate与process-kill边界矩阵完成：启动恢复持有跨进程exclusive lock，apply/rollback持有shared lease；prepared、prepared+backup、applying无receipt、applying+complete receipt及另一进程持锁边界由真实abort子进程覆盖。journal v2兼容读取v1，恢复测试6/6、真实进程测试2/2、Rust全量279 passed/4 ignored与严格Clippy通过；legacy direct-write命令仍按U6迁移退役。
    - [x] (2026-08-01 14:33+08:00) versioned receipt/history decoder、ownership record与fresh inverse rollback plan完成。
      - [x] (2026-08-01 12:46+08:00) OperationReceipt v2与fd-confined per-file History decoder完成：新回执记录operation/context/rollback eligibility/createdAt，legacy无版本回执继续显示但rollback unavailable；损坏、identity不符和future schema成为单项diagnostic，不影响同目录正常记录。Rust全量283 passed/4 ignored、前端142/142、typecheck/lint与严格Clippy通过。
      - [x] (2026-08-01 14:33+08:00) Project Skill/Plugin ownership record、receipt ownership evidence、fresh preview-confirm-apply inverse rollback与startup replay完成；apply/rollback claim绑定preview时的canonical context与risk fingerprint，project root绑定device/inode并拒绝与AD/Claude/Codex root重叠。项目History中的user receipt保持可见但inspect-only；post-publish receipt错误、partial create、并发startup与Profiles关闭竞态均有回归测试。`cargo test --all-targets --all-features`、严格Clippy、前端143/143、format/lint/typecheck/build及`pnpm tauri build`通过，生成AD.app与DMG；本地多角色review完成，外部Claude peer因CLI未登录未产出结果。
- [x] (2026-08-01 14:35+08:00 开始，15:35+08:00 完成) M1：实现 effective inventory 与分层 Settings（验证标准：Claude/Codex provenance、coverage、canonical context测试通过）
  - [x] (2026-08-01 14:50+08:00) Canonical WorkspaceDescriptor 后端签发入口完成：项目真实路径、base/effective installation 与 prepared Codex runtime revision 统一生成 workspace identity，ownership workspace key 改为复用同一签名。路径别名、同名不同项目、base/runtime 收敛、unknown/mismatched installation 及 Rust/TypeScript IPC 边界测试通过。
  - [x] (2026-08-01 14:56+08:00) ProjectWorkspaceInventory、InventoryRevision、AdapterDiscoveryContract、Settings effective/layer/field/editable-target/sensitivity 契约完成 Rust/Zod 同构；严格 schema 测试证明 unknown 字段被拒绝、未验证 Agent 版本保持 partial、敏感值边界只允许遮罩形态。
  - [x] (2026-08-01 15:25+08:00) Backend effective inventory 与分层 Settings 读取/编辑服务完成：Claude user/shared/local 与 Codex base/native-project/runtime-manifest 的 field winner、provenance、health、coverage 由后端统一解析；generated Codex config 不作为声明。敏感值在 IPC 前遮罩，私有 digest 仍使 secret rotation 改变 inventory revision；project edit 只提交项目语义层并由后端恢复遮罩字段、保留未知字段。Claude/Codex fixture、legacy project Settings IPC 限界、ownership recovery 与严格 Clippy 通过。
  - [x] (2026-08-01 15:35+08:00) Project Settings/Skills/Plugins 首屏切换到 typed inventory：effective 与继承层只读、backend editable target 才可 preview/apply/rollback，dirty draft 阻断项目/Agent切换；集合按 effective state、provenance、health、management 与 partial/failed coverage 展示，M3前不再暴露基于raw snapshot猜测的toggle。所有mutation使用inventory签发的effective context，stale response不会覆盖新workspace。前端143/143、typecheck/lint/build，Rust全量303 passed/4 ignored、全部integration tests与严格Clippy通过。
- [x] (2026-08-01 15:36+08:00 开始，16:22+08:00 完成) M2：引入 immutable Skill artifact 和安全 source acquisition（验证标准：更新项目A不改变B，migration fixtures幂等）
  - [x] (2026-08-01 15:43+08:00) Git执行边界不再拼接用户输入：固定`/bin/zsh -lc`只解析绝对Git路径和allowlisted GUI认证环境，后续URL/ref全部先校验并使用结构化argv；禁用submodule递归与LFS smudge，限制输出、总时长和无进展时长，超时终止整个process group。恶意URL/ref、credential URL、GUI最小PATH和timeout测试4/4，legacy Skill测试11/11与严格Clippy通过。
  - [x] (2026-08-01 16:01+08:00) Immutable Skill artifact tree与staging/publish服务完成：规范manifest覆盖相对path、节点类型、内容digest和执行位；排除Git/cache，限制depth/count/file/total bytes与磁盘保留量，拒绝hardlink、special file、absolute/逃逸/循环symlink及copy期间drift。artifact按tree digest只读发布，使用macOS no-replace rename；复用前完整复验manifest/tree，collision或篡改不覆盖。activation impact记录instruction、hook/MCP/command、script/binary与执行位；artifact/staging测试8/8与严格Clippy通过。
  - [x] (2026-08-01 16:12+08:00) Source Catalog改为backend签发`skill-source:<uuid>`并通过独立Preview→确认→Apply事务管理；catalog/artifact/backup/journal/receipt均不暴露物理target，source drift、catalog drift、过期/重放/risk变化fail closed。跨进程catalog lock、staging flock lease、atomic catalog publish、崩溃后receipt补写/补偿和corrupt journal mutation gate完成；删除Source只删catalog entry，不物理删除artifact或legacy checkout。定向execution/recovery/migration测试10/10、Rust全量319 passed/1 ignored及全部integration tests、严格Clippy通过。
  - [x] (2026-08-01 16:22+08:00) Settings的Skill Source入口切换到typed catalog与独立risk dialog：先检查完整tree和activation impact，再明确确认发布；刷新明确提示不会升级任何项目，移除明确提示保留project pin与旧artifact，取消计划立即释放staging。旧artifact ref在catalog刷新/移除后仍完整复验，两个独立revision pin互不变化；legacy registry/project/link盘点重复运行字节不变并阻断unsafe ID、path alias与external ownership。前端146/146、typecheck/lint/build通过。
- [x] (2026-08-01 16:23+08:00 开始，16:56+08:00 完成) M3：补齐 Skills/Plugins item lifecycle planners（验证标准：install/toggle/update/remove的支持与退化矩阵通过）
  - [x] Backend-owned item action pipeline 完成：renderer只提交workspace key、inventory revision、resource key和action；后端重新盘点并自行解析catalog artifact、ownership record和project override，拒绝stale inventory、未提供动作、risk未确认与外部/所有权不明目标。plan/receipt保留workspace与action identity，复用ExecutionEngine补偿和guarded rollback。
  - [x] Skill生命周期完成：catalog资源按项目显示Install；AD-owned project link显示Enable/Disable、Update与Remove；source刷新后A/B继续pin旧artifact，只有显式Update的项目retarget。Claude旧`skill-library`路径启发式不再作为新artifact所有权，replace/remove必须通过legacy受控路径或精确ownership evidence。
  - [x] Plugin项目override矩阵完成：Claude/Codex支持安全Enable/Disable和Reset override；仅继承user/shared声明、未准备Codex Runtime、外部更新或marketplace acquisition均明确Unavailable/External，不回退修改user/base配置。shared项目声明不会伪装成可移除的local override。
  - [x] 第一方Project资源UI已提前接入backend actions与独立Preview→确认→Apply入口；不可用动作保持禁用并展示原因，Plugin Remove明确显示为“重置项目覆盖”。A/B Skill install/update/remove、Plugin local override/user/shared/peer isolation、stale request、external ownership、显式确认、receipt rollback测试5/5；Rust全量324 passed/1 ignored及全部integration suites、严格Clippy，前端147/147、format/lint/typecheck/build通过。
- [x] (2026-08-01 16:58+08:00 开始，17:18+08:00 完成) M4：完成统一 Project Agent Workspace UI（验证标准：所有真实动作可从ProjectDetail完成，draft/close行为、partial/stale/empty与可访问状态准确）
  - [x] ProjectDetail收敛为Settings、Skills & Plugins、History三个ARIA tab；三个surface保持挂载，tab切换不再销毁Settings草稿或进行中的Apply。方向键/Home/End导航、tabpanel关系、不可用状态与项目History入口均有明确语义；ProjectDetail拆分后相关文件保持500行内。
  - [x] Project collection Apply接入按workspace key跟踪的detached operation：UI离开/卸载不取消后端命令，重开原workspace恢复Applying/complete/partial/failed状态，单workspace重复Apply复用同一pending promise；完成事件刷新inventory与History，持久事实仍以后端receipt为准。report workspace不匹配时fail closed，external/unsupported/conflict/no-change不再伪装为“已应用”。
  - [x] 通用plan确认按风险、activation impact、批准/权限、target、技术细节排序；危险计划使用文本与图标而非仅颜色表达。Dialog补齐label/description关系、初始安全焦点、Tab trap、Escape与focus return，busy时关闭入口真实禁用。资源页明确区分loading、inspection error、workspace empty、category empty、filter no-match及partial/failed coverage。
  - [x] History对Claude与Codex统一使用project filter；受保护回滚使用backend返回且已判定eligible的receipt context，不再用缺少project的base active context。dirty Settings在workspace refresh时保留草稿，只更新其inventory baseline。React定向30/30、前端全量154/154、format/lint/typecheck/build、Rust全量324 passed/1 ignored与全部integration suites、严格Clippy通过；`pnpm tauri build`生成17:18的AD.app与DMG，`git diff --check`通过。
- [x] (2026-08-01 17:19+08:00 开始，18:17+08:00 完成) M5：让 Conversion 复用 workspace inventory/planners/result（验证标准：有效继承输入、resolver重新preview、safe subset、residual、补偿和rollback通过）
  - [x] Project Conversion先校验typed workspace inventory，再把Claude user/shared/local Settings与user/project Skills作为只读有效输入；Settings按层级深合并，继承敏感字段不跨scope复制，所有写入严格落到Codex project runtime/overlay，source user/shared/local bytes保持不变。
  - [x] Conversion plan与sealed `ConversionReport`一起存储；required residual阻止full Apply，只有显式safe-subset重新preview才签发可应用计划。Apply返回domain report并分层附带receipt，complete/compensated/partial、逐项final state与residual不再由UI从receipt猜测。
  - [x] Resolution、Skill确认与safe subset都触发后端重新preview和新risk fingerprint；重复project conversion通过runtime manifest的`projectSettingsKeys`判定已物化overlay并收敛为no mutation，rollback恢复target并移除本次创建的project Skill link。
  - [x] Conversion UI接入workspace外置detached tracker，关闭/重开继续显示同一Apply且不会重复invoke；tracker按workspace+plan去重、拒绝同workspace不同plan并发，completed结果有界保留。Conversion主组件拆分后488行，safe subset、补偿/partial report、detached reopen与schema边界均有回归测试。
  - [x] `ce-simplify-code`三路审查完成：复用3项、质量3项、效率2项落实；保留完整inventory预检的fail-closed coverage语义，并保留report schema转发层以打断运行时循环。前端157/157、format/lint/typecheck/build，Rust326 passed/1 ignored、全部integration suites与严格Clippy通过；`pnpm tauri build`于18:17生成AD.app与DMG，`git diff --check`及frozen HTML零差异通过。
- [ ] M6：迁移并删除 legacy project写路径（验证标准：旧用户状态可恢复，全仓库无legacy consumer）
- [ ] M7：完成自动化、真机release验证和文档状态收敛（验证标准：所有门禁通过且evidence matrix有真实证据）
- [ ] 完成结果回顾并将 MD + frozen HTML 一起移到 `docs/exec-plans/completed/`

## 意外发现

- 发现：当前 `AgentCollectionPanel` 已有 toggle 的 Preview → Apply → receipt → rollback，但没有 install/update/remove UI，尽管 install IPC 已存在。
  证据：`src/components/AgentCollectionPanel.tsx` 与 `src/lib/tauri.ts::previewAgentCollectionInstall`。
- 发现：Claude 项目资源页可能把 global Skill toggle写到用户目录；Codex Project Runtime未准备时也可能回退到base context管理user Plugin。
  证据：`claude_ports/skills.rs`、`codex_plugins.rs` 与 `ProjectDetail.tsx` 的context选择路径。
- 发现：legacy `update_skill_source` 对共享checkout原地Git pull，所有指向它的project symlink会在无preview/receipt时改变。
  证据：`commands/skills.rs::update_skill_source` 与AD-managed symlink安装路径。
- 发现：Plugin provenance在列表阶段被压平；Claude只保留winner，Codex generated config丢失base/overlay/ownership来源。
  证据：`claude_ports/plugins.rs`、`codex_plugins.rs`、Project Codex manifest/runtime实现。
- 发现：项目 conversion 已覆盖 Settings/Skills/Plugins并有source read-only保护，但部分 resolution没有真实UI动作，receipt也没有保留全部residual。
  证据：`conversion_route.rs`、`AgentConversionArtifacts.tsx` 与 `OperationReceipt` contract。
- 发现：仓库没有 `docs/solutions/` 或 critical-patterns 经验库，本次迁移模式必须由实际执行结果新建证据。
  证据：只读仓库搜索结果。
- 发现：现有Git helper把URL/ref插入shell string，legacy source id只检查非空后参与path join/remove。
  证据：`src-tauri/src/fs/git.rs`与`commands/skills.rs` source CRUD。
- 发现：`is_ad_managed_symlink`的containment heuristic不能证明link由AD创建，且project root canonicalization不能阻止`.claude`/`.agents`等ancestor symlink导向项目外。
  证据：symlink ownership helper与adapter target construction路径。
- 发现：内存补偿无法覆盖进程在receipt持久化前终止，两个不同plan也可能同时通过旧digest检查产生lost update。
  证据：现有ExecutionEngine/PlanStore只有单次执行内补偿与plan replay保护。
- 发现：当前ExecutionEngine的path-based写入即使在锁内重验，也不能抵抗另一个同用户进程在验证后替换ancestor；进程内锁也不能协调多个AD实例。
  证据：现有`execution_fs.rs`使用普通path调用，计划审查要求fd-relative operation和cross-process lock。
- 发现：旧OperationReceipt、GUI启动的Git认证环境、Settings敏感值和unknown future Agent schema均缺少显式兼容/降级合同。
  证据：当前History decoder、`fs/git.rs` login-shell说明、Settings/conversion数据流与adapter discovery逻辑。
- 发现：U1契约直接追加到既有`operations.rs`、`capabilities.rs`和`agentTypes.ts`会使三个核心文件越过项目约定的单文件规模，继续扩展会再次形成难以控制的边界。
  证据：首次实现后文件分别达到713、534和707行；集成审查后拆为`workspace_contracts.rs`、`resource_inventory.rs`、`agentWorkspaceTypes.ts`、`agentResourceInventoryTypes.ts`与`agentOperationReports.ts`，原文件回落到396、362和490行。
- 发现：首次journal接线遗漏了“执行失败且补偿成功，但失败回执持久化失败”分支，该分支会把journal留在`applying`；同时prepared记录没有固定计划使用的receipt id，不利于启动恢复精确关联History。
  证据：主代理集成审查新增`failure_receipt_persistence_still_records_compensation`回归测试，并为journal加入`plannedReceiptId`；修复后ExecutionEngine聚焦测试14/14通过。
- 发现：当前adapter target resolution会跟随项目`.claude`或用户`~/.codex` symlink；仅依赖canonical target与atomic rename时，ExecutionEngine会成功改写symlink后的outside配置。
  证据：proof-first fixture中`project_settings_ancestor_symlink_cannot_modify_outside_sentinel`与`user_agent_root_symlink_cannot_modify_outside_sentinel`在修复前均返回Complete并改变outside bytes；fd-relative接入后18个ExecutionEngine测试全部通过。
- 发现：把symlink/directory接入统一confinement后，若锁内read-only dependency与mutation共用同一root授权，Codex Project Runtime读取base config所需的跨installation观察会迫使写权限一起放宽；两者必须是不同能力。
  证据：恢复严格installation identity检查后，`project_plugin_config_failure_restores_replaced_directories_before_activation`在base-config read-set处正确暴露边界冲突；拆分只读dependency root后通过，而mutation仍保持identity绑定。
- 发现：首次统一`ConfinedTarget`会在执行准备阶段创建缺失的target parent；如果后续precondition或backup失败，项目会留下没有receipt的空目录。
  证据：新增`resolving_a_missing_target_does_not_create_parent_directories`；实现改为持有最近存在ancestor fd与pending components，只在Apply写入时逐级创建并sync后测试通过。
- 发现：分别加固journal、backup和history文件仍不能关闭AD state race；如果每一步重新从path打开，整个`.ad`或任一child在步骤之间被替换后，lock、journal、backup与receipt会落入不同物理根。
  证据：`execution_state_root_swap_cannot_redirect_transaction_artifacts`在`Backup(0)`前替换整个`.ad`；只有让ExecutionEngine从lock获取到receipt提交共享同一组directory descriptors后，journal/manifest/receipt才全部留在原根且outside为空。
- 发现：existing lockfile若是指向外部current-user文件的hard link，`O_NOFOLLOW`不能阻止metadata写入破坏外部文件；initial journal/receipt若使用replace rename也会在极小identity碰撞时覆盖既有证据。
  证据：新增hard-link sentinel与same-operation双writer测试；lock要求`st_nlink == 1`，journal/receipt通过synced temp + no-replace link发布。
- 发现：`prepared` journal持久化完成与backup目录创建之间存在合法崩溃窗口；此时backup不存在代表尚无target写入，不能升级为人工修复。
  证据：新增journal-only单元fixture与`prepared_empty`真实abort子进程边界；恢复把backup缺失视为幂等清理并转为`compensated`。
- 发现：v1 journal只记录目标身份，不包含足以证明applying阶段已写入哪些target的逐项证据；applying且无有效receipt时自动覆盖目标可能破坏外部修改。
  证据：真实abort子进程在target已变化但receipt缺失时，恢复保留target并转为`repair_required`，普通ExecutionEngine mutation被gate阻止。
- 发现：引入`repaired`终态后继续写schema v1会破坏旧合同语义；但仓库已经可能存在v1 terminal/in-flight journal，不能简单拒绝旧版本。
  证据：新写journal提升为v2，reader接受v1-v2；v1 journal发生恢复迁移时原子升级为v2，future/corrupt journal保持fail-closed。
- 发现：旧History loader把单个损坏或future receipt静默跳过，既无法向用户说明历史不完整，也会让旧receipt继续借当前rollback decoder尝试执行。
  证据：History读取已迁入held state descriptor下的逐文件version gate；legacy receipt被规范化为schema v1并标记`legacy_receipt`不可回滚，corrupt/future fixture与正常legacy fixture同时返回。
- 发现：只用`planId`执行mutating IPC时，renderer可以在用户切换project或preview失效后继续消费旧plan；仅靠plan内部context不能证明调用方仍在确认的workspace。
  证据：apply与rollback command现要求提交preview时的canonical `AgentContext`和risk fingerprint，PlanStore在claim消费前逐项匹配；stale workspace/risk返回retryable `resource_changed`且不执行写入。
- 发现：canonical project path既可能直接别名AD/Claude/Codex配置根，也可能在preview后由ancestor rename换成另一物理目录；字符串containment不能证明执行仍属于同一项目。
  证据：项目添加和ExecutionEngine preview拒绝重叠root；PlanExecutionIntent记录project root的device/inode，held no-follow root在observe与apply时重验，alias-user-home与ancestor-swap fixtures通过。
- 发现：no-replace receipt publish成功后，后续fault或directory fsync失败会让调用方看到错误，但磁盘上已经存在可用于startup reconciliation的commit证据；直接补偿会产生“成功receipt + 已回滚target”的矛盾状态。
  证据：执行层在persist错误后读取并比对同一receipt bytes；已发布时重试history directory sync并继续ownership/journal commit，无法判定时保留journal/backups并返回non-retryable partial failure。
- 发现：另一个AD实例持有startup recovery lease是合法并发状态，不应导致第二个实例启动失败；同时Profiles rollback preview在对话框关闭后仍可能继续apply。
  证据：startup仅对`operation_recovery`阶段的retryable lock conflict延后恢复，其余错误仍fail closed；Profiles以request token和open ref使关闭后的异步结果失效，定向测试和全量前端测试通过。
- 发现：项目History按installation过滤时会同时返回同installation的user-scope receipt；若直接沿用receipt自身rollback eligibility，项目页能触发用户层回滚。
  证据：project filter下context不匹配的receipt仍用于理解继承历史，但统一覆盖为`workspace_mismatch` inspect-only；History组件测试和Rust decoder测试覆盖该边界。
- 发现：ProjectDetail通常会把prepared Codex runtime context传给资源组件，但盘点期间runtime状态变化时，renderer持有的context仍可能落后于后端签发的effective installation。
  证据：Settings editor改为从`ProjectWorkspaceInventory.workspace`构造mutation context，apply使用plan自身context，rollback优先使用receipt context；定向测试覆盖inventory context与组件输入context不同的情况。
- 发现：Claude旧`is_ad_managed_symlink`把“link目标位于mutable skill-library”当成所有权；新immutable artifact正确安装后会被该启发式拒绝更新，而任意同目录link也不能因此获得所有权证明。
  证据：A/B真实catalog lifecycle首次Update在adapter preview阶段失败；改为legacy library containment或project ownership record二选一，并在ExecutionEngine再次校验target/artifact evidence后，A-only Update、unmanaged user link拒绝与guarded rollback同时通过。
- 发现：Plugin的`Project`层并不等于“可重置的项目覆盖”；Claude shared settings与local settings都属于项目输入，但Reset override只应移除local/runtime声明。
  证据：inventory observation新增backend-only resettable语义；project B只有shared声明时Remove为Unavailable，项目A local override仍可Enable与Reset，user/shared/peer bytes保持不变。
- 发现：原ProjectDetail通过改变React `key`响应任何workspace change，collection Apply或conversion完成会同时卸载Settings editor；即使dirty guard阻止项目切换，仍可能因同workspace刷新丢失未保存草稿。
  证据：Settings与collection改为各自监听workspace刷新；Settings loader在同workspace保留相对旧baseline已变更的draft，React测试在磁盘baseline变化后仍保持用户输入。
- 发现：原History仅为Codex传project filter，并用store中的base `activeContext`生成rollback preview；Claude项目回执会混入全局History，而prepared runtime或项目回执可能因缺少project context无法回滚。
  证据：History现在对所有Agent提交active project filter，并只对backend标记eligible的receipt使用其持久化context；Claude project fixture验证filter和rollback context均精确包含project path。
- 发现：renderer组件卸载不会取消已进入Rust的Apply，但原资源UI把busy/result只保存在组件local state，重开后无法区分“仍在执行”“已完成”或“已取消”，容易诱发重复操作。
  证据：workspace operation tracker在组件外持有pending promise和暂态结果；卸载/重开测试证明Apply只调用一次，结果通过History持久化并回到原workspace。
- 发现：重复project conversion不能只用generated runtime config中的值相等判断no-change；首次bootstrap时base值可能恰好相同，但若manifest尚未记录该key，跳过overlay会让isolated runtime丢失项目声明。
  证据：初始启发式使project plugin conversion测试报`Project settings require a generated runtime config plan`；改为同时检查runtime manifest的`projectSettingsKeys`后，首次bootstrap保留overlay，第二次preview才稳定返回空mutation。
- 发现：detached operation仅按workspace key复用Promise时，同workspace的新plan可能静默拿到旧plan结果，造成“按钮已点但新Apply未执行”的假象。
  证据：共享tracker现在同时记录operation id；相同plan复用pending promise，不同plan明确拒绝且UI在workspace Applying期间保持busy。新增store测试覆盖单次invoke与冲突路径。
- 发现：macOS `create-dmg`偶发在中间镜像已包含正确`AD.app`后因卸载阶段退出，并留下`/Volumes/dmg.*`挂载；立即重跑会继续受到陈旧DiskImages/Finder状态影响。
  证据：失败后`hdiutil info`显示AD的`rw.*.dmg`仍挂载且内容完整；只卸载明确的build interstitial、不改变代码后，同一提交再次`pnpm tauri build`成功生成18:17的AD.app与DMG，结束后无AD临时挂载。

## 决策日志

- 决策：以项目工作区为产品中心，以用户任务闭环和风险分级证据判断完成。
  理由：模块或代码存在不能证明用户完成了安全、隔离、可恢复的工作流。
  日期/作者：2026-08-01 / Codex（承接用户确认的 brainstorm Product Contract）
- 决策：保留source catalog，使用immutable artifact隔离项目revision。
  理由：兼容现有Git/local来源能力，同时消除mutable shared symlink造成的跨项目隐式更新。
  日期/作者：2026-08-01 / Codex
- 决策：Source Catalog使用独立的sealed AD-state transaction，不伪造AgentContext/ResourceRef；显示名称只作UI metadata，backend UUID才是身份。
  理由：catalog、artifact与migration archive属于AD自身状态，伪装成Agent资源会错误继承scope/installation语义，并重新允许用户字符串成为路径身份。
  日期/作者：2026-08-01 / Codex
- 决策：扩展现有`AgentCollectionPanel`路径，不恢复`ProjectSkills`或另建第三条项目资源路径。
  理由：新路径已经接入AgentContext、ExecutionEngine与stale request保护；继续双轨会让状态再次失控。
  日期/作者：2026-08-01 / Codex
- 决策：本轮不开放外部Agent自动化。
  理由：现有planId/apply IPC不足以表达caller identity和independent human approval，直接开放会产生self-escalation风险。
  日期/作者：2026-08-01 / Codex
- 决策：接受文档审查提出的执行边界加固，不缩窄“项目隔离”承诺。
  理由：path检查和进程内锁无法证明多实例与active ancestor race下的隔离；改用fd-relative写入、cross-process lock和startup recovery gate。
  日期/作者：2026-08-01 / Codex（ce-doc-review）
- 决策：Settings完整管理包含项目层edit lifecycle，并对dirty draft、敏感值和context切换定义明确行为。
  理由：仅有effective view不足以完成用户任务，静默丢draft或跨scope复制secret都会破坏可信闭环。
  日期/作者：2026-08-01 / Codex（ce-doc-review）
- 决策：U1按workspace identity、inventory view、public plan/report三个边界拆分模块，同时保持Rust `agents`公共导出语义不变。
  理由：契约会被U3/U4/U5持续扩展，提前建立模块边界可避免再次把所有Agent语义堆入单文件。
  日期/作者：2026-08-01 / Codex
- 决策：跨进程advisory lock直接使用lockfile中已有的`rustix 1.1` `flock` API，只将`fs` feature声明为直接依赖。
  理由：满足macOS非阻塞排他锁与显式unlock需求，不增加新的crate或手写unsafe syscall；锁文件只承载version/instance/operation元数据。
  日期/作者：2026-08-01 / Codex
- 决策：regular-file target的受信root从raw project path、raw Claude/Codex home或canonical AD home重新建立，不能把adapter discovery已跟随symlink得到的canonical installation root当作写入授权。
  理由：canonical路径能描述“最终去了哪里”，但不能证明用户配置的root本身没有被symlink替换；held directory fd和`openat(O_NOFOLLOW)`才是提交期边界。
  日期/作者：2026-08-01 / Codex
- 决策：ExecutionEngine的read-only dependency observation与mutation confinement使用不同root capability；前者可观察另一个已配置且受信的base installation，后者必须严格绑定resource installation identity。
  理由：Project Runtime需要把base config作为只读precondition，但这不能隐式授权runtime plan写入base installation。
  日期/作者：2026-08-01 / Codex
- 决策：缺失的target parent以“最近存在ancestor fd + pending components”表示，precondition/backup阶段只观察，只有Apply写入才通过no-follow `openat/mkdirat`创建并同步目录项。
  理由：既保持ancestor-swap安全，也避免失败计划在receipt之前留下项目目录副作用。
  日期/作者：2026-08-01 / Codex
- 决策：每次ExecutionEngine操作只打开一次`ExecutionState`，并把held lock/journal/backups/history descriptors传过target lock、journal lifecycle、backup/compensation、receipt与rollback read。
  理由：逐文件校验不能保证跨阶段事务仍位于同一物理AD root；operation-scoped capability才能让路径替换后继续fail confined。
  日期/作者：2026-08-01 / Codex
- 决策：新operation journal和receipt使用no-replace hard-link publish，lockfile必须是current-user、single-link regular file。
  理由：操作证据与锁身份不能被并发writer或预置hard link静默覆盖；transition仍使用同directory fd内的atomic replace。
  日期/作者：2026-08-01 / Codex
- 决策：startup recovery使用跨进程exclusive lease，ExecutionEngine apply/rollback使用shared lease；任何未终结或不可解码journal阻止普通mutation。
  理由：恢复扫描与新写入不能并发，多个正常实例仍可在不同physical target上并行并由target lock协调。
  日期/作者：2026-08-01 / Codex
- 决策：prepared状态执行幂等backup清理后转`compensated`；applying仅在存在identity匹配的完整/补偿receipt时自动收敛，否则转`repair_required`且不猜测目标状态。
  理由：journal-only阶段确定尚未写target，而applying v1/v2现有证据不足以安全重放或覆盖。
  日期/作者：2026-08-01 / Codex
- 决策：精确匹配repair-required receipt的guarded rollback可穿过mutation gate，成功后把原journal标记为v2 `repaired`；其他mutation继续阻断。
  理由：修复动作本身必须可执行，但不能借修复入口放开无关写入。
  日期/作者：2026-08-01 / Codex
- 决策：新OperationReceipt使用schema v2，显式携带operation kind、parent/context、可选workspace/action identity、rollback eligibility与createdAt；无版本输入只按v1读取且永远不授予rollback。
  理由：History兼容展示与安全恢复需要分离；缺少新证据的旧文件不能因为仍能反序列化就自动获得写权限。
  日期/作者：2026-08-01 / Codex
- 决策：History目录级错误仍使请求失败，但单个文件的unreadable/malformed/future-version作为可排序诊断项返回并与正常receipt共存。
  理由：目录边界不可信时无法证明扫描范围，单文件问题则不应隐藏其他有效历史。
  日期/作者：2026-08-01 / Codex
- 决策：所有第一方apply/rollback IPC在claim时同时绑定preview context与risk fingerprint，不能只提交opaque plan id。
  理由：plan id证明后端生成过计划，但不能证明用户当前仍处于同一workspace、确认的风险摘要也未变化。
  日期/作者：2026-08-01 / Codex（ce-code-review）
- 决策：project root授权同时要求配置根不重叠、逐组件no-follow打开和preview/apply device+inode一致。
  理由：项目级隔离必须绑定物理目录身份，不能把可替换的path string当作持续授权。
  日期/作者：2026-08-01 / Codex（ce-code-review）
- 决策：项目History保留user/inherited receipt用于解释来源，但只有context精确匹配当前project的receipt可生成rollback plan。
  理由：隐藏user历史会损失provenance，允许项目页修改user层则违反项目默认隔离。
  日期/作者：2026-08-01 / Codex（agent-native review）
- 决策：receipt no-replace publish后的错误按可观察提交状态处理；精确receipt已存在时完成sync/reconciliation，不确定时保留恢复证据而不猜测补偿。
  理由：发布后的外部可见状态不能用发布前失败语义处理，否则History、target与journal会互相矛盾。
  日期/作者：2026-08-01 / Codex（ce-code-review）
- 决策：AD-owned project Plugin目录的显式Replace允许目标或旧source digest漂移，但仍要求ownership identity、preview digest、fresh source digest与workspace绑定；Delete、Skill link和rollback继续要求精确证据。
  理由：Plugin运行时可能产生可忽略缓存且source refresh是既有受支持流程；Replace本身已经显示并绑定当前内容，而破坏性删除和回滚必须保持更严格的外部修改保护。
  日期/作者：2026-08-01 / Codex
- 决策：在尚无可验证Agent二进制版本来源时，Claude/Codex Settings、Skills、Plugins coverage统一保持`partial`并返回`agent_version_unverified`，不因已扫描当前已知目录而宣称complete。
  理由：产品目标中的“管理全”必须建立在version/schema/location compatibility证据上；未知版本的完整性声明会制造虚假完成感。
  日期/作者：2026-08-01 / Codex
- 决策：legacy project Settings documents IPC只映射typed inventory中backend判定可编辑的项目层，并使用opaque workspace URI；user层与raw物理路径不再通过该项目入口返回。
  理由：兼容旧preview调用的同时关闭继承secret和跨scope target泄露，后续U6可在没有新consumer后删除该兼容入口。
  日期/作者：2026-08-01 / Codex
- 决策：Project collection action IPC只接受backend签发的workspace/inventory/resource身份和枚举动作；物理`ResourceRef`、path、catalog artifact与ownership record全部在重新盘点后由后端解析。
  理由：renderer只能请求已展示的用户意图，不能把路径或伪造资源身份升级为写权限；stale inventory、ownership/source drift和未确认risk在plan claim前后分别fail closed。
  日期/作者：2026-08-01 / Codex
- 决策：detached operation tracker只保存renderer会话内的展示状态，durable完成事实、恢复与rollback入口始终以后端History receipt为准。
  理由：UI暂态可以改善关闭/重开体验，但不能成为提交证据；进程退出或renderer崩溃后仍必须从fd-confined receipt恢复真实状态。
  日期/作者：2026-08-01 / Codex
- 决策：Project Workspace的三个tab保持surface挂载，并由各surface在同workspace内执行保留草稿的refresh；切换project/Agent仍走统一dirty guard。
  理由：tab是同一workspace的不同观察面，不应被当作销毁编辑会话的context切换；外部Apply完成也不应静默覆盖用户草稿。
  日期/作者：2026-08-01 / Codex
- 决策：Conversion的完整应用资格由后端根据required residual判定；safe subset必须作为显式option重新preview，不能由renderer从同一计划中自行删减mutation。
  理由：用户选择和risk fingerprint必须绑定真正执行的计划，renderer不能把“部分可做”自行升级为写权限；residual仍由domain report如实保留。
  日期/作者：2026-08-01 / Codex
- 决策：Conversion tracker只把workspace+plan id相同的pending操作视为同一次Apply，报告也只在两者同时匹配时附着到当前preview；完成结果限制为最近32个workspace。
  理由：避免旧报告污染新preview、不同plan静默复用及renderer长期会话的无界内存增长；durable事实仍以后端History为准。
  日期/作者：2026-08-01 / Codex（ce-simplify-code）

## 结果回顾

待执行完成后填写：

- 实际交付的用户工作流与evidence level。
- 与统一实施计划的偏差及原因。
- 迁移用户数量/fixture结果、遗留兼容窗口和可删除时间点。
- 未达到Release verified的能力及后续计划。
- 是否产生可沉淀到`docs/solutions/`的迁移、安全执行或项目隔离经验。

## 上下文和方向

AD 是 Tauri 2 macOS 应用。React通过`src/lib/tauri.ts`调用Rust commands；Agent adapter通过`SettingsPort`、`SkillsPort`、`PluginsPort`观察和规划资源；`ExecutionEngine`是唯一安全写盘层，提供plan store、digest preconditions、备份、补偿、receipt与guarded rollback。Codex Project Runtime位于`~/.ad/codex-homes/<project-id>`，manifest记录AD拥有的project overlay和Plugin状态。

当前ProjectDetail已经挂载`AgentSettingsEditor`与`AgentCollectionPanel`。后者能列出Skills/Plugins并预览toggle，但前端从`ResourceSnapshot.content`猜name/enabled，无法可靠表达继承、winner、ownership、单项限制或扫描完整性。legacy `ProjectSkills.tsx`虽未挂载，`commands/skills.rs`、`store/skills.ts`和Settings中的`SkillSourcesSection`仍保留另一套source/project写路径。

实施方向是把“真实项目环境”固化为共享domain contract：canonical context → effective inventory → item action policy → backend plan → human approval → execution receipt/history。所有frontend surface和Conversion都使用这条链；legacy只作为source catalog/migration输入。

## 工作计划

先完成U1 contract：定义backend workspace descriptor、resource/declaration/physical target身份、sealed Agent/AD-state target、versioned discovery coverage、inventory envelope、item action、public plan dependency、versioned receipt与domain report，并同步Rust、Zod、Tauri wrapper与contract tests。随后完成U10：统一fd-relative target confinement、cross-process target/recovery locks、synced durable operation journal、startup command gate、legacy receipt decoder、ownership record和rollback inverse plan。任何新source或lifecycle write必须等待U10门禁通过。

紧接着完成U8 Settings effective view和U3 collection inventory的只读部分，让项目环境先可被正确解释，再允许写操作。Settings明确敏感字段遮罩/不跨scope复制、semantic unknown-field preservation和dirty draft切换保护；Codex generated runtime config只作为derived projection，runtime identity与manifest先完成versioned迁移保护。

随后完成U2 acquisition：把现有source registry/scanner/Git helper提取成服务，通过`AdStateRef`管理catalog/staging/artifact，建立GUI-compatible trusted Git launch和network/time/disk budgets，写migration fixtures证明source update不跨项目。然后完成U3的action planners和backend workspace policy，覆盖所有权、runtime prepared、canonical context、source drift和risk acknowledgement。

后端动作稳定后，执行U4：扩展AgentCollectionPanel并拆分resource item/action dialog，统一Settings、Skills、Plugins、History和Codex runtime状态。UI所有按钮只渲染backend item actions；dirty draft先阻断context切换，其他stale preview立即失效。确认界面按影响/权限/target/技术细节排序，keyboard、focus、live status和非颜色表达全部可测。Apply开始后关闭UI只detatch，结果仍归档原workspace。

再执行U5后端：保留现有conversion route，把有效继承环境纳入source inventory，复用同一project planners与report/receipt。U9迁移Conversion UI，定义“提交resolution → 后端重新preview → 重新确认risk → Apply”的循环；未解决required item阻止full Apply，只允许明确的safe subset并产出partial residual。

最后执行U6迁移/清理：只读盘点legacy state，preview/apply reconciliation，成功后删除direct-write IPC与未挂载UI。U7运行全量自动化、production build和真实macOS workflow，依据证据更新产品文档并归档ExecPlan。

## 验证和验收

执行期每个milestone先运行受影响的focused tests，再运行下列全量门禁：

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
pnpm tauri build
git diff --check
```

行为验收必须在隔离temp home与真实安装app中覆盖：

- 项目A/B的同名Skills/Plugins、user inherited与project override；A的所有动作不改变B或user source。
- Claude user/shared/local与Codex base/project overlay的Settings effective winner及project-only edit；unknown字段保留，sensitive值遮罩且不自动跨scope复制，dirty draft不会静默丢失。
- Git/local source生成immutable revision；GUI-style最小环境仍支持授权Git/SSH source；source refresh后A/B保持旧revision，只有显式sync的项目变化。
- supported/degraded/external/unsupported逐项准确；unknown Agent version/schema/location和partial scan不隐藏其他资源且不宣称完整。
- Preview不修改Agent/project/catalog/published artifact，允许受控、operation-scoped staging；stale context、expired/replayed plan、source/target drift、hostile URL/ref/source id、network/time/disk budget、active ancestor race、unsafe AD root、special file、artifact collision和unowned target全部拒绝。
- 两个AD进程争用同一target时只有一个commit；startup recovery在mutation command可用前完成，journal file/parent sync fault不会产生无History写入。
- complete/compensated/partial和guarded rollback；外部修改后的rollback拒绝覆盖。
- Conversion包含inherited环境，source字节不变，resolution变化必须重新preview，required residual阻止full Apply，重复运行no-change/revalidate，partial不会显示完整成功。
- Workspace与Conversion UI支持keyboard-only、focus return、live progress和非颜色状态；applying中关闭再打开不会重复执行或显示假取消。
- Legacy migration重复运行幂等，冲突/缺失source不删除旧状态或external target。

## 幂等性和恢复

- 所有preview是只读且可重复；plan有短期过期时间，context/source/target变化后必须重新preview。
- Immutable artifact由规范tree manifest计算content digest并原子发布；重复acquisition先完整复验已有artifact，绝不覆盖。重复project install/update产生no-change或同一retarget结果。
- Migration先写新状态和receipt，再记录marker/归档旧状态；任何失败保留legacy registry/config/link并输出可重试状态。
- Unpublished staging在取消、失败或startup recovery后仅在确认未被plan/journal引用时清理；published artifact、legacy checkout和receipt pin不进入本轮GC。
- ExecutionEngine继续在所有写入前完成备份；apply失败尝试补偿，partial failure保留逐项residual和人工恢复入口。
- Legacy/current receipt逐文件兼容读取；证据不足的旧receipt仍可见但rollback unavailable，损坏或future receipt不会阻断整个History。
- Rollback只有在receipt、backup manifest、post-apply digest和current target一致时执行；漂移后拒绝覆盖外部修改。
- Source catalog删除与artifact删除分离；本轮不实现任何published artifact或legacy checkout物理GC，rollback-eligible receipt与migration archive持续pin旧revision。
- 若新workspace在发布前不可用，回退应用代码但不删除新artifact/receipt；legacy数据仍兼容读取。禁止通过`git reset --hard`或手工批量删除恢复用户数据。

## 接口和依赖

本计划优先复用serde/serde_json、Zod、Zustand、Tauri IPC和ExecutionEngine。fd-relative filesystem与advisory lock先做macOS API spike；若Rust标准库不足，允许引入一个最小、审计过的直接syscall依赖（优先复用lockfile中已有的`rustix`能力），并在MD决策日志记录版本、API面和替代方案。现有shell-string Git helper不能直接复用。

里程碑结束时必须存在并保持Rust/TypeScript语义一致的接口概念：

- `WorkspaceDescriptor` / `WorkspaceKey`：canonical project、base/effective installation、runtime identity与opaque revision，由后端签发。
- `ProjectWorkspaceInventory`：workspace key、inventory revision、category coverage、Settings/Skills/Plugins views和diagnostics。
- `ResourceKey` / `DeclarationKey` / physical `ResourceRef`：分别标识effective resource、layer declaration和ExecutionEngine target。
- Sealed `MutationTarget`：backend-only区分`AgentResourceRef`和allowlisted `AdStateRef`；public IPC不暴露或接受物理path。
- `CollectionResourceView`：resource identity、layers/provenance、effective state、ownership、health、management state和item actions。
- `CollectionAction` / `CollectionActionState`：add/install、toggle、sync/update、remove/reset override、external steps及支持状态。
- `SettingsEffectiveView`：layer/field provenance、winner、editable target、sensitive classification和semantic unknown-field preservation信息。
- `MutationPlanView`：sanitized target、scope、read-only/user dependency、risk disclosure、required acknowledgement和expiry。
- Versioned `OperationReceipt`：attempted mutation、backup/artifact pins、原workspace/action identity与rollback eligibility。
- `WorkspaceOperationReport` / `ConversionReport`：包装零或一个receipt，并保存no-change/external/unsupported/conflict/residual。
- `SkillArtifactRef`：source identity、revision、directory digest、immutable location和ownership。
- `OperationJournal` / `OwnershipRecord`：synced crash recovery状态、writer instance与精确link/package/receipt所有权。
- `AdapterDiscoveryContract`：已验证Agent version/schema/location set与unknown-future coverage降级规则。

Adapter只实现inventory与plan接口；Tauri command负责context/request validation和plan store编排；ExecutionEngine负责写盘与receipt；React不持有mutation content或自行决定ownership。

## 里程碑规划

### M0 — 合同与证据基线

完成U1的Rust/TS shape、backend workspace identity、Agent/AD-state target与versioned discovery trace，并完成U10 fd-relative confinement、cross-process locks、synced journal、startup recovery gate、legacy receipt、ownership与rollback plan。只改contract/engine/tests，不开放新UI动作。验收：严格schema、stale/risk claim、active ancestor race、unsafe AD root、two-process contention、crash/sync fault和rollback activation tests通过。

### M1 — 正确解释有效项目环境

完成U3 inventory只读部分与U8 Settings effective view。验收：Claude/Codex layers、winner、version-bound coverage、health、sensitive masking、dirty draft guard、prepared/unprepared runtime和canonical path identity准确。

### M2 — 隔离Skill acquisition

完成U2 immutable artifact与source plan。验收：A/B revision隔离、GUI Git auth、resource budgets、source drift、referenced deletion、staging recovery和legacy input migration tests通过。

### M3 — 完整资源生命周期

完成U3 action planners和安全preview/apply。验收：item action matrix、ownership、external/degraded、risk acknowledgement、compensation与rollback测试通过。

### M4 — 统一项目工作区

完成U4 ProjectDetail/collection/settings/history交互。验收：用户可走通所有真实支持动作，五类empty/error状态、risk hierarchy、keyboard/focus/live status、detached apply、stale context和receipt入口准确。

### M5 — 转化收敛

完成U5 effective-source conversion backend与U9 UI integration。验收：真实resolver循环、re-preview、safe subset、inherited source、project-only target、replay、risk fingerprint、partial residual和failure recovery通过。

### M6 — Legacy退役

完成U6 reconciliation和direct-write删除。验收：迁移幂等可恢复，全仓库无legacy project API consumer，Settings source入口继续可用。

### M7 — 发布证据与文档

完成U7全量门禁、production Tauri build、真实app workflow和文档同步。验收：evidence matrix支持每项状态，HTML保持批准基线，MD记录实际结果后一起归档。

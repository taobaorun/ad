# Implementation Plan: Skill source checkout links

Contract: ad-implementation-plan/v1
Plan ID / version: `IP-skill-source-checkout-links` / `v1`
Status: `complete`
Product Contract: `PC-skill-source-checkout-links-2026-08-05-v1` at `docs/plans/2026-08-05-001-feat-skill-source-checkout-links-contract.md`
Technical Design: `TD-skill-source-checkout-links/v1` accepted; source digest `sha256:6f39bdd4ba2f82a4e77d82744ed071a8670a9d4db6ea412c7c2310ee4ad395d5`
Commit policy: `delivery-only`
Commit policy authority: user invocation of `ad-harness:ad-lfg` on 2026-08-05; local delivery commit and repository push/PR are authorized, merge is not

## Goal and execution boundary

把 Project Workspace 的 managed Skill 安装从“链接到独立 immutable artifact revision”改为“链接到原始 Local source 或稳定 Git managed checkout”。Local/Git source 变化通过同一 source binding 影响所有关联安装；Agent 侧仍只写 symlink，并继续受 Preview、确认、ownership、receipt、rollback 和 confinement 保护。

本计划授权 `ad-work` 修改工作树与运行验证，并由 `ad-ship` 在最终 verified/ready tree 上创建交付提交、push 当前仓库分支和创建或更新 PR；不授权 merge、部署、真实用户 source 更新或其他外部写入。开发和测试必须使用临时 `AD_HOME`、临时项目与本地 Git fixture，不得修改当前 `~/.ad`、真实 Agent homes 或现有项目 Skill links。

## Implementation decisions

### D1 — Source binding replaces install artifact identity

新增版本化 `SkillSourceBinding` / `SkillSourceItem`。Catalog v2 保存 source identity、stable/physical roots、observed revision、manifest/digests、Skill subpaths 和 activation impact；安装请求从自由 `path + artifactId` 改为 backend-issued binding。Local stable root 是规范化原始目录；Git stable root 是 `~/.ad/skill-library/<opaque-source-key>/current`。

Catalog、plan、receipt 与 IPC decoder 在迁移期读取 v1 artifact 和 v2 binding；新写入只产生 v2。`source-key` 由 backend-issued source ID 生成，不使用 display name、URL 或用户可控 path segment。

### D2 — Stable lexical link target is separate from canonical validation

Port/ExecutionEngine 接收两个概念：实际写入 symlink 的 lexical stable target，以及 apply 前用于安全检查的 canonical resolution/digest。Git link text 必须保留 `current/<skill-subpath>`，不能被 `canonicalize` 改成 generation path；Local link text使用 source add 时记录的规范绝对路径。

### D3 — Ownership v2 follows source identity, not mutable content

Ownership v2 继续绑定 workspace、physical target、精确 link target digest、creating/updating receipt，同时记录 source ID/kind/stable-root identity/Skill subpath。Source tree digest 是 health 与 evidence，不是 delete/replace authority；内容变化不撤销 ownership，手工 retarget 或 source identity/subpath 变化仍 fail closed。

### D4 — Git uses validated generations and one atomic current switch

Git add/update 在 operation staging 中 clone/fetch/checkout、检查 tree/activation impact，然后发布 generation 并原子切换 `current`。切换前将 staged manifest 与全部关联 ownership subpath join；缺失或无效的已安装 Skill 阻止切换。Source update receipt 记录 before/after revision 和所有受影响 workspace；rollback 恢复上一 current generation。

### D5 — Legacy migration is explicit; source removal is reference-gated

V1 artifact links 继续可读可用并显示 `Relink`。Relink 复用 Project Collection Preview/Apply/rollback，将单个 workspace link 与 ownership 升级到 source binding。只读 inventory/startup 不写入。任何 AD-owned link 仍引用 source 时，source remove preview 返回结构化关联列表并禁止 apply；零引用后才删除 catalog binding，Git generation/artifact GC 仍不在本轮执行。

### D6 — Backend signs action intent and blocked previews

`ResourceAction` request继续使用稳定的 Install/Update/Remove；`ResourceActionView`新增backend-owned intent（`standard | relink | repair`），Project action Apply重新检查当前resource state与intent条件，frontend不能提交或升级intent。Skill source plan view新增 `applicability: applicable | blocked`、`blocking_issues` 与 `affected_resources/workspaces`；breaking Git update和有引用的source removal都返回可审阅的blocked preview，但PlanStore拒绝claim/apply。这样UI可以解释影响而不把错误字符串当mutation authority。

### Rejected implementation shortcuts

- 不直接恢复 legacy `commands/skills.rs` 的即时文件写入；它绕过新 Workspace、plan、receipt 和 ownership 契约。
- 不在 Git working tree 内原地 `pull`；消费者会观察到部分更新，且崩溃恢复不可靠。
- 不在 source update 时逐项目重写 link；这会制造跨项目 partial failure。
- 不把 source content digest继续当 ownership 条件；这与 live-link requirement 冲突。

## Scope deltas

- `technical-necessary` — Catalog/ownership schema v2。若省略，R3 的合法内容更新会让安装被误判为外部，或 catalog 仍只能表达 artifact pin。Authority: accepted TechnicalDesign.
- `technical-necessary` — Git generation/current transaction。若省略，R2/R3 只能通过非原子 working-tree update 或跨项目 link rewrites 实现。Authority: accepted TechnicalDesign.
- `technical-necessary` — shared-impact preview、missing-subpath gate 与 evidence invalidation。若省略，Git update 可静默破坏多个项目。Authority: accepted TechnicalDesign.
- `technical-necessary` — legacy Relink 与 source-remove reference gate。若省略，既有安装会被静默改写、遗弃或变成 dangling link。Authority: accepted TechnicalDesign.
- `adjacent` — generation/artifact GC、Local content rollback、跨机器 source 分发、Windows/Linux link variants。Disposition: defer.

## Implementation units

### U1 — Add a v2 source-binding catalog and stop copying Local source trees

- **Requirements:** R1, R4, R8
- **Depends on:** none
- **Observable result:** Settings → Skill Sources 对 Local source 执行 Add/Refresh 后，catalog/plan/receipt 暴露一个指向原始规范目录的 binding；临时 staging 被清理，`~/.ad/artifacts/skills` 不产生新的 Local tree，Skill inventory 仍能列出内容与 activation impact。
- **Change:**
  - 在 `src-tauri/src/agents/skill_source_bindings.rs`（新文件）定义 binding/item、source scan、stable-root/physical-root validation、manifest 与 source-key helpers；复用 `skill_artifact_tree.rs` 和 `skill_activation.rs` 的 hostile-tree 限制。
  - 在 `src-tauri/src/fs/paths.rs` 增加 managed source root/generation 路径 helper，保持 AD root no-follow/权限约束。
  - 将 `skill_catalog.rs` 升级为 v2 document/entry，同时保留 v1 decode；把 `current_artifact` 的新写入替换为 `current_binding`。
  - 将 `skill_catalog_plans.rs` 的 staged artifact 改为 staged source binding；Local preview 只生成验证 manifest/metadata，不发布 tree copy。Plan risk fingerprint绑定 source ID、stable root、tree/activation digest 和 catalog revision。
  - 将 `skill_catalog_execution.rs` 的 catalog transaction、journal、receipt 和 startup recovery升级到 binding；Local apply 在锁内复验原始 source revision并只提交 catalog bytes/receipt。
  - 更新 `src-tauri/src/agents/mod.rs`、`src-tauri/src/commands/skill_catalog.rs` 的 exports/IPC，但暂时为 v1 frontend字段提供兼容 decoder/adapter，避免在 U1 中破坏 UI。
- **Focused verification:**
  - 扩展 `skill_artifacts_tests.rs`/新增 binding tests：Local hostile tree、drift-before-apply、取消/过期计划、无 artifact tree publication、staging cleanup。
  - `cargo test --manifest-path src-tauri/Cargo.toml skill_source_binding`
  - `cargo test --manifest-path src-tauri/Cargo.toml skill_catalog`
  - `cargo test --manifest-path src-tauri/Cargo.toml --test skill_catalog_execution`
- **Checkpoint/recovery:** v1 catalog fixtures必须在首次只读 load 后字节不变；U1 未完成时保留 v1 writer，不提交半升级 schema。

### U2 — Install Local Skills as live source links with ownership v2

- **Requirements:** R1, R3, R4, R7
- **Depends on:** U1
- **Observable result:** 从 Project Workspace 安装 Local Skill 后，Claude `.claude/skills/<name>` 或 Codex `.agents/skills/<name>` 的 `read_link` 等于原始 Skill 目录；编辑 source 后项目读取到新内容、link bytes不变、inventory仍标为 AD-managed。移除只删除该项目 link/ownership。
- **Change:**
  - 在 `collection_actions.rs` 以 verified `SkillSourceBinding + subpath` 代替 artifact path 构造安装/更新请求；拒绝 frontend提供任意 target。
  - 在 `claude_ports/skills.rs`、`codex_skills.rs` 拆分 lexical target 与 canonical validation，更新 create/replace/remove 和 source digest precondition；不得 canonicalize 后再写 link。
  - 在 `capabilities.rs`（或相邻 typed request）新增 backend-only source-binding payload，删除 managed collection path 对自由 JSON path 的依赖；conversion 的已确认 external-local path route保持兼容但不获得 catalog ownership。
  - 将 `resource_ownership.rs` 升级为 v2 ownership record/validator；`execution.rs` 在 apply 时验证 source binding与 preview digest，成功后保存 link/source identity，并允许随后内容 drift。
  - 更新 `collection_skills.rs` 的 installed observation/health：source drift与broken source单独报告，link target正确时 ownership不降级；`collection_management.rs` 对 v2 live link不再按 artifact差异提供 per-project Update。
  - 保留 Codex enable/disable config路径；canonical `SKILL.md` 可随 live source变化，但 toggle preview必须重新解析当前有效路径。
- **Focused verification:**
  - 改写 `collection_actions_tests::catalog_skill_actions_keep_project_revisions_isolated` 为 Local A/B live-binding contract：两个 link均指向 original source，source编辑同时可见，移除 A 不改变 B。
  - 增加 source drift后 ownership有效、人工 retarget/目录占位/外部 link拒绝、install rollback、Codex config toggle测试。
  - `cargo test --manifest-path src-tauri/Cargo.toml collection_actions`
  - `cargo test --manifest-path src-tauri/Cargo.toml resource_ownership`
  - `cargo test --manifest-path src-tauri/Cargo.toml agent_parity`
- **Checkpoint/recovery:** 若 ownership v2 写入失败，现有 ExecutionEngine compensation恢复原 link/ownership；不允许 link成功但ownership缺失时返回 complete。

### U3 — Deliver Git managed checkout generations, shared update, and rollback

- **Requirements:** R2, R3, R4, R5, R7, R8
- **Depends on:** U1, U2
- **Observable result:** 两个项目安装同一 Git Skill 后，link text均指向同一 `skill-library/<source-key>/current/<subpath>`；Source Update只原子切换 `current`，两个项目同时读取新 revision且项目 link bytes不变；source rollback使两者同时恢复。新 revision缺失任一已安装 Skill时 preview/apply拒绝切换。
- **Change:**
  - 在 `skill_source_bindings.rs` 实现 Git staging、fixed-argv Git executable resolution、generation publish、manifest验证和 stable `current` link metadata；复用现有 acquisition limits，不使用用户输入 shell interpolation。
  - 在 `skill_catalog_plans.rs` 的 Git Update plan加入 before/after binding、affected workspace/resource summaries、breaking Skill diagnostics与 acknowledgement风险指纹。
  - 为 source plan view 增加backend签发的applicability/blocking issues；missing-subpath update返回blocked preview，PlanStore的claim对blocked plan稳定拒绝且不消费/发布staging。
  - 在 `skill_catalog_execution.rs` 扩展 journal state，使 generation publish、current switch、catalog write、receipt write可在 crash recovery 中 finish-or-compensate；TargetLockSet覆盖 catalog、source root/current与ownership index。
  - 在 `resource_ownership.rs` 提供按 `source_id` 查找所有有效 v2/v1 ownership 的只读 index；unknown/corrupt record使 update fail closed而不是漏报影响。
  - 扩展 receipt/history schema记录 affected workspace keys、before/after source revision、current link digests和 evidence invalidation consequence。
  - Source Update staged manifest与每个关联 Skill subpath join，任何缺失/非目录/无 `SKILL.md` 返回稳定错误码 `source_update_breaks_installed_skill`。
- **Focused verification:**
  - 用本地 bare/non-bare Git fixtures覆盖 add、branch/ref、no-change、A/B shared update、missing-subpath block、current tamper、catalog/source drift、并发 update、crash before/after switch、rollback/restart。
  - 验证 update不改写任何项目 link，失败/补偿时 old current与两个项目内容保持不变。
  - `cargo test --manifest-path src-tauri/Cargo.toml skill_catalog_execution`
  - `cargo test --manifest-path src-tauri/Cargo.toml execution_recovery`
  - `cargo test --manifest-path src-tauri/Cargo.toml collection_actions`
- **Conditional specialist:** U3完成后运行 `ad-security`，范围限于 Git argv/env、source root confinement、symlink race、ownership authority与journal recovery；高置信 finding返回本单元修复并重跑focused tests。
- **Checkpoint/recovery:** 每个测试使用独立 temp `AD_HOME`；故障注入后必须能重新打开 catalog并得到 old-or-new完整状态，禁止测试真实 `~/.ad/skill-library`。

### U4 — Migrate legacy artifact links and guard source removal

- **Requirements:** R4, R5, R6, R7
- **Depends on:** U2, U3
- **Observable result:** v1 artifact link仍被识别为 AD-managed并显示 Relink；Relink apply把单个项目切到 stable source target，rollback恢复精确旧 artifact target。只要任一有效/待迁移 ownership引用 source，Remove Source preview显示关联资源并拒绝删除；全部移除后只退役 catalog binding，不执行 artifact/generation GC。
- **Change:**
  - 扩展 `skill_legacy_inventory.rs` / `skill_legacy_migration.rs` 或新增窄的 v1→v2 binding adapter，使 catalog v1 artifact和ownership v1可只读解析；不得恢复 legacy direct-write commands。
  - 在 `collection_skills.rs` 标记 `legacy_artifact_link` management state，在 `collection_management.rs` 把 Update语义收敛为 Relink，并在 `collection_actions.rs` 生成 guarded Replace + ownership upgrade plan。
  - 扩展 `ResourceActionView` 的 backend-owned intent；legacy artifact link的Update intent为Relink，link identity仍有效但source binding需要重建时为Repair。若source本身不健康，Repair保持unavailable并引导先恢复/refresh source，不能用Repair覆盖外部target。
  - 为 v1 Git artifact创建首个 stable current view时允许引用被保留的 verified artifact tree，不复制；后续 source update再发布正常 generation。Local Relink重新验证原始 location/subdirectory/current Skill path。
  - 在 `skill_catalog_plans.rs` 的 Remove preview查询全部 v1/v2 ownership；引用存在时返回 `applicability=blocked`、结构化 blocker/affected resources，PlanStore不允许claim destructive plan。零引用 apply只改 catalog/retention metadata。
  - Receipt rollback恢复旧 link、旧 ownership schema和retention pin；startup replay保持幂等。
- **Focused verification:**
  - 扩展 `src-tauri/tests/skill_catalog_migration.rs`：v1 bytes只读、Local/Git Relink、rollback、重复迁移、损坏ownership、dangling artifact、source remove blocker、零引用remove。
  - 扩展 `collection_actions_tests.rs`：只迁移Project A时B仍指向旧 artifact；A rollback不改变B。
  - `cargo test --manifest-path src-tauri/Cargo.toml --test skill_catalog_migration`
  - `cargo test --manifest-path src-tauri/Cargo.toml collection_action_receipt_supports_guarded_rollback`
- **Conditional specialist:** U4 schema/recovery实现后运行 `ad-migrate` 评估 forward/backward reader、重复执行和失败恢复；不授权对真实用户数据执行迁移。
- **Checkpoint/recovery:** 保留v1 decoder、artifact retention和rollback fixture，直到完整发布验证完成；不在本单元删除 legacy artifacts或旧 state文件。

### U5 — Expose shared source semantics and Relink/Repair in the UI

- **Requirements:** R5, R6, R8
- **Depends on:** U1–U4 backend contracts stable
- **Observable result:** Skill Sources dialog展示 Local live path或Git managed checkout revision、更新会影响的workspace数量和breaking diagnostics；Project Collection actions把旧 artifact Update显示为 Relink，把broken binding显示为 Repair，不再宣称项目 revision isolation。
- **Change:**
  - 更新 `src/lib/skillCatalogTypes.ts`、`src/lib/agentTypes.ts` 与 Tauri wrappers，严格解析 binding、applicability/blocking issues、affected workspaces、backend-owned action intent、migration/health diagnostics和新receipt字段；schema拒绝未知/不完整 mutation authority。
  - 更新 `src/store/skills.ts` 在 source operation完成后刷新source与当前Project inventory，并保持 busy/error/plan cancellation行为。
  - 更新 `SkillSources.tsx`、`SkillCatalogPlanDialog.tsx`：Local live、Git shared impact、before/after revision、missing Skill blocker、remove references、rollback consequence；删除 `revisionIsolation` 承诺。
  - 更新 `AgentCollectionPanel.tsx`/Project support，使 action label和说明根据 backend-owned standard/Relink/Repair intent及 limitation code呈现，仍只提交 backend-issued ResourceKey/action；frontend不提交intent。
  - 更新 `src/i18n/locales/en.json`、`zh.json` 与locale contract，确保 shared-impact 和 destructive/blocker信息中英文完整。
- **Focused verification:**
  - `pnpm vitest run tests/components/SkillSources.test.tsx tests/components/AgentCollectionPanel.test.tsx tests/lib/agentTypes.test.ts tests/i18n/locales.test.ts`
  - 覆盖 affected workspace摘要、breaking update禁用、Relink确认/取消/失败、Repair limitation、remove blocker、慢apply期间防重复提交。
- **Experiential acceptance:** not-required；行为是结构化文件系统/receipt契约。生产 `.app` 的隔离临时项目 smoke在U6作为客观release evidence执行。
- **Checkpoint/recovery:** frontend在同一单元内兼容backend v2；不以自由字符串/path补造backend未返回的影响范围。

### U6 — Harden the full lifecycle, update canonical docs, and pass release gates

- **Requirements:** R1–R8
- **Depends on:** U1–U5
- **Observable result:** Local/Git source add→install→live update→relink/remove→rollback在Claude/Codex临时项目中闭环；产品规格、设计和UI都描述共享 checkout语义；完整CI和macOS release build通过。
- **Change:**
  - 增加端到端临时home fixtures，覆盖Claude/Codex、Project A/B、Local/Git、legacy v1、source removal、crash recovery、hostile source与ancestor/link swap。
  - 更新 `docs/product-specs/project-agent-workspace.md` 中 immutable artifact/project pin 声明、证据矩阵和真机流程；同步 `README.md` 或其他声称独立revision的当前文档。历史 completed plans保留as-built历史，不篡改为新事实；需要时加superseded说明。
  - 将 accepted `TD-skill-source-checkout-links/v1` 在实现完成后更新为 as-built需走单独设计生命周期；本单元只记录偏差与证据指针，不擅自改变accepted body。
  - 运行 `ad-security` residual复核、`ad-verify`完整verification contract和`ad-code-review`；任何tree-changing fix使旧receipt失效并重跑相关focused/full checks。
- **Focused verification:** 下方完整 Verification contract。
- **Checkpoint/recovery:** release smoke只使用隔离temp source/project与测试installation；不得安装到真实默认Claude/Codex homes。任何production build生成的tracked/untracked artifact按仓库规则清理，保留用户原有 `.agents/` 不动。

## Dependency order

```text
U1 source binding catalog
  -> U2 Local live install + ownership
    -> U3 Git shared checkout transaction
      -> U4 legacy Relink + remove guard
        -> U5 UI lifecycle
          -> U6 full hardening/docs/release
```

U5可以在U4 backend schema冻结后并行编写展示测试，但在U1–U4完成前不得以frontend mock定义新的后端语义。

## Verification contract

### Repository baseline

执行前记录 `git status --short --branch`，确认当前HEAD与用户已有 untracked `.agents/`；所有单元保留该目录，不加入diff、不清理、不改写。先运行最小基线以区分既有失败：

```text
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml --all-features
```

若基线存在失败，记录精确命令/测试和tree identity；只在本变更触及或恶化时纳入修复，不把无关失败隐藏在计划单元中。

### Acceptance scenarios

| Scenario | Requirements | Required evidence |
|---|---|---|
| Local direct link | R1, R4 | `read_link`为原始Skill path；无新Local artifact tree；receipt/ownership v2完整 |
| Local live drift | R3, R7 | 修改source后link bytes不变、内容更新、ownership仍managed；retarget则失效 |
| Git shared update | R2, R3, R8 | A/B指向同一current view；一次source update同步更新两者并列出affected workspaces |
| Git rollback/recovery | R3, R4 | rollback恢复两者；crash前后重启只得到完整old/new状态 |
| Breaking Git revision | R5 | 缺失任一installed subpath时current不切换、消费者内容不变 |
| Project isolation | R4 | Install/Remove/Relink A不改写B link/ownership；source update仅按显式shared contract影响内容 |
| Legacy migration | R6 | v1只读可用、显式Relink、rollback精确恢复、重复运行幂等 |
| Source removal | R5 | 有引用时结构化阻塞；零引用时catalog removal成功且不做GC |
| Hostile/tampered state | R4, R7 | escaping symlink、special file、stale plan、ancestor/current swap、损坏ownership均fail closed |
| UI comprehension | R8 | shared impact、Relink/Repair/remove blocker中英文呈现；无revision-isolation旧文案 |

### Focused and cross-unit checks

```text
cargo test --manifest-path src-tauri/Cargo.toml skill_source_binding
cargo test --manifest-path src-tauri/Cargo.toml skill_catalog
cargo test --manifest-path src-tauri/Cargo.toml collection_actions
cargo test --manifest-path src-tauri/Cargo.toml resource_ownership
cargo test --manifest-path src-tauri/Cargo.toml --test skill_catalog_execution
cargo test --manifest-path src-tauri/Cargo.toml --test skill_catalog_migration
cargo test --manifest-path src-tauri/Cargo.toml --test agent_parity
pnpm vitest run tests/components/SkillSources.test.tsx tests/components/AgentCollectionPanel.test.tsx tests/lib/agentTypes.test.ts tests/i18n/locales.test.ts
```

若实际test target/filter名称调整，WorkReceipt记录替代命令与覆盖映射，不以“无匹配测试”的成功退出作为证据。

### Evidence roles and delivery consequence

- **V1 — Required:** 全部 acceptance scenarios 的隔离 Rust/TypeScript 自动化证据。任一场景失败为 `needs-work`；不得用更低保真检查替代。
- **V2 — Required:** frontend format/lint/typecheck/full tests/build 与 Rust fmt/strict Clippy/all-targets tests。任一失败为 `needs-work`。
- **V3 — Required:** `ad-security` final-tree evidence覆盖 Git argv/env、source-root confinement、symlink race、ownership authority 与 journal recovery。高置信 finding 返回 `ad-work` 修复后重验。
- **V4 — Required:** `ad-migrate` evidence覆盖 v1/v2 reader、显式 Relink、幂等和失败恢复。兼容性 finding 返回 `ad-work` 修复后重验。
- **V5 — Preferred:** production Tauri bundle和隔离 `.app` 启动 smoke。若打包环境不可用，允许保留 draft PR但不得达到 merge-ready；没有预授权 fallback。
- **V6 — Preferred:** 隔离 production `.app` 的完整 Preview → Apply → update/Relink → History → rollback UI workflow。若自动化 harness 无法驱动 Tauri UI，可由同一 backend lifecycle集成测试加 packaged-app startup作为 fallback，保真边界是不证明人工桌面点击路径；authority为本计划，交付后果是明确 residual，不阻止 merge-ready，因为体验验收为 `not-required`。

### Final regression/build/release checks

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

Production bundle smoke使用隔离temp项目验证Preview → Apply → source update/Relink → History/receipt → rollback；记录 `.app`/DMG路径与摘要，不把构建成功代替工作流证据。

### Conditional specialists and skips

- `ad-security`: required after U3 and final tree；检查source checkout/confinement/Git execution/ownership/recovery。
- `ad-migrate`: required after U4；检查v1/v2 catalog/ownership/link迁移与rollback。
- `ad-verify`: required on final exact tree；汇总所有要求/单元/命令证据。
- `ad-code-review`: required after verified final tree；覆盖design→plan→diff、correctness、security、migration与evidence sufficiency。
- `ad-test-browser`: skip；变更界面运行于Tauri desktop且核心风险为filesystem transaction，React交互由Vitest覆盖。
- `ad-performance`: skip unless source inventory出现可重复性能退化；若大量ownership join成为风险，先建立fixture和阈值再调用。
- Experiential acceptance: `not-required`；无主观视觉/交互质量要求。隔离 `.app` smoke属于客观release evidence。

## Risks and recovery

| Risk | Recovery contract |
|---|---|
| Catalog v2 writer使旧state不可读 | v1 fixtures与dual reader先落地；首次v2 write前保留catalog backup/journal；失败恢复原bytes |
| Local source在preview/apply间变化 | lock内revision/manifest复验，返回SourceChanged；不写catalog/link |
| Project link canonicalized成generation pin | lexical-target tests断言精确read_link；canonical path仅用于验证 |
| Git current切换后catalog/receipt失败 | journal finish-or-compensate；恢复old current或完成new catalog，绝不留下unknown状态 |
| Source update遗漏某project | ownership index完整性与corrupt-record fail-closed；preview/receipt列出affected workspace keys |
| Legacy rollback target被提前删除 | artifact/generation retention pin；本轮无GC |
| Local live source引入危险内容 | UI标记live trust boundary；inventory/relink/update重验并报告health；Skill执行沙箱不在AD权限内 |
| 用户已有工作树内容被覆盖 | specific-file edits、每单元status检查；`.agents/`和无关dirty files保持原样 |

发生实现失败时，停止在最近完成单元边界，保留未完成diff与focused red/green evidence。不得用删除用户state、重置worktree或真实source mutation恢复测试；测试状态通过tempdir销毁或fixture重建恢复。

## Definition of done

- Accepted TechnicalDesign `TD-skill-source-checkout-links/v1` 的R1–R8全部由代码、测试和文档覆盖，无未授权Scope Delta。
- 新Local install不发布Skill tree副本，Agent link直接指向原始Skill目录。
- 新Git install指向稳定managed checkout view；共享update/rollback、missing-subpath gate和crash recovery有A/B证据。
- Ownership v2在content drift下保持有效，在target/source tamper下fail closed。
- V1 artifact link显式Relink/rollback可用；source removal不会制造dangling install；未实现GC。
- UI/i18n/产品规格不再声称项目独立artifact pin，并正确呈现shared impact、Relink、Repair和blocker。
- 所有focused/full checks、strict Clippy、production Tauri build和隔离 `.app` workflow通过；任何skip有证据与后果说明。
- Final VerificationReceipt和ReviewReport绑定同一exact tree，工程 verdict为verified/ready；residual仅包含已明确defer的GC、Local content rollback和non-macOS支持。
- Commit policy保持`delivery-only`；仅在最终 verified/ready tree 创建交付提交并由 `ad-ship` push/创建或更新PR，不merge且不写真实用户配置。

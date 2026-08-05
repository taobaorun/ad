# Technical Design: Skill source checkout links

Design: ad-technical-design/v1
Design ID / version: `TD-skill-source-checkout-links` / `v1`
Status: `accepted`
Source digest: `sha256:6f39bdd4ba2f82a4e77d82744ed071a8670a9d4db6ea412c7c2310ee4ad395d5` (SHA-256 of this Markdown with the digest value replaced by `PENDING`)
Product Contract: `PC-skill-source-checkout-links-2026-08-05-v1`
Requirements: `R1`–`R8`
Authority: user request on 2026-08-05, clarified as “让安装链接直接指向原始/受管 source checkout”
Review report: `docs/design-docs/skill-source-checkout-links.review.md`

## Product Contract

### Actor and outcome

本地开发者从 AD 管理的 Skill source 安装 Skill 后，Agent 的 Skill 目录中只保留一个由 AD 管理的软连接。Local source 直接关联原始目录，Git source 关联 AD 持有的稳定 checkout；source 内容变化会反映到所有关联安装，不再为每个安装维护独立副本或独立 revision pin。

### Requirements

- **R1 — Local source 直接关联。** 新安装的 Local Skill 链接目标必须是经校验的原始 source 中对应 Skill 目录。
  - Acceptance: `read_link(installed_skill)` 等于规范化后的 Local Skill 路径，且 AD 不为该安装创建 Skill tree 副本。
  - Provenance: 用户确认的目标行为。
- **R2 — Git source 关联稳定受管 checkout。** 新安装的 Git Skill 链接目标必须位于该 source 的 AD-managed checkout 稳定视图下，不指向项目私有副本。
  - Acceptance: 多个项目安装同一 Git Skill 时链接到同一 source checkout view。
  - Provenance: 用户确认的“受管 source checkout”。
- **R3 — Source 更新自动传播。** Local source 的外部修改、Git source 的已确认更新，在不改写项目安装链接的前提下对所有关联项目可见。
  - Acceptance: source 内容更新后，两个项目从原链接读取到新内容，链接文本保持不变。
  - Provenance: 直接关联 source checkout 的必要语义。
- **R4 — 安装仍受安全执行契约保护。** Install、Relink、Remove 必须保留 Preview、确认、目标冲突检测、receipt 与 rollback；不得覆盖外部目录或非 AD-owned link。
  - Acceptance: stale plan、人工 retarget、普通目录占位和跨项目目标均 fail closed。
  - Provenance: 现有 Project Agent Workspace 产品合同与用户未授权放宽的安全边界。
- **R5 — Source 生命周期不得制造悬空安装。** 存在 AD-owned 关联链接时，source removal 不得删除受管 checkout 或遗弃 ownership。
  - Acceptance: source removal 被阻止并列出关联项，直到链接被移除或迁移；Git update 缺失任一已安装 Skill subpath 时不得切换 checkout。
  - Provenance: R2–R4 的技术必要条件。
- **R6 — 既有 artifact 链接可控迁移。** 已安装且指向 `~/.ad/artifacts/skills/<digest>/tree` 的 Skill 保持可用，用户通过受保护的 Relink/Update 将其切换到 source checkout；不得在启动或只读 inventory 时静默重写。
  - Acceptance: 迁移前旧链接可继续使用；迁移 preview/apply/rollback 能在旧 target 与新 checkout target 间往返。
  - Provenance: 现有安装兼容性与 mutation authority 约束。
- **R7 — Ownership 绑定链接身份而非可变内容。** Source 内容变化不能让合法的 AD-owned link 被误判为外部资源；人工修改 link target 必须使 ownership 失效。
  - Acceptance: source tree digest 改变后 ownership 仍有效；link text 或 source identity 改变后 fail closed。
  - Provenance: R3 与现有 ownership 安全不变量的共同要求。
- **R8 — UI 与文档反映共享更新语义。** 项目级“Update”不再表示独立 revision pin；需要迁移时显示 Relink/Update，需要修复时显示 Repair，source 更新预览展示所有受影响的关联项目。
  - Acceptance: 用户能区分 source refresh、legacy relink、broken-link repair 与 remove，产品规格不再声称项目独立 pin artifact revision。
  - Provenance: R3、R6 的可理解性要求。

### In scope

- Project Workspace 中由 Skill source catalog 安装到 Claude Code 或 Codex 的 Skills。
- Local source、Git source、source add/update/remove、install/relink/remove、ownership、receipt、rollback、legacy artifact-link 兼容和相关 UI/文档。
- User-scope Skill port 的共享 link primitive，确保其不重新引入复制路径；当前主要验收入口仍是 Project Workspace。

### Out of scope

- Plugin、MCP、hook 或 Agent package 的安装模型。
- Windows/Linux 支持；AD 当前发布目标为 macOS。
- 对 Local source 外部编辑提供内容级 rollback。
- 自动删除旧 immutable artifact 或历史 checkout generation；物理 GC 另行设计。
- 修改第三方 Skill 的运行权限或提供执行沙箱。

### Confirmed decisions and delegated defaults

- 产品决策：安装链接直接指向原始/受管 source checkout。
- 工程默认：Local 指向原始规范路径；Git 使用 source-scoped、稳定的 `current` checkout view。
- 工程默认：既有 artifact links 显式迁移，不在只读扫描或启动时自动改写。
- 工程默认：source 尚有 AD-owned links 时禁止 remove。
- Open decisions: none.

## Current behavior, constraints, and invariants

当前 source acquisition 在 `skill_artifacts.rs` 中把 Local tree 或临时 Git clone 复制进只读、按 digest 命名的 `~/.ad/artifacts/skills/<digest>/tree`。`SkillCatalogEntry.current_artifact` 记录这份 tree；`collection_actions::catalog_binding` 验证 artifact 后把具体 Skill path 交给 Claude/Codex Skills port。

Skills port 和 ExecutionEngine 已经把 Agent 侧目标写成 symlink，不会在项目目录复制 Skill tree。当前隔离发生在 link target：Project A/B 可以各自 pin 不同 artifact revision，source refresh 不改变已安装项目，项目必须逐个执行 Update 才会 retarget。

必须保留的不变量：

1. Agent 侧目标只能是精确解析出的 User/Project Skill path，并受 confinement 保护。
2. 非 AD-owned target、stale preview、手工 retarget、source 在 apply 前漂移时不得被覆盖。
3. Source acquisition/scan 继续拒绝逃逸 symlink、特殊文件、超限 tree 和不完整 `SKILL.md`。
4. Local source 是外部拥有的目录；AD 只能读取和链接，不能修改或删除。
5. Git checkout 是 AD-owned 数据；更新和删除必须经过 source catalog 的 plan/apply/recovery 边界。
6. Read-only inventory 不产生目录、链接、catalog migration 或 ownership mutation。
7. Link apply/rollback 改变 tree 后，旧 verification/review evidence 失效。

## Proposed structure and responsibilities

### 1. Replace install artifacts with source bindings

引入版本化 `SkillSourceBinding`，把“可安装路径”从 `current_artifact` 中分离：

```rust
struct SkillSourceBinding {
    schema_version: u32,
    source_id: String,
    source_kind: SkillSourceType,
    checkout_root: PathBuf,
    stable_root: PathBuf,
    observed_revision: String,
    tree_digest: ContentDigest,
    manifest_digest: ContentDigest,
    skills: Vec<SkillSourceItem>,
    activation_impact: SkillActivationImpact,
}

struct SkillSourceItem {
    logical_id: String,
    subpath: PathBuf,
    instruction_digest: ContentDigest,
}
```

- Local binding：`checkout_root == stable_root == canonicalize(location/subdirectory)`。
- Git binding：`checkout_root` 是当前 generation 的物理 tree；`stable_root` 是 `~/.ad/skill-library/<source-key>/current`。
- Install target：`stable_root.join(skill.subpath)`，而不是 artifact tree 或项目副本。
- Catalog 保存 source binding 与最近一次已确认 scan metadata；资源 inventory 在当前 source 上做只读复验并单独报告 drift/health。

`source-key` 必须从 backend-issued `source_id` 生成不透明、文件系统安全的稳定值，不使用 display name、URL 或未经处理的用户输入。

### 2. Local source lifecycle

Add/Refresh 继续在 operation-scoped staging 中检查规范 tree manifest、Skill discovery 与 activation impact，但不发布 tree 副本。Catalog commit 前重验 Local source revision；commit 后 binding 直接引用原始规范路径。

Local 内容由外部进程修改时：

- 已安装 link 无需变化即可读取新内容。
- 下一次 inventory 重新计算 health/digest；catalog snapshot 与 live source 不同时显示 `source_drifted`，但 link ownership 仍有效。
- “Refresh source”只更新 AD 的观察 metadata，不修改 Local tree，也不承诺回滚外部内容。
- Local root、Skill subpath 或 `SKILL.md` 消失时，资源为 degraded/broken；AD 可以 Repair/Remove link，但不能猜测新路径。

### 3. Git managed checkout lifecycle

Git source 使用 generation + stable view：

```text
~/.ad/skill-library/<source-key>/
  current -> generations/<revision-key>/tree
  generations/
    <revision-key>/
      tree/
      manifest.json
```

Add/Update 在唯一 staging 中执行受控 clone/fetch/checkout 和 tree validation。验证成功后把 generation 原子发布到 source root，再以 symlink Replace 原子切换 `current`。项目链接始终指向 `current/<skill-subpath>`，因此 current 切换会同时更新所有关联项目而不触碰项目目录。

Source Update preview 必须枚举 ownership index 中所有引用该 `source_id` 的 workspace，并明确这是共享影响。Apply 前重验 remote/ref、staged revision、catalog revision、current link digest 与 source root identity。失败时保持旧 `current`；current 已切换但 catalog/receipt 未完成时，startup recovery 根据 journal 完成或恢复旧 link。Rollback 将 `current` 恢复到前一 generation，同时影响所有关联项目，并在 receipt 中列出影响范围。

Source Update receipt 必须记录所有受影响 workspace 和切换前后 revision；这些 workspace 上绑定旧 source revision 的 verification/review evidence 统一失效，不能因为项目 link bytes 未变化而沿用。

切换前必须把 staged manifest 与所有关联 ownership 的 Skill subpath 做完整 join。任一已安装 subpath 在新 generation 中缺失、改变节点类型或不再包含有效 `SKILL.md` 时，source update 以 `source_update_breaks_installed_skill` fail closed，并列出受影响 workspace；用户需先移除这些安装或选择仍包含它们的 source revision，不能通过一次宽泛确认制造 dangling links。

旧 generation 在 receipt、rollback 或 legacy link 仍引用期间保留。本设计不实现 GC。

### 4. Installation and ownership

Claude/Codex port 继续生成 `application/vnd.ad.symlink` mutation，但 request 从自由 `path + artifactId` 收紧为 backend-issued source binding：source ID、Skill subpath、stable target、preview-time tree digest 和 binding revision。ExecutionEngine 仍在 apply 前检查 source 存在且与 preview 一致，防止计划期间漂移；成功后允许 source 内容继续变化。

Port 必须把“写入 link 的稳定 lexical target”与“用于安全校验的 canonical resolution”分开。Git binding 写入的 link text 始终是 `stable_root/<skill-subpath>`；验证时才 canonicalize 并确认它解析到当前受管 generation。不得复用当前 `install_source` 的“canonicalize 后返回路径”行为，否则项目 link 会直接指向 generation 并重新形成 revision pin。Local binding 的 lexical target 使用 source add 时记录的规范绝对路径，canonical resolution 用于 preview/apply revalidation。

Ownership schema v2 记录：

- workspace/resource/physical target identity；
- 精确 link target digest；
- `source_id`、source kind、stable root identity 与 Skill subpath；
- creating/updating receipt；
- 仅作诊断的 `observed_source_revision`，不把它当作 link ownership 条件。

验证 ownership 时要求 link text 与 source binding identity 完全匹配，并确认 target 落在该 binding 的允许 root 内。不得再因当前 source directory digest 与安装时不同而撤销 ownership。Content drift 进入 health，不进入 delete/replace authority。

### 5. Project actions and inventory

- **Install:** 未配置资源创建 checkout-target link 和 ownership v2。
- **Relink/Update:** 仅对 legacy artifact link 或 source binding target 版本不正确的资源可用；替换 link 并升级 ownership，可 rollback。
- **Repair:** link dangling、stable root 缺失或 checkout health 失败时提供；需要先恢复 source binding，不能用 Repair 覆盖外部 target。
- **Remove:** 删除当前 workspace 的 AD-owned link/相关 config，并删除 ownership，不触碰 Local source、Git checkout或其他 workspace。
- **Source Refresh/Update:** 更新 catalog observation 或 Git current view；不逐个重写项目 link。
- **Source Remove:** ownership index 存在引用时返回结构化阻塞与受影响 workspace；零引用时才允许移除 catalog binding，Git checkout 的物理删除另受 receipt/rollback 保留期约束。

同名不同 source 的 ResourceKey、冲突呈现和 external read-only 边界保持不变。

## Interfaces and data/control flow

### Install

```text
catalog inventory
  -> verify live SkillSourceBinding
  -> port resolves Agent target
  -> plan contains exact source binding + source/link preconditions
  -> confirmation
  -> ExecutionEngine writes one symlink
  -> ownership v2 + receipt
```

### Git source update

```text
source update preview
  -> stage managed checkout generation
  -> validate manifest + activation impact
  -> enumerate affected ownership records
  -> confirmation
  -> publish generation
  -> atomically switch source current link
  -> commit catalog snapshot + receipt
  -> all installed links resolve new content
```

### Local source drift

```text
external editor changes Local tree
  -> installed links immediately resolve changed bytes
  -> next inventory rescans source
  -> ownership remains AD-managed
  -> source health/snapshot reports drift or broken state
```

## Alternatives and rejected approaches

### Keep immutable artifact pinning

Rejected because it is the current behavior the product decision replaces. It requires per-project retargeting and does not make installed Skills follow the original/managed checkout.

### Link every project directly to a mutable Git working tree updated in place

Rejected because an interrupted pull/checkout can expose a partial tree to every project and content-level rollback is unreliable. A stable `current` view over validated generations preserves shared live semantics while making cutover atomic and recoverable.

### Rewrite every installed project link after a source update

Rejected because it turns one source operation into a multi-project distributed mutation with partial-failure states. Stable link targets let one source-scoped atomic switch update all consumers.

### Auto-rewrite legacy links during startup/inventory

Rejected because read-only discovery cannot create mutation authority, and silently changing existing projects would invalidate receipts and project evidence without preview.

## Compatibility, migration, and recovery

Catalog and ownership readers support schema v1 and v2 during migration. A v1 artifact link with valid ownership remains AD-managed and usable. Migration is explicit:

1. Materialize/validate the source binding without changing project links.
2. Show legacy installations as `Relink available`.
3. Relink preview binds old link digest, new stable target, workspace and source identity.
4. Apply replaces only that link and writes ownership v2.
5. Rollback restores the exact artifact link and ownership v1 while the referenced artifact is retained.

For a v1 Git catalog entry, its verified current artifact may seed the first managed generation without copying the tree again; `current` can initially reference that retained tree. A later Git update publishes a normal managed generation. For Local sources, binding resolves the current original path and verifies the Skill subpath still exists before offering Relink.

Recovery rules:

- Install/Relink failure uses existing symlink backup/compensation and ownership journal.
- Git update failure before `current` switch leaves consumers unchanged.
- Failure after switch uses source-update journal to finish catalog/receipt or restore the prior current target.
- Manual current/link retarget is never auto-adopted; inventory degrades it and requires an explicit repair/relink preview.
- Missing Local source cannot be recovered by AD; report the original path and allow Remove after ownership validation.

## Risks and verification approach

| Risk | Mitigation and evidence |
|---|---|
| Local source edits instantly change active Agent instructions | UI/source inventory labels Local links as live; drift health and source address remain visible; product security boundary states third-party Skills are not sandboxed |
| Git update changes many projects at once | source update preview lists all owned consumers; atomic `current` switch; one shared rollback restores all |
| Git revision removes an installed Skill | join staged manifest against every owned subpath and block the switch before publishing `current` |
| Mutable contents accidentally invalidate ownership | ownership v2 binds link/source identity; tests mutate contents while preserving ownership and reject target retarget |
| Source remove creates dangling links | reference check blocks removal while ownership records remain |
| Legacy artifacts are deleted too early | v1 links and rollback receipts pin referenced artifact/generation; no GC in this delivery |
| Stable view or project ancestor is swapped through symlink race | reuse held directory descriptors, no-follow traversal, target confinement and lock-time revalidation |
| Source adds escaping symlink or special file after initial validation | live inventory reports drift/degraded; add/update/relink preflight revalidates canonical manifest; execution sandboxing remains explicitly out of scope |
| UI continues promising independent project revisions | update product spec, source/action labels and A/B tests to assert shared source semantics |

Focused verification must cover:

1. Local install target equals original Skill path; editing source changes observed Skill bytes without link mutation or artifact copy.
2. Git A/B projects link to one stable checkout view; update and rollback change both atomically while project links remain byte-identical; a revision missing either installed subpath is rejected before cutover.
3. Ownership remains valid across source content drift and fails on link/source-ID/subpath tampering.
4. Source removal is blocked while any owned link exists; removing one project never changes another.
5. Legacy artifact link discovery, explicit Relink, rollback and restart recovery are idempotent.
6. Hostile source, dangling path, stale plan, external target, ancestor swap and interrupted Git update fail closed.
7. Claude/Codex project paths and conversion-installed Skills preserve platform parity.
8. Full frontend/Rust suites, strict lint/Clippy, production build and release bundle pass.

## Scope deltas and specialist evidence

- `technical-necessary` — stable Git checkout view and source-scoped rollback. Without it, R2/R3 exposes partially updated trees or requires unsafe multi-project link rewrites.
- `technical-necessary` — ownership v2. Without it, R3 makes every legitimate source edit invalidate AD management, failing R7.
- `technical-necessary` — source removal reference gate. Without it, R5 cannot prevent dangling installations.
- `technical-necessary` — explicit legacy Relink. Without it, R6 either strands existing installations or silently mutates them.
- `adjacent/deferred` — artifact/generation GC, source sharing across machines, Windows link variants, Local content rollback.
- Specialist evidence: not required. Existing execution confinement, catalog, ownership and migration implementations provide the required seams; no prototype is needed before planning.

## Open decisions

None.

## Review history and linked Plan

- Document review: `ready`; exact-digest report in `docs/design-docs/skill-source-checkout-links.review.md`.
- Human acceptance: accepted by the user with “lgtm” on 2026-08-05.
- Linked ImplementationPlan: `docs/plans/2026-08-05-001-feat-skill-source-checkout-links-plan.md`.

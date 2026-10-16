# Technical Design: Source-tracking Skill and Plugin installations

Design identity: source-tracking-installations-v1
Product Contract: `docs/product-specs/skill-plugin-resource-management.md`
Requirements covered: R3, R4, R6, R9, R10, R11, R13, R14
Authority: Product Contract confirmed by the user on 2026-09-03 and autonomous `ad-gallop` execution authority

## Current behavior, constraints, and invariants

AD already gives every managed Source a stable root: a local Source resolves to its original directory, while a Git Source resolves through AD's stable `current` binding. Project Skills and Claude project Plugins already use this stable path. User Skills are linked to immutable AD artifacts, user Plugins are copied into Agent caches, and Codex project Plugins are copied into an isolated runtime cache. Source update planning also blocks when an installed resource disappears. Those latter behaviors contradict R6 and R13.

AD may mutate only targets proven by ownership and installation records. External resources remain read-only. Source bytes are never rewritten. Runtime Agent differences remain inside adapters and must not surface as installation-mode choices.

## Decision summary and active design dimensions

1. `ResolvedCatalogResource.stable_path` is the only content locator for every `latest` installation. Local stable paths are live directories; Git stable paths follow the atomically switched Source binding.
2. Every new managed Skill or Plugin installation records `tracking_policy = latest`, the resolved Source revision, and the resolved content digest. These fields identify state but do not retain historical content.
3. Skills and Claude project Plugins remain direct links. Codex project Plugin cache entries become links to the stable resource path. User Plugin CLIs still perform native registration first; AD then replaces only the validated cache entry with a link to the stable path.
4. No frontend or IPC installation-mode option remains. “Install” has one semantic across Agents, resource kinds, and scopes.
5. Catalog resources that disappear remain identity tombstones. Inventories are the union of current catalog candidates and managed installation records, so a missing resource remains visible as `source_missing`, degraded, and removable.
6. Git update previews carry non-blocking missing-installation warnings and affected counts. Local Sources expose the same state on inspection. Missing resources never trigger automatic uninstall or migration to a new identity.
7. Existing AD-managed immutable/copy installations are upgraded idempotently to stable links. Conversion uses existing ownership evidence, confined target operations, and source identity checks; unavailable Sources remain visible and are retried after the Source becomes resolvable.

## Proposed structure and responsibilities

- Source catalog owns stable Source/resource identities, current revision metadata, tombstones, and update impact.
- Resource installation records own scope, Agent, resource identity, `latest` tracking policy, last resolved revision/digest, and ownership pointers.
- Agent adapters own the concrete link or launch indirection needed for their runtime.
- User Plugin management owns native CLI registration, cache-path discovery, safe cache linking, and link removal before native uninstall.
- Workspace inventories join catalog and installation records and own the `source_missing` presentation state.
- Startup/source-update reconciliation upgrades only legacy AD-owned targets; it never adopts or replaces external targets.

## Interfaces and data/control flow

`ResourceInstallationRecord` advances compatibly to schema v2 with `trackingPolicy`, `resolvedSourceRevision`, and `resolvedContentDigest`. Legacy v1 records decode as `latest` with absent resolved metadata and are backfilled when their Source is available.

`UserPluginManagementRecord` keeps its compatible v2 envelope but drops the user-selectable install mode. New records always store the stable Source path, linked cache path, `latest` policy, and resolved revision/digest. Legacy records remain readable and are reconciled.

`SkillCatalogPlanView` adds non-blocking `warnings`, including `source_update_missing_installations` with the affected installation count. `blockingIssues` remains reserved for unsafe or unreadable ownership, not ordinary Source removal/rename.

Resource inventories add `source_missing` as an effective state. Missing items retain their resource key, Source display metadata where safe, ownership proof, and only actions that remain truthful—inspect and remove.

## State, failure, compatibility, migration, security, and operations

Link replacement is allowed only when the target is missing or matches current AD ownership. Agent-reported Plugin cache paths must be canonical descendants of the selected Agent cache with matching marketplace and install identity. Ancestor traversal uses confined file-descriptor operations.

Migration is idempotent. A successful replacement updates ownership and installation metadata together; a failed replacement restores the prior AD-managed target when possible and leaves the old record retryable. Missing Sources are not fabricated and do not block startup. External or mismatched targets are reported, never overwritten.

Git Source publication remains atomic at the stable `current` binding. A successfully fetched and inspected candidate may advance even when resources disappear; the preview shows concise impact. Transport, checkout, corrupt state, or unreadable ownership failures still fail safely.

For Claude Plugin uninstall, AD removes its owned cache link before invoking the CLI so Claude cannot write orphan markers through the link into Source content. If the CLI fails before unregistering, AD restores the link.

## Alternatives and rejected approaches

- Installation-mode toggle: rejected because snapshot mode is hidden version pinning and contradicts the single current product semantic.
- Copy on every Source change: rejected because local edits would still require a watcher or manual synchronization and create stale copies.
- Global Source pin: deferred; future version policy belongs to each installation relation.
- Automatic uninstall for missing resources: rejected by R13; the installation remains visible and user-controlled.
- Hidden historical snapshot retention: rejected by R14.

## Risks and verification approach

The main risks are unsafe cache-path deletion, legacy record drift, Agent CLI format drift, and missing-resource inventory disappearance. Verification therefore requires confined-path adversarial tests, legacy migration fixtures, Local/Git update matrices, real Codex and Claude CLI cache-link/uninstall probes in isolated homes, frontend warning/state tests, and full regression/build checks.

## Scope deltas and specialist evidence

This design supersedes only installation-mode and stale-copy behavior covered by R6/R13/R14. Explicit Harness resource removal keeps the R10 cleanup workflow. No version selection, history retention, automatic install, automatic uninstall, or running-session hot reload is added.

## Open technical decisions

None.

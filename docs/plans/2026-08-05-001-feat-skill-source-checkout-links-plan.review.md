# Document Review: Skill source checkout links ImplementationPlan

Report: ad-document-review/v1
Status: `complete`
Document / digest: `IP-skill-source-checkout-links/v1` / `sha256:f86615a1208ae31d13033e62dab0936c740754c8de10a2f0e5104e66844d97af`
Mode: `non-interactive`
Reviewed: 2026-08-05
Verdict: `ready`

## Perspectives and coverage

- Design trace: mapped accepted design R1–R8 and D1–D5 to U1–U6 and Definition of Done.
- Implementation: checked catalog/binding, ports, execution, ownership, migration, frontend and docs seams against current files.
- Dependency/recovery: checked v1 reader before v2 writer, Local before Git, shared checkout before legacy migration, and backend schema before UI.
- Verification: checked focused public-seam tests, A/B/shared-update scenarios, full CI/build/release commands and no-match-test warning.
- Operations/security: checked temp-home isolation, Git argv/confinement, current-switch recovery, source removal, real-user-state prohibition and specialist triggers.
- Commit/completion: checked `delivery-only` policy against the explicit LFG authorization envelope, merge exclusion, honest skips, residuals and final exact-tree receipts.

## Findings

No unresolved finding remains for the reviewed digest.

## Applied edit batch before the ready pass

- **P1 — Relink/Repair labels lacked a backend authority contract.** Severity: high. Added backend-owned action intent while retaining stable Install/Update/Remove requests; Apply revalidates intent conditions and frontend cannot submit intent.
- **P2 — Breaking source updates/removals could fail only as opaque errors.** Severity: high. Added `applicability`, structured blockers and affected resources/workspaces to source plan views; blocked previews remain reviewable but cannot be claimed/applied.
- **P3 — Focused Rust filters could silently match no tests.** Severity: medium. Verification contract requires WorkReceipt coverage mapping and forbids treating a no-match success as evidence.
- **P4 — Multi-stage Product Contract lacked a standalone resolvable pointer.** Severity: medium. Persisted `PC-skill-source-checkout-links-2026-08-05-v1` and linked it from the Plan without changing R1–R8 semantics.
- **P5 — Commit policy was stale after entry through LFG.** Severity: high. Changed the Plan from `none` to `delivery-only`, bound authority to the explicit LFG invocation, and retained the no-merge/no-real-user-state exclusions.
- **P6 — Evidence commands lacked explicit roles and fallback limits.** Severity: high. Classified scenario/suite/specialist evidence as required, bundle/startup as preferred, and authorized only the backend-lifecycle plus packaged-startup fallback for the unavailable Tauri UI driver, with its fidelity limit recorded.

## Deferred decisions and residuals

- Artifact/generation GC, Local content rollback, cross-machine source distribution and non-macOS links remain accepted out-of-scope residuals.
- Delivery commit, push and PR create/update are authorized only after final verified/ready evidence; merge, deployment and unrelated external writes remain unauthorized.
- Implementation must not touch existing untracked `.agents/` or real `~/.ad`/Agent homes.

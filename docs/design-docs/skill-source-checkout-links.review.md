# Document Review: Skill source checkout links

Report: ad-document-review/v1
Status: `complete`
Document / digest: `TD-skill-source-checkout-links/v1` / `sha256:6f39bdd4ba2f82a4e77d82744ed071a8670a9d4db6ea412c7c2310ee4ad395d5`
Mode: `non-interactive`
Reviewed: 2026-08-05
Verdict: `ready`

## Perspectives and coverage

- Product/actor: traced the confirmed direct-link outcome through Local, Git, update propagation, source removal and legacy behavior.
- System design: traced source catalog → source binding → Agent port → ExecutionEngine → ownership and the source-update path through stable Git checkout generations.
- Implementation: checked the split between lexical link target and canonical validation, source/catalog schema evolution and existing Claude/Codex link primitives.
- Operations/recovery: checked install/relink rollback, Git current-switch recovery, shared impact enumeration and source-removal guards.
- Verification: checked observable Local/Git link behavior, A/B propagation, tamper detection, migration and hostile/race cases.
- Compatibility/migration: checked v1 artifact/catalog/ownership readers, explicit Relink and retention of legacy rollback targets.
- Security: checked target confinement, source validation, ownership authority, external Local source boundaries and mutable-source residual risk.
- Fresh-context comprehension: checked that the document distinguishes the existing Agent-side symlink from the requested change to its source target/lifecycle.

## Findings

No unresolved finding remains for the reviewed digest. The only changes from the previously ready digest were lifecycle metadata recording the user's acceptance and the linked Plan path; the accepted solution body is unchanged.

## Applied edit batch before the ready pass

- **D1 — Stable Git target could be canonicalized into a generation pin.** Severity: high. The design now requires separate lexical link targets and canonical safety resolution; Git project links preserve `current/<skill-subpath>` as link text.
- **D2 — Git update could remove an installed Skill and create dangling links.** Severity: high. The design now joins every installed ownership subpath against the staged manifest and blocks checkout cutover when any installed Skill is absent or invalid.
- **D3 — Shared source updates did not explicitly invalidate downstream evidence.** Severity: medium. The design now records all affected workspaces/revisions in the source-update receipt and invalidates revision-bound verification/review evidence.

## Deferred decisions and residuals

- Human design acceptance was supplied by the user with “lgtm” on 2026-08-05; acceptance remains owned by the TechnicalDesign, not this report.
- The linked ImplementationPlan may now be derived from the accepted design.
- Artifact/generation GC, Local content rollback and non-macOS link variants remain explicitly out of scope.

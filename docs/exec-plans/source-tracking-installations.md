# Implementation Plan: Make every managed installation follow Source current content

Product Contract: `docs/product-specs/skill-plugin-resource-management.md`
Technical Design: `docs/design-docs/source-tracking-installations.md`
Requirements: R3, R4, R6, R9, R10, R11, R13, R14
Commit policy / authority: none; user authorized working-tree delivery through `ad-gallop`, but did not authorize commit or remote publication

## Implementation decisions

- Use existing Source stable roots and ownership/confinement primitives; add no watcher and expose no installation mechanism.
- Preserve backward decoding for existing installation and user Plugin records.
- Keep explicit Harness resource removal behavior separate from Source-content disappearance.

## Scope deltas

No product delta beyond the confirmed Product Contract. Existing uncommitted installation-mode UI/API work is removed as superseded implementation, while the earlier target-Agent select and GUI CLI-resolution fixes remain in scope as already verified prerequisites.

## Implementation units

### U1 — Remove installation-mode choice and make new Skill installations source-tracking

- Requirements: R4, R6, R14
- Dependencies and accepted-design pointers: Technical Design decisions 1–4
- Affected modules and mutation: ResourceCenter/AgentCollectionPanel and IPC request schemas; Codex/Claude Skill adapters; user Skill ownership contracts.
- Entry / exit conditions: Product Contract accepted; exit when Local and Git Skills in project/user scopes target stable Source paths with no mode switch.
- Focused verification: frontend dialog/request tests and two-Agent Skill link/update tests.
- Recovery checkpoint: revert U1 paths without touching Source or Agent state.

### U2 — Make new Plugin installations source-tracking across both Agents and scopes

- Requirements: R3, R4, R6, R9
- Dependencies and accepted-design pointers: U1 contracts; Technical Design decisions 1–4 and security rules
- Affected modules and mutation: user Plugin plan/record/cache handling, Claude project launch links, Codex project runtime package targets and inherited package projections.
- Entry / exit conditions: stable resource paths available; exit when every managed Plugin runtime resolves through the stable Source path and native uninstall cannot mutate Source.
- Focused verification: adapter/unit tests plus isolated real Codex/Claude CLI install-link-uninstall probes.
- Recovery checkpoint: native CLI compensation plus confined restoration of the previous AD-managed target.

### U3 — Preserve and present installations whose Source resource disappears

- Requirements: R6, R13
- Dependencies and accepted-design pointers: installation identity from U1/U2
- Affected modules and mutation: source update impact contract/UI, resource catalog projection, project/user inventories, localized concise states.
- Entry / exit conditions: installation ledger can resolve identities independent of current presence; exit when update proceeds, no cleanup occurs, missing installations remain visible, and restore/new-resource semantics match R13.
- Focused verification: Local/Git deletion, rename, restore, new-resource, preview warning, and zero-write inventory tests.
- Recovery checkpoint: catalog update receipt and unchanged installation/ownership records.

### U4 — Add version-ready metadata and migrate legacy AD-managed copies

- Requirements: R6, R14
- Dependencies and accepted-design pointers: U1–U3 stable binding model
- Affected modules and mutation: resource installation/user Plugin schemas, startup/source-update reconciliation, ownership records and migration tests.
- Entry / exit conditions: legacy schemas remain readable; exit when available legacy copies become stable links idempotently, missing/external targets are preserved, and latest/revision metadata is backfilled without history retention.
- Focused verification: v1/v2 compatibility, repeated migration, failure restoration, missing Source, and hostile target fixtures.
- Recovery checkpoint: per-target confined replacement restores prior managed target before metadata publication.

## Verification contract

- Required: focused tests for all four units; full `pnpm test`, `pnpm lint`, `pnpm build`, `cargo fmt --check`, and `cargo test`.
- Required: real-browser evidence for concise Git update warning and persistent `source_missing` state.
- Required: security evidence for cache-path confinement, mismatched links, Source-inside-Agent-root rejection, and uninstall restoration.
- Preferred: real current Codex and Claude CLI probes in isolated temporary homes; if either CLI is unavailable, the pre-authorized fallback is fixture-based adapter evidence with the loss explicitly reported.
- Experiential acceptance: agent-run UI mechanics are required; final visual taste remains pending Product/Design and does not block local-complete.

## Risks and recovery

Legacy migration and Agent cache replacement are the highest-risk units. Mutate only AD-owned targets, publish metadata after target success, restore on pre-commit failure, and leave unresolved drift visible rather than guessing. Never delete Source content or external Agent resources.

## Definition of done

- One installation semantic—follow Source current—works for Local/Git, Skill/Plugin, Codex/Claude Code, and project/user.
- No installation-mode UI/API remains.
- Source deletion/rename advances, performs no automatic uninstall, and yields concise persistent missing states.
- New resources remain opt-in.
- Installation records are future-compatible with per-installation version policy without retaining history.
- Full verification and fresh code review pass on the same working-tree basis.

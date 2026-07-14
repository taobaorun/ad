# Multi-Agent Support Tasks

## Phase 1: Contract and compatibility

- [ ] Task 1: Define agent-neutral Rust models and IPC schemas
  - Acceptance: AgentId, capabilities, canonical installation, composite profile identity, conversion preview and issue types round-trip across Rust/TypeScript.
  - Verify: Rust model tests, TypeScript schema tests, old Claude profile fixture parses.
  - Files: src-tauri/src/models.rs, src/lib/agentTypes.ts, tests/schema.test.ts
  - Dependencies: None

- [ ] Task 2: Add adapter registry and canonical discovery deduplication
  - Acceptance: built-in adapters register statically; duplicate discovery results produce one canonical installation.
  - Verify: Rust unit tests with duplicate paths and normalized roots.
  - Files: src-tauri/src/agents/registry.rs, src-tauri/src/agents/types.rs, src-tauri/src/agents/discovery.rs, src-tauri/tests/agents.rs
  - Dependencies: Task 1

### Checkpoint A

- [ ] Existing test suite passes.
- [ ] No implementation changes to Codex behavior begin before the contract is stable.

## Phase 2: Claude and Codex parity

- [ ] Task 3: Wrap existing Claude behavior in Claude adapter
  - Acceptance: settings, skills, plugins, process detection and terminal operations are exposed through capabilities without regressions.
  - Verify: existing Rust and frontend tests plus legacy activation manual check.
  - Files: src-tauri/src/agents/claude.rs, src-tauri/src/commands/agents.rs, src/lib/tauri.ts, tests/agents.test.ts
  - Dependencies: Task 1, Task 2

- [ ] Task 4: Implement Codex discovery and configuration operations
  - Acceptance: canonical installation discovery and user/project configuration read/write work against fixtures and confirmed local paths.
  - Verify: isolated AD_HOME tests, permission/error cases, manual macOS inspection.
  - Files: src-tauri/src/agents/codex.rs, src-tauri/src/agents/codex_config.rs, src-tauri/src/fs/paths.rs, src-tauri/tests/codex.rs
  - Dependencies: Task 2

- [ ] Task 5: Implement Codex skills, plugins, process detection and terminal operations
  - Acceptance: all five capabilities are declared and callable; errors are structured and actionable.
  - Verify: capability matrix tests and manual end-to-end checks.
  - Files: src-tauri/src/agents/codex_capabilities.rs, src-tauri/src/commands/agents.rs, src-tauri/src/terminal/mod.rs, src-tauri/tests/codex_capabilities.rs
  - Dependencies: Task 4

### Checkpoint B

- [ ] Claude and Codex capability matrix is complete.
- [ ] Both adapters pass isolated filesystem tests.

## Phase 3: Conversion safety

- [ ] Task 6: Build Claude-to-Codex conversion preview
  - Acceptance: source is read-only; target document, diff and unsupported fields are returned without writes.
  - Verify: fixture tests prove source bytes remain unchanged.
  - Files: src-tauri/src/agents/conversion.rs, src-tauri/src/models.rs, src/lib/agentTypes.ts, src-tauri/tests/conversion.rs
  - Dependencies: Task 3, Task 4

- [ ] Task 7: Add target backup, atomic apply and rollback
  - Acceptance: existing target is backed up only after confirmation; failed apply restores target; source is never changed.
  - Verify: injected write failure and rollback integration tests.
  - Files: src-tauri/src/agents/conversion_apply.rs, src-tauri/src/fs/atomic.rs, src-tauri/src/commands/agents.rs, src-tauri/tests/conversion_rollback.rs
  - Dependencies: Task 6

### Checkpoint C

- [ ] Conversion preview and apply are safe in temporary projects.
- [ ] History records source agent, target agent, operation and backup paths.

## Phase 4: UI and release

- [ ] Task 8: Add Agent-aware stores and selectors
  - Acceptance: project/profile state is isolated by composite identity and capability state is available to UI.
  - Verify: React store tests and TypeScript typecheck.
  - Files: src/store/agents.ts, src/store/profiles.ts, src/store/projects.ts, src/lib/tauri.ts
  - Dependencies: Task 3, Task 4

- [ ] Task 9: Add capability-gated parity UI and conversion flow
  - Acceptance: settings, skills, plugins, process and terminal actions are available for both agents; conversion preview/confirm/rollback is visible.
  - Verify: component tests and manual two-window flow.
  - Files: src/components/AgentSelector.tsx, src/components/ConversionDialog.tsx, src/components/ProjectDetail.tsx, src/SettingsApp.tsx, src/i18n/locales/{zh,en}.json
  - Dependencies: Task 5, Task 7, Task 8

- [ ] Task 10: Update docs and complete release verification
  - Acceptance: architecture and product indexes describe multi-agent behavior; all checks pass; plan archived.
  - Verify: pnpm typecheck && pnpm lint && pnpm test && cargo test --manifest-path src-tauri/Cargo.toml && pnpm build
  - Files: docs/design-docs/architecture.md, docs/product-specs/index.md, docs/exec-plans/active/multi-agent-support.md, docs/exec-plans/active/multi-agent-support.html
  - Dependencies: Task 9

### Checkpoint D

- [ ] All acceptance criteria pass.
- [ ] Human reviews the final diff before archive.


# Multi-Agent v1 Tasks

## Phase 1: v1 Contract

- [x] Task 1: Define typed identities and Agent context
  - Acceptance: AgentId, InstallationId, ProfileId, PlanId and ReceiptId are distinct newtypes; AgentDefinition, AgentInstallation and AgentContext round-trip through serde.
  - Verify: failing-first Rust tests prove IDs are serialized predictably and context preserves optional project scope.
  - Files: `src-tauri/src/agents/types.rs`, `src-tauri/src/agents/mod.rs`
  - Dependencies: None

- [x] Task 2: Define resource and operation contracts
  - Acceptance: ResourceRef/Snapshot, MutationPlan, OperationReceipt and AgentError represent read/write preconditions and partial outcomes without Agent-specific fields.
  - Verify: Rust round-trip tests and invariant tests for source/read-only resources.
  - Files: `src-tauri/src/agents/types.rs`, `src-tauri/src/agents/operations.rs`
  - Dependencies: Task 1

- [x] Task 3: Replace declared capabilities with capability ports
  - Acceptance: AgentAdapter exposes optional Settings/Skills/Plugins/Process/Launch ports; public descriptors are derived from present ports.
  - Verify: a fake adapter cannot expose a capability descriptor without returning its port; scope/operation descriptors round-trip.
  - Files: `src-tauri/src/agents/registry.rs`, `src-tauri/src/agents/capabilities.rs`, `src-tauri/src/agents/{claude,codex}.rs`
  - Dependencies: Tasks 1-2

- [x] Task 4: Add v1 TypeScript boundary schemas
  - Acceptance: frontend validates identities, contexts, resources, descriptors, plan views, receipts and structured errors without using unsafe casts.
  - Verify: `pnpm test -- tests/lib/agentTypes.test.ts` and `pnpm typecheck`.
  - Files: `src/lib/agentTypes.ts`, `tests/lib/agentTypes.test.ts`
  - Dependencies: Tasks 1-3

### Checkpoint A

- [x] Targeted Rust and TypeScript tests pass.
- [x] Existing v0 commands and profile fixtures still pass.
- [x] No v1 operation writes user files.

## Phase 2: Canonical Context

- [x] Task 5: Move canonical identity into adapters
  - Acceptance: candidates sharing an effective config home deduplicate; different config homes remain distinct; aliases are not persisted.
  - Verify: AD_HOME fixtures for default home, CODEX_HOME, trailing separators and duplicate discovery evidence.
  - Files: `src-tauri/src/agents/discovery.rs`, `src-tauri/src/agents/registry.rs`, `src-tauri/src/agents/{claude,codex}.rs`
  - Dependencies: Task 3

- [x] Task 6: Expose and persist AgentContext
  - Acceptance: IPC and Zustand select installation + optional project while migrating legacy `activeAgentId` state.
  - Verify: Rust command tests, store tests and typecheck.
  - Files: `src-tauri/src/commands/agents.rs`, `src/lib/tauri.ts`, `src/store/agents.ts`, `tests/store/agents.test.ts`
  - Dependencies: Tasks 4-5

## Phase 3: Safe Execution

- [ ] Task 7: Wrap Claude behavior with v1 ports
  - Acceptance: existing settings, skills, plugins, process and launch behavior is reachable through capability ports with unchanged legacy IPC behavior.
  - Verify: existing Claude tests plus port contract tests.
  - Files: `src-tauri/src/agents/claude.rs`, `src-tauri/src/agents/claude_ports.rs`, `src-tauri/src/commands/agents.rs`
  - Dependencies: Task 3

- [ ] Task 8: Add backend-owned plan store and digest checks
  - Acceptance: preview returns a plan view; apply accepts planId only; changed read/write resources invalidate the plan.
  - Verify: unit tests for expiry, unknown plan, replay and target-changed conflict.
  - Files: `src-tauri/src/agents/plan_store.rs`, `src-tauri/src/agents/operations.rs`, `src-tauri/src/commands/agents.rs`
  - Dependencies: Tasks 2, 7

- [ ] Task 9: Implement shared safe execution
  - Acceptance: all backups complete before writes; atomic writes create receipts; failures compensate in reverse order and report partial outcomes.
  - Verify: AD_HOME fault-injection integration tests.
  - Files: `src-tauri/src/agents/execution.rs`, `src-tauri/src/fs/atomic.rs`, `src-tauri/tests/agent_execution.rs`
  - Dependencies: Task 8

## Phase 4: Codex Parity

- [ ] Task 10: Implement Codex settings resources
  - Acceptance: user/project config scopes are inspected and planned with TOML unknown-field preservation; sensitive runtime files are excluded.
  - Verify: Codex fixture tests and real local read-only inspection.
  - Files: `src-tauri/src/agents/codex.rs`, `src-tauri/src/agents/codex_settings.rs`, `src-tauri/tests/codex_settings.rs`
  - Dependencies: Tasks 5, 9

- [ ] Task 11: Implement Codex skills and plugins ports
  - Acceptance: list/install/enable/disable workflows reflect actual Codex scopes and return structured limitations where authorization is required.
  - Verify: isolated filesystem/CLI fixture tests.
  - Files: `src-tauri/src/agents/codex_skills.rs`, `src-tauri/src/agents/codex_plugins.rs`, `src-tauri/tests/codex_extensions.rs`
  - Dependencies: Tasks 5, 9

- [ ] Task 12: Generalize process detection and terminal launch
  - Acceptance: adapters return process match specs and launch recipes; terminal command has no Claude-specific parameters.
  - Verify: process matcher unit tests and terminal backend tests.
  - Files: `src-tauri/src/agents/runtime.rs`, `src-tauri/src/commands/terminal.rs`, `src-tauri/src/terminal/`
  - Dependencies: Tasks 3, 7

- [ ] Task 13: Prove operation-level parity
  - Acceptance: Claude and Codex satisfy the required settings/skills/plugins/process/launch user journeys or report explicit degraded limitations.
  - Verify: shared adapter contract suite.
  - Files: `src-tauri/tests/agent_parity.rs`
  - Dependencies: Tasks 10-12

## Phase 5: Profiles and Conversion

- [ ] Task 14: Introduce AgentProfile envelope
  - Acceptance: adapter-owned payload schemas coexist with legacy Claude ProfileFile reads; profiles remain keyed by `(agentId, profileId)`.
  - Verify: migration fixtures and frontend schema tests.
  - Files: `src-tauri/src/agents/profiles.rs`, `src-tauri/src/commands/profiles.rs`, `src/lib/profileSchema.ts`
  - Dependencies: Tasks 4, 7, 10

- [ ] Task 15: Build artifact conversion route
  - Acceptance: conversion reports exact/mapped/requires_input/unsupported/conflict/unchanged per artifact and never puts source resources in write-set.
  - Verify: source/target fixture tests and invariant tests.
  - Files: `src-tauri/src/agents/conversion.rs`, `src-tauri/src/agents/conversion_route.rs`
  - Dependencies: Tasks 10-11, 14

- [ ] Task 16: Apply and rollback conversion plans
  - Acceptance: confirmed plans use shared execution; rollback checks current digests and never overwrites post-apply user changes.
  - Verify: integration tests with existing target, external modification and injected failure.
  - Files: `src-tauri/src/commands/agents.rs`, `src-tauri/src/agents/execution.rs`, `src-tauri/tests/conversion_execution.rs`
  - Dependencies: Tasks 9, 15

## Phase 6: UI and Release

- [ ] Task 17: Migrate stores and common UI to v1 descriptors
  - Acceptance: AgentContext drives profile/project state; common actions use descriptors; Agent-specific editors are registered centrally.
  - Verify: store/component tests, typecheck and browser runtime check.
  - Files: `src/store/`, `src/components/`, `src/lib/tauri.ts`
  - Dependencies: Tasks 6, 13, 14

- [ ] Task 18: Complete conversion UI and i18n
  - Acceptance: preview, issues, confirmation, receipt and rollback states are available in zh/en with no hardcoded user strings.
  - Verify: component tests, locale-key parity and browser flow.
  - Files: `src/components/ConversionDialog.tsx`, `src/i18n/locales/{zh,en}.json`
  - Dependencies: Tasks 16-17

- [ ] Task 19: Release verification and documentation closure
  - Acceptance: all checks pass; as-built design/product docs match code; ExecPlan result review is complete and MD + frozen HTML are archived together.
  - Verify: `pnpm typecheck && pnpm lint && pnpm test && cargo test --manifest-path src-tauri/Cargo.toml && pnpm build` plus macOS manual acceptance.
  - Files: `docs/`, `tasks/plan.md`, `tasks/todo.md`
  - Dependencies: Task 18

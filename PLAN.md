# Plan: cc-switch Implementation

> Source spec: [`SPEC.md`](./SPEC.md). This plan breaks the spec into **8 milestones** at 1–2 days each. Each milestone is an independently verifiable checkpoint. After Phase 3 (`TASKS.md`) we further decompose each milestone into ≤ 5-file tasks.

---

## Dependency Graph

```
        M0 ── scaffold
         │
         ▼
        M1 ── fs & models
         │
         ▼
        M2 ── profile CRUD + migration
         │
    ┌────┴────┐
    ▼         ▼
   M3        M4 ── activate + backup + notify
 editor       │
  + diff      │
    │    ┌────┼────┐
    ▼    ▼    ▼    ▼
        M5    M6   M7
     history menu import
              │
              ▼
             M8 ── sign + notarize + release
```

**Critical path** (sequential): `M0 → M1 → M2 → M4 → M6 → M8`.
**Parallelizable**: `M3 ⊥ M4`, then `M5 ⊥ M6 ⊥ M7`.

- Solo, single thread: ≈ **12 working days**
- With subagent fan-out at the parallel forks: ≈ **9 working days**

---

## Milestone M0 — Scaffolding & Toolchain (≈ 1 day)

**Goal**: An empty Tauri 2 + React + TS app that builds, lints, tests, and opens.

**Scope**

- `pnpm create tauri-app` with React-TS template; replace template UI with a placeholder.
- Add: Tailwind, shadcn/ui generator, lucide-react, zustand, zod, Monaco (`@monaco-editor/react`), @tauri-apps/api v2.
- Configure: `tsconfig.json` paths (`@/* → src/*`), `vite.config.ts` alias, ESLint + Prettier, Vitest + RTL, `cargo fmt` + `clippy` settings.
- `package.json` scripts per `SPEC.md §3` (without `release:mac` yet — M8).
- `.github/workflows/ci.yml` running lint + typecheck + frontend test + `cargo clippy -- -D warnings` + `cargo test` on macOS-latest. **No release job yet.**
- Tauri config narrowed to `macos` only.

**Acceptance**

- `pnpm tauri dev` opens an empty window with the title "cc-switch" on macOS.
- `pnpm lint`, `pnpm typecheck`, `pnpm test`, `cargo clippy`, `cargo test` all green.
- CI passes on a throwaway PR.

**Verification**

```bash
pnpm install
pnpm tauri dev          # window opens, then Ctrl-C
pnpm lint && pnpm typecheck && pnpm test --run
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

**Risks**

- Tauri 2 + pnpm + Vite can produce stale lockfile collisions on first init. **Mitigation**: pin exact Tauri 2 minor version in `Cargo.toml` and `package.json`.

---

## Milestone M1 — Filesystem Primitives & Data Model (≈ 1 day)

**Goal**: All path resolution, atomic writes, and the canonical types exist and are tested. No UI yet.

**Scope**

- `src-tauri/src/fs/paths.rs`: `claude_dir()`, `profiles_dir()`, `cc_switch_dir()`, `backups_dir()`, `claude_settings_path()`, `history_path()`, `legacy_dir()`. Resolve via `dirs::home_dir()`; never hard-code `~/`.
- `src-tauri/src/fs/atomic.rs`: `write_atomic(path, &[u8])` — write to `path.with_extension("tmp.<uuid>")`, `fsync`, `rename`. Property: a crash mid-write leaves the original untouched.
- `src-tauri/src/models.rs`: `ProfileFile`, `ClaudeSettings`, `ActivationResult`, `ActivationLogEntry` — serde with `#[serde(rename_all = "camelCase")]`.
- `src/lib/profileSchema.ts`: zod schema with **identical** field shape; export `ProfileFile` type.
- `src/lib/claudeSettings.ts`: TS types for the `settings` block (env, permissions, hooks, model, theme; everything else `Record<string, unknown>` for forward-compat).
- Rust unit tests using `tempfile::TempDir`: atomic write happy path, atomic write fails cleanly, paths resolve correctly.
- One TS test asserting zod schema rejects a malformed sample and accepts the existing `homi.json`.

**Acceptance**

- 100% of `fs::atomic` and `fs::paths` covered.
- A round-trip test deserializes a real `~/.claude/profiles/homi.json` (copy fixture into `src-tauri/tests/fixtures/`) into `ProfileFile` and back.
- Rust ↔ TS schema parity asserted by checking that `serde_json::to_string(&ProfileFile::sample())` parses cleanly through the zod schema (fixture-based, run as a Vitest snapshot).

**Verification**

```bash
cargo test --manifest-path src-tauri/Cargo.toml fs::
cargo test --manifest-path src-tauri/Cargo.toml models::
pnpm test src/lib/profileSchema
```

**Risks**

- Schema drift between Rust serde and TS zod. **Mitigation**: M1 ships a `schema-parity.test.ts` that loads a JSON fixture produced by Rust and validates it through zod. Fixture regenerated whenever models change.

---

## Milestone M2 — Profile CRUD + Legacy Migration (≈ 2 days)

**Goal**: User can list, create, edit (raw), and delete profiles. On first launch, existing `~/.claude/profiles/*.json` are migrated.

**Scope**

- `src-tauri/src/commands/profiles.rs`: `list_profiles`, `get_profile`, `save_profile`, `delete_profile`. All go through `fs::atomic`.
- Migration routine: on app start, scan `profiles_dir()`. Files matching the legacy `{ displayName, env }` shape get wrapped as `{ id: <stem>, displayName, settings: { env } }` and re-saved. Originals moved to `profiles_dir()/.legacy/` with a `MIGRATION.md` explaining the move.
- `src/store/profiles.ts`: zustand store with `profiles`, `activeId`, `selectedId`, `loadAll()`, `save(profile)`, `delete(id)`.
- `src/lib/tauri.ts`: typed `invoke` wrappers.
- `src/components/ProfileList.tsx`: sidebar list, "+ New" button, delete affordance, no editor yet (placeholder pane).
- Component tests with `vi.mock('@tauri-apps/api/core')` returning fixture data.
- One Rust integration test that creates a temp `HOME`, drops legacy fixtures, runs migration, asserts the rewrites.

**Acceptance**

- On a Mac with existing `~/.claude/profiles/{homi,alibaba}.json`, launching the app shows both profiles in the sidebar with their `displayName`.
- Originals preserved under `~/.claude/profiles/.legacy/` (read-only check, no deletion).
- "+ New" creates a profile named "Untitled" with empty `settings`. Delete asks for confirmation.

**Verification**

- Manual: Backup `~/.claude/profiles/`, run app, observe migration, restore originals.
- `cargo test commands::profiles::tests::migration_round_trip`.
- `pnpm test src/components/ProfileList`.

**Risks**

- Destructive migration. **Mitigation**: migration is idempotent (re-running on already-migrated files is a no-op via shape detection); originals preserved in `.legacy/` (per `SPEC.md §13` "Never delete a backup file"). Migration unit test runs against a `tempfile`-backed fake home.

---

## Milestone M3 — Editor, Diff View, Validation (≈ 2 days, ⊥ M4)

**Goal**: Selecting a profile opens a Monaco JSON editor with schema validation; a "Diff vs current" view shows what activation would change.

**Scope**

- `src/components/ProfileEditor.tsx`: Monaco-based JSON editor wired to the selected profile's `settings`. Inline error markers from zod validation results.
- `src/components/DiffView.tsx`: Monaco `DiffEditor` between selected profile's `settings` and the live `~/.claude/settings.json` (read via a new `read_current_settings` command).
- "Save" button (disabled when invalid). Save round-trips through `save_profile`.
- Bundle-size measurement: log Monaco's contribution to the final bundle in CI; fail the M3 PR if the dev bundle exceeds 8 MB JS uncompressed (Monaco is the dominant cost).
- Component tests: editor renders, validation errors render, diff view fetches current settings, "Save" calls store action.

**Acceptance**

- Editing `homi`'s `ANTHROPIC_BASE_URL`, saving, then re-opening shows the persisted change.
- Introducing invalid JSON disables Save and surfaces an inline error.
- Diff view shows added/removed/changed keys against the current `~/.claude/settings.json`.

**Verification**

```bash
pnpm test src/components/ProfileEditor src/components/DiffView
pnpm tauri dev   # manual: edit-save-reopen cycle on homi
```

**Risks**

- Monaco bundle bloat in Tauri's WebView. **Mitigation**: import Monaco from `@monaco-editor/react` with `loader: { paths: { vs: '/monaco/vs' } }` and self-host only the JSON language to skip TS/JS tokenizers.

---

## Milestone M4 — Activate + Backup + Restart Notification (≈ 2 days, ⊥ M3)

**Goal**: Clicking "Activate" overwrites `~/.claude/settings.json` safely, backs up the prior contents, appends to history, sends a native macOS notification, and shows running `claude` PIDs in a toast.

**Scope**

- `src-tauri/src/commands/activate.rs`: `activate_profile(id)` →
  1. Read current `~/.claude/settings.json` (if present), write to `~/.claude/cc-switch/backups/<ISO8601>.json`.
  2. `write_atomic(claude_settings_path(), serde_json::to_vec_pretty(&profile.settings)?)`.
  3. Append `ActivationLogEntry` to `history.jsonl`.
  4. Return `{ activated_id, backup_path, detected_pids }`.
- `src-tauri/src/commands/processes.rs`: `detect_claude_processes()` — uses `sysinfo` crate (cross-process safe, no shell-out). Filters processes with executable basename `claude` or `claude-code`.
- Native notification via `tauri-plugin-notification` v2.
- Frontend: "Activate" button on the editor pane; success → toast component listing PIDs and a "Copy restart command" button (`pkill -f claude && claude` template; user must opt-in to actually run).
- Crash-invariant test: spawn a process that calls `write_atomic` and `kill -9`s itself before `rename`. Assert the original file is byte-identical.
- Tests:
  - Rust: `activate_profile` on a `tempfile` HOME — backup created, settings updated, history line appended.
  - Rust: `detect_claude_processes` returns at least the running test binary's parent (sanity check).
  - Frontend: toast renders; clicking "Activate" calls the command and shows a success notification stub.

**Acceptance**

- Activating `homi` overwrites a synthetic `~/.claude/settings.json` test fixture; original restorable from the timestamped backup.
- macOS notification appears within 500 ms of click.
- Running `claude --help &` in another terminal causes the toast to list the PID.
- Activation end-to-end < 200 ms (per `SPEC.md §10`).

**Verification**

```bash
cargo test commands::activate
cargo test commands::processes
pnpm test src/components/ActivateToast
pnpm tauri dev   # manual: activate, observe notification + toast
```

**Risks**

- macOS notification permission prompt may suppress first notification. **Mitigation**: on first launch, request `notification` permission via `tauri-plugin-notification`; show an inline banner if denied with a link to System Settings.
- `sysinfo` performance on busy systems. **Mitigation**: refresh only `processes_with_kind(Process)` once per activation; don't poll continuously.

**Decision needed during M4**: confirm the exact "Copy restart command" template. Default proposal: `pkill -f "claude " && exec $SHELL -l`. Reject this default if you usually run Claude Code via a different launcher (e.g., a wrapper script).

---

## Milestone M5 — History Panel + Rollback (≈ 1 day, ⊥ M6, M7)

**Goal**: Last 50 activations visible; one-click rollback restores any prior backup.

**Scope**

- `src-tauri/src/commands/history.rs`: `read_history(limit: usize)`, `restore_backup(backup_path: PathBuf)` — `restore_backup` takes the same atomic-write + new-backup path as `activate_profile` (so rolling back also creates a backup of the now-discarded state).
- `src/components/HistoryPanel.tsx`: list view with timestamp, from→to, "Restore" button.
- Tests: rolling back twice yields the original state.

**Acceptance**

- `homi → alibaba → restore` results in `~/.claude/settings.json` byte-identical to the pre-`alibaba` state.
- History panel shows all transitions, newest first.

**Verification**

```bash
cargo test commands::history
pnpm test src/components/HistoryPanel
```

**Risks**

- Backup file naming collisions if two activations happen in the same second. **Mitigation**: include milliseconds in ISO8601 (`2026-05-13T22-55-30.123Z`); fall back to a 4-char random suffix on collision.

---

## Milestone M6 — Menubar Tray (≈ 2 days, ⊥ M5, M7)

**Goal**: A macOS menubar icon shows the active profile's color; clicking opens a dropdown of profiles; clicking a profile activates it via the same pipeline as the main UI.

**Scope**

- Tauri 2 `tray::TrayIconBuilder` in `src-tauri/src/main.rs`.
- Dynamic tray icon: regenerate a 16×16 PNG on startup and on every activation (colored circle from `profile.color`).
- Menu items: profile list (with checkmark on active), separator, "Show cc-switch", "Quit".
- Menu click handler invokes the same `activate_profile` command.
- Tests: tray creation is integration-tested behind `#[cfg(test)]` with a mock event loop. (Tray UI is hard to E2E; we settle for "the click handler dispatches the same command path".)

**Acceptance**

- Menubar icon visible at all times; reflects active profile's color.
- Activating from the menu produces the same notification + history entry as activating from the main window.
- Quit from menu fully exits the app (no leftover processes).

**Verification**

- Manual: switch profiles from menubar and from main UI alternately; confirm history shows both.
- `cargo test tray::handler::tests`.

**Risks**

- Tauri 2's tray API has had minor version churn. **Mitigation**: pin Tauri to a specific minor version in `Cargo.toml`; document the chosen version in `PLAN.md` once selected.

---

## Milestone M7 — Import (file / URL / gist) (≈ 1 day, ⊥ M5, M6)

**Goal**: User can import a profile from a local JSON file, a raw URL, or a GitHub gist.

**Scope**

- `src-tauri/src/commands/importers.rs`: `import_from_file(path)`, `import_from_url(url)`. URL fetch uses `reqwest` with a 5s timeout, max body size 1 MB, only accepts `application/json`.
- `import_from_url` recognizes `https://gist.github.com/<user>/<id>` and rewrites to the raw URL of the first JSON file in the gist (via the GitHub Gists API, unauthenticated).
- `src/components/ImportDialog.tsx`: tabbed dialog (File / URL / Gist).
- Validate via zod before saving; reject if `id` collides (offer rename).
- Tests: importer parses our own `homi.json`; URL importer rejects non-JSON content-type.

**Acceptance**

- Importing `~/.claude/profiles/.legacy/homi.json` produces a new profile.
- Importing a malformed file shows a clear error and does not write anything.

**Verification**

```bash
cargo test commands::importers
pnpm test src/components/ImportDialog
```

**Open question (now needs a call)**: Import URL format — accept any well-formed `ProfileFile` JSON, or require a wrapper `{ "version": "1", "profile": {...} }`?
**Recommendation**: accept any well-formed `ProfileFile`. A version field can be added later without breaking existing imports if absent ⇒ treated as v1.

---

## Milestone M8 — Code Signing, Notarization, Release Pipeline (≈ 2 days)

**Goal**: A `git tag v0.1.0` push produces a notarized, stapled `.dmg` published as a GitHub Release.

**Scope**

- `tauri.conf.json` macOS section: `signingIdentity`, hardened-runtime entitlements (`com.apple.security.cs.allow-jit` only if Monaco demands; verify), notarization via App Store Connect API key.
- `pnpm release:mac` script: `tauri build` → `codesign --verify --deep --strict` → `xcrun notarytool submit --wait` → `xcrun stapler staple`.
- `.github/workflows/release.yml`: triggered on `v*` tag, runs on `macos-latest`, secrets: `APPLE_API_KEY`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`, `APPLE_SIGNING_IDENTITY` (base64), `APPLE_SIGNING_PASSWORD`. Uploads `.dmg` to the release.
- DMG cosmetics (background, icon position) via `tauri.conf.json` `dmg` block.
- Smoke test: download the produced DMG on a clean Mac VM (or another machine), verify Gatekeeper accepts it without "unidentified developer" warning.

**Acceptance**

- `git tag v0.1.0 && git push --tags` produces a release with a notarized `.dmg`.
- Installing the `.dmg` on a fresh Mac does not show Gatekeeper warnings.
- `spctl -a -vvv /Applications/cc-switch.app` reports `accepted` and `Notarized Developer ID`.

**Verification**

- Manual install on a second Mac (or `nuke-and-pave` VM image).
- Workflow logs show notarization status `Accepted`.

**Risks**

- App Store Connect API key handling. **Mitigation**: store as a base64 GitHub secret; document key generation in `docs/release.md`; never log the decoded value.
- First-run permission prompts (Notifications, Accessibility if any). **Mitigation**: declare usage strings in `Info.plist`; document the expected prompts in the README.

**Open question (now needs a call)**: Tauri auto-update — in v0.1.0 or wait for v0.2.0?
**Recommendation**: defer to v0.2.0. Auto-update requires an additional update-server endpoint and a signing key for update bundles; cuts complexity from the release pipeline.

---

## Cross-cutting verification checkpoints

After each milestone, before claiming "done":

```bash
pnpm lint && pnpm typecheck && pnpm test --run
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

After M4, M5, M6 (any milestone touching activation):

```bash
# Crash-safety property test
cargo test --release --manifest-path src-tauri/Cargo.toml \
  fs::atomic::tests::crash_during_write_preserves_original
```

After M3, M6 (any UI-visible milestone):

- Run `pnpm tauri dev`, exercise the changed surface manually, confirm bundle size still under target.

---

## Risk Register (consolidated)

| #   | Risk                                                  | Likelihood | Impact       | Mitigation                                                             | Owner milestone |
| --- | ----------------------------------------------------- | ---------- | ------------ | ---------------------------------------------------------------------- | --------------- |
| R1  | Rust ↔ TS schema drift                               | Med        | High         | Fixture-based parity test                                              | M1              |
| R2  | Migration corrupts existing profile files             | Low        | **Critical** | Idempotent shape detection + originals to `.legacy/` + temp-HOME tests | M2              |
| R3  | Monaco bundle bloat                                   | High       | Med          | Self-host only JSON language; CI bundle-size gate                      | M3              |
| R4  | macOS notification permission denied → silent failure | Med        | Med          | First-run permission request + denial banner                           | M4              |
| R5  | Mid-write crash leaves half-written settings          | Low        | **Critical** | Atomic write (temp+fsync+rename); crash-injection test                 | M1, M4          |
| R6  | Tauri 2 tray API churn                                | Med        | Med          | Pin Tauri minor version; document chosen version                       | M6              |
| R7  | Notarization credential leak                          | Low        | High         | Base64 GH secrets; never log; rotation doc                             | M8              |
| R8  | First-Mac install flagged by Gatekeeper               | Med        | High         | Smoke-test install on clean Mac before tagging                         | M8              |

---

## Decisions locked 2026-05-13

- **M4 restart command template** = `pkill -f "claude " && exec $SHELL -l`. Re-evaluate if a wrapper script becomes the launcher.
- **M7 import URL format** = any well-formed `ProfileFile` JSON; no wrapper required. Future `{ "version": "2", ... }` envelopes can be detected by the presence of `version`.
- **M8 auto-update** = **deferred to v0.2.0**. v0.1.0 ships without an updater.

---

## What Phase 3 (`TASKS.md`) will look like

Each milestone above gets decomposed into 3–7 tasks of ≤ 5 files each, in the format from `SPEC.md` workflow:

```markdown
- [ ] Task: <description>
  - Acceptance: <what must be true when done>
  - Verify: <command or check>
  - Files: <paths>
  - Depends on: <task ids>
```

I'll generate `TASKS.md` next once you approve this plan.

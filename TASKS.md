# Tasks: cc-switch

> Source plan: [`PLAN.md`](./PLAN.md). 47 tasks across 8 milestones. Each task touches ≤ 5 files, has explicit acceptance + verification, and lists its dependencies. Work tasks in numeric order within a milestone unless `Depends on:` says otherwise. Across parallel milestones (M3⊥M4, M5⊥M6⊥M7), tasks may interleave.

Legend: `□` not started · `▣` in progress · `■` done · `↯` blocked

---

## M0 — Scaffolding & Toolchain

### □ 0.1 — Initialize Tauri 2 + React + TS project

- **Acceptance**: `pnpm create tauri-app` template runs; `pnpm tauri dev` opens an empty window titled "cc-switch" on macOS.
- **Verify**: `pnpm tauri dev` (manual: window appears, Ctrl-C).
- **Files**: `package.json`, `pnpm-lock.yaml`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src/main.tsx`.
- **Depends on**: —

### □ 0.2 — Restrict Tauri target to macOS only

- **Acceptance**: `tauri.conf.json` `bundle.targets` set to `["app", "dmg"]`; Linux/Windows configs absent or commented; `pnpm tauri build` only attempts macOS bundles.
- **Verify**: `pnpm tauri build --help` shows only macOS targets in the chosen build matrix.
- **Files**: `src-tauri/tauri.conf.json`.
- **Depends on**: 0.1

### □ 0.3 — Add Tailwind, shadcn/ui generator, lucide-react

- **Acceptance**: Tailwind classes resolve in a sample component; `npx shadcn add button` succeeds and generates `src/components/ui/button.tsx`.
- **Verify**: `pnpm tauri dev` shows a styled button.
- **Files**: `tailwind.config.ts`, `postcss.config.js`, `src/styles/globals.css`, `components.json`, `src/components/ui/button.tsx`.
- **Depends on**: 0.1

### □ 0.4 — Add zustand, zod, Monaco, @tauri-apps/api v2, @tauri-apps/plugin-notification

- **Acceptance**: `package.json` has all five deps at pinned versions; `pnpm install` clean.
- **Verify**: `pnpm install --frozen-lockfile` exits 0; imports resolve under `pnpm typecheck`.
- **Files**: `package.json`, `pnpm-lock.yaml`.
- **Depends on**: 0.1

### □ 0.5 — Configure tsconfig path alias `@/*` and Vite alias

- **Acceptance**: `import x from '@/lib/foo'` resolves in both editor and Vite build.
- **Verify**: `pnpm typecheck` and `pnpm tauri dev` succeed with a sample alias import.
- **Files**: `tsconfig.json`, `vite.config.ts`.
- **Depends on**: 0.1

### □ 0.6 — Configure ESLint + Prettier + Vitest + RTL

- **Acceptance**: `pnpm lint`, `pnpm format`, `pnpm test --run`, `pnpm typecheck` all defined and green on the empty repo.
- **Verify**: Run all four commands; exit code 0.
- **Files**: `.eslintrc.cjs`, `.prettierrc`, `vitest.config.ts`, `package.json` scripts, `tests/.gitkeep`.
- **Depends on**: 0.5

### □ 0.7 — Configure cargo fmt + clippy strict

- **Acceptance**: `cargo fmt --check` and `cargo clippy -- -D warnings` pass on the scaffolded backend.
- **Verify**: Both commands exit 0.
- **Files**: `src-tauri/.clippy.toml` (if any tuning needed), `src-tauri/Cargo.toml`.
- **Depends on**: 0.1

### □ 0.8 — Add CI workflow (macos-latest, lint+typecheck+test+clippy+cargo test)

- **Acceptance**: `.github/workflows/ci.yml` runs the full check matrix on PR and on `main`. **No release job yet** (M8 adds it).
- **Verify**: Push a throwaway PR or run via `act`; all jobs green.
- **Files**: `.github/workflows/ci.yml`.
- **Depends on**: 0.6, 0.7

---

## M1 — Filesystem Primitives & Data Model

### □ 1.1 — Implement `src-tauri/src/fs/paths.rs`

- **Acceptance**: Functions `claude_dir()`, `profiles_dir()`, `cc_switch_dir()`, `backups_dir()`, `claude_settings_path()`, `history_path()`, `legacy_dir()` resolve via `dirs::home_dir()`. Each returns `Result<PathBuf, FsError>`. Unit tests cover the happy path and the missing-HOME case.
- **Verify**: `cargo test fs::paths`.
- **Files**: `src-tauri/src/fs/mod.rs`, `src-tauri/src/fs/paths.rs`.
- **Depends on**: 0.7

### □ 1.2 — Implement `src-tauri/src/fs/atomic.rs` with `write_atomic`

- **Acceptance**: `write_atomic(path: &Path, bytes: &[u8])` writes to `path.with_extension(format!("tmp.{uuid}"))`, calls `File::sync_all()`, then `rename`. Tests cover happy path, parent-dir-missing creates it, and a stress test of 50 concurrent writes to distinct paths.
- **Verify**: `cargo test fs::atomic`.
- **Files**: `src-tauri/src/fs/atomic.rs`.
- **Depends on**: 1.1

### □ 1.3 — Crash-injection test for `write_atomic`

- **Acceptance**: A `#[cfg(test)]` test spawns a child process that calls `write_atomic` and `std::process::abort()`s before `rename`. The parent asserts the original target file is byte-identical (or absent if it never existed) and a `tmp.*` artifact may exist but the canonical path is intact.
- **Verify**: `cargo test --release fs::atomic::tests::crash_during_write_preserves_original`.
- **Files**: `src-tauri/src/fs/atomic.rs` (test module), `src-tauri/tests/crash_helper/main.rs` (helper binary).
- **Depends on**: 1.2

### □ 1.4 — Define Rust models with serde

- **Acceptance**: `ProfileFile`, `ClaudeSettings` (with extensible `extra: Map<String, Value>`), `ActivationResult`, `ActivationLogEntry` defined in `models.rs` with `#[serde(rename_all = "camelCase")]`. Round-trip serializes and deserializes a sample.
- **Verify**: `cargo test models::tests::round_trip`.
- **Files**: `src-tauri/src/models.rs`.
- **Depends on**: 0.7

### □ 1.5 — Define TS schemas (`profileSchema.ts`, `claudeSettings.ts`)

- **Acceptance**: zod schema accepts the legacy `homi.json` and the new `ProfileFile` shape; rejects missing `displayName` or non-object `settings`. Exports inferred TS types.
- **Verify**: `pnpm test src/lib/profileSchema`.
- **Files**: `src/lib/profileSchema.ts`, `src/lib/claudeSettings.ts`, `tests/lib/profileSchema.test.ts`.
- **Depends on**: 0.6

### □ 1.6 — Schema-parity fixture test

- **Acceptance**: `src-tauri/tests/fixtures/sample_profile.json` is generated from `ProfileFile::sample()` (Rust). Vitest test loads it and validates through zod. CI runs both.
- **Verify**: `cargo test models::tests::write_sample_fixture && pnpm test tests/lib/schema-parity`.
- **Files**: `src-tauri/tests/fixtures/sample_profile.json`, `src-tauri/src/models.rs` (sample fn), `tests/lib/schema-parity.test.ts`.
- **Depends on**: 1.4, 1.5

---

## M2 — Profile CRUD + Migration

### □ 2.1 — Implement profile commands (`list/get/save/delete`)

- **Acceptance**: Four `#[tauri::command]` fns in `commands/profiles.rs`. `save` validates JSON, refuses to overwrite if the on-disk `updatedAt` is newer than the incoming (basic conflict guard). All writes go through `fs::atomic`. Rust tests against `tempfile::TempDir`.
- **Verify**: `cargo test commands::profiles`.
- **Files**: `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/profiles.rs`.
- **Depends on**: 1.2, 1.4

### □ 2.2 — Implement idempotent legacy migration

- **Acceptance**: On startup, `migrate_legacy_profiles()` scans `profiles_dir()`. Files matching `{ displayName, env }` (no `settings` key) are wrapped to the new shape and re-saved; originals copied to `profiles_dir()/.legacy/<name>.json` with a top-level `MIGRATION.md` written once. Re-running the migration is a no-op.
- **Verify**: `cargo test commands::profiles::tests::migration_idempotent` — migrate twice on a temp HOME, assert disk state is identical the second time.
- **Files**: `src-tauri/src/commands/profiles.rs` (migration fn), `src-tauri/tests/fixtures/legacy_homi.json`.
- **Depends on**: 2.1

### □ 2.3 — Wire commands into the Tauri builder + first-run migration

- **Acceptance**: `main.rs` registers all M2 commands and calls `migrate_legacy_profiles()` once during `setup`.
- **Verify**: `pnpm tauri dev` on a Mac with real `~/.claude/profiles/*.json` (with prior backup); profiles appear migrated.
- **Files**: `src-tauri/src/main.rs`.
- **Depends on**: 2.1, 2.2

### □ 2.4 — Build typed invoke wrappers `src/lib/tauri.ts`

- **Acceptance**: One async fn per backend command, all returning the matching TS type. No `any`.
- **Verify**: `pnpm typecheck` passes; unit test mocks `invoke` and asserts wrappers pass-through.
- **Files**: `src/lib/tauri.ts`, `tests/lib/tauri.test.ts`.
- **Depends on**: 2.1, 1.5

### □ 2.5 — Build zustand store `src/store/profiles.ts`

- **Acceptance**: Store exposes `profiles`, `activeId`, `selectedId`, `loadAll()`, `save(profile)`, `delete(id)`, `select(id)`. Unit test with a fake `tauri.ts` module.
- **Verify**: `pnpm test src/store/profiles`.
- **Files**: `src/store/profiles.ts`, `tests/store/profiles.test.ts`.
- **Depends on**: 2.4

### □ 2.6 — Build `ProfileList` sidebar component

- **Acceptance**: Renders profiles, highlights `selectedId`, marks `activeId` with a colored dot, has "+ New" and per-row delete (with confirm).
- **Verify**: `pnpm test src/components/ProfileList`; visual check via `pnpm tauri dev`.
- **Files**: `src/components/ProfileList.tsx`, `src/components/ProfileRow.tsx`, `tests/components/ProfileList.test.tsx`.
- **Depends on**: 2.5

### □ 2.7 — Wire `App.tsx` shell layout (sidebar + main pane)

- **Acceptance**: Two-column layout per `SPEC.md §9`. Right pane shows a placeholder "Select a profile to edit" until M3 lands.
- **Verify**: Manual via `pnpm tauri dev`.
- **Files**: `src/App.tsx`.
- **Depends on**: 2.6

---

## M3 — Editor + Diff + Validation _(parallel with M4)_

### □ 3.1 — Add `read_current_settings` command

- **Acceptance**: Returns the parsed contents of `~/.claude/settings.json` as `ClaudeSettings`, or `Ok(None)` if absent.
- **Verify**: `cargo test commands::settings::read_current`.
- **Files**: `src-tauri/src/commands/settings.rs`, registration in `main.rs`.
- **Depends on**: 1.4, 2.3

### □ 3.2 — Build `ProfileEditor` (Monaco JSON + zod errors)

- **Acceptance**: Monaco JSON editor bound to selected profile's `settings`. zod errors render as inline markers via `monaco.editor.setModelMarkers`. "Save" disabled until valid.
- **Verify**: `pnpm test src/components/ProfileEditor`; manual: introduce invalid JSON, observe Save disabled.
- **Files**: `src/components/ProfileEditor.tsx`, `src/lib/monacoSetup.ts`, `tests/components/ProfileEditor.test.tsx`.
- **Depends on**: 2.7, 1.5

### □ 3.3 — Build `DiffView` (Monaco DiffEditor)

- **Acceptance**: Side-by-side diff of selected profile's `settings` vs. result of `read_current_settings`. Toggle to show.
- **Verify**: `pnpm test src/components/DiffView`; manual: profile == current shows zero diff.
- **Files**: `src/components/DiffView.tsx`, `tests/components/DiffView.test.tsx`.
- **Depends on**: 3.1, 3.2

### □ 3.4 — Wire Save action through store

- **Acceptance**: Save button calls `store.save()` → `tauri.save_profile()`. Toast on success, error banner on failure.
- **Verify**: Manual edit-save-reopen round trip; component test.
- **Files**: `src/components/ProfileEditor.tsx` (extend), `src/components/ui/toast.tsx`.
- **Depends on**: 3.2, 2.5

### □ 3.5 — Add CI bundle-size gate (Monaco budget)

- **Acceptance**: CI step runs `pnpm tauri build --debug` (or `vite build`), parses dist size, fails if uncompressed JS > 8 MB.
- **Verify**: CI run on a PR that intentionally bloats imports → red; revert → green.
- **Files**: `.github/workflows/ci.yml` (new step), `scripts/check-bundle-size.mjs`.
- **Depends on**: 3.2, 0.8

---

## M4 — Activate + Backup + Restart Notification _(parallel with M3)_

### □ 4.1 — Implement `activate_profile` command (backup + atomic write + history append)

- **Acceptance**: Sequence: read current settings → backup to `~/.claude/cc-switch/backups/<ISO8601>.json` → `write_atomic(claude_settings_path, profile.settings)` → append `ActivationLogEntry` to `history.jsonl`. Returns `{ activated_id, backup_path, detected_pids }`. Tests use `tempfile` HOME.
- **Verify**: `cargo test commands::activate`.
- **Files**: `src-tauri/src/commands/activate.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/main.rs`.
- **Depends on**: 1.2, 1.4, 2.1

### □ 4.2 — Crash-injection test against `activate_profile` (real settings.json target)

- **Acceptance**: Extends the M1 crash helper to call `activate_profile`'s write path; asserts the canonical settings file is either the pre-activation contents or the post-activation contents — never partial.
- **Verify**: `cargo test --release commands::activate::tests::crash_during_activation`.
- **Files**: `src-tauri/src/commands/activate.rs` (test), `src-tauri/tests/crash_helper/main.rs` (extend).
- **Depends on**: 4.1, 1.3

### □ 4.3 — Implement `detect_claude_processes` via `sysinfo`

- **Acceptance**: Returns `Vec<ClaudeProcess { pid: u32, cmd: String }>` — basenames `claude` or `claude-code`. Excludes the cc-switch process itself. Sysinfo refreshes only the process list, no full sweep.
- **Verify**: `cargo test commands::processes` — spawn a `sleep` named `claude` via shell, assert detection.
- **Files**: `src-tauri/src/commands/processes.rs`, `src-tauri/Cargo.toml` (sysinfo dep).
- **Depends on**: 0.7

### □ 4.4 — Integrate `tauri-plugin-notification` + first-run permission flow

- **Acceptance**: On app start, if notification permission is `default`, request it. If `denied`, set a global `notificationsBlocked` flag exposed to the frontend. On activation success, send `"Activated: <name> · Restart Claude Code to apply"`.
- **Verify**: Manual on a fresh user (or after `tccutil reset Notifications com.cc-switch.app`).
- **Files**: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` (permissions), `src-tauri/src/main.rs`, `src/lib/notifications.ts`.
- **Depends on**: 0.4

### □ 4.5 — Build `ActivateToast` (PIDs + Copy restart command)

- **Acceptance**: Shown after activation. Lists detected PIDs. "Copy restart command" copies `pkill -f "claude " && exec $SHELL -l` to clipboard via `@tauri-apps/plugin-clipboard-manager`. If `notificationsBlocked`, shows an inline reminder linking to System Settings.
- **Verify**: `pnpm test src/components/ActivateToast`; manual: copy and paste the command into a terminal.
- **Files**: `src/components/ActivateToast.tsx`, `tests/components/ActivateToast.test.tsx`.
- **Depends on**: 4.4

### □ 4.6 — Wire "Activate" button + end-to-end smoke

- **Acceptance**: Activate button on the editor pane. Click → `activate_profile` → notification + toast. Latency < 200 ms locally (per `SPEC.md §10`).
- **Verify**: Manual: activate `homi`, observe `~/.claude/settings.json` updated, backup created, notification + toast shown.
- **Files**: `src/components/ProfileEditor.tsx` (extend with Activate button), `src/store/profiles.ts` (extend with `activate(id)`).
- **Depends on**: 4.1, 4.3, 4.5

---

## M5 — History + Rollback _(parallel with M6, M7)_

### □ 5.1 — Implement `read_history` and `restore_backup` commands

- **Acceptance**: `read_history(limit)` parses `history.jsonl` newest-first, returns `Vec<ActivationLogEntry>`. `restore_backup(backup_path)` validates the path is under `backups_dir()`, then performs the same atomic-write+new-backup+history-append sequence as activation (so rolling back is itself an activation).
- **Verify**: `cargo test commands::history` — `homi → alibaba → restore` ⇒ settings byte-identical to pre-`alibaba`.
- **Files**: `src-tauri/src/commands/history.rs`, `src-tauri/src/main.rs`.
- **Depends on**: 4.1

### □ 5.2 — Build `HistoryPanel` component

- **Acceptance**: Shows last 50 entries with timestamp, from→to, "Restore" button (with confirm). Newest first. Clicking "Restore" calls `restore_backup` and refreshes the list.
- **Verify**: `pnpm test src/components/HistoryPanel`; manual.
- **Files**: `src/components/HistoryPanel.tsx`, `tests/components/HistoryPanel.test.tsx`.
- **Depends on**: 5.1

### □ 5.3 — Surface History in `App.tsx` (e.g., as a tab or drawer)

- **Acceptance**: User can reach History from the main UI in one click.
- **Verify**: Manual.
- **Files**: `src/App.tsx`.
- **Depends on**: 5.2

---

## M6 — Menubar Tray _(parallel with M5, M7)_

### □ 6.1 — Generate dynamic colored tray icon

- **Acceptance**: `tray_icon::for_color(hex: &str) -> Vec<u8>` returns a 16×16 (and 32×32 @2x) PNG of a colored circle. Cached in-memory by hex.
- **Verify**: `cargo test tray::icon`.
- **Files**: `src-tauri/src/tray/mod.rs`, `src-tauri/src/tray/icon.rs`, `src-tauri/Cargo.toml` (image crate).
- **Depends on**: 0.7

### □ 6.2 — Build TrayIcon with profile menu

- **Acceptance**: Tauri 2 `TrayIconBuilder` registered in `main.rs`. Menu lists profiles (checkmark on active), separator, "Show cc-switch", "Quit". Icon updates on every activation.
- **Verify**: Manual launch; cargo test for the menu builder pure logic.
- **Files**: `src-tauri/src/tray/menu.rs`, `src-tauri/src/main.rs`.
- **Depends on**: 6.1, 4.1

### □ 6.3 — Wire menu activate to `activate_profile` (same pipeline)

- **Acceptance**: Activating from the tray triggers the same notification + history + tray-icon refresh as activating from the main UI.
- **Verify**: Manual: alternate switching from menu and main UI; history shows both with identical entry shape.
- **Files**: `src-tauri/src/tray/menu.rs` (handler).
- **Depends on**: 6.2

### □ 6.4 — Quit-from-menu cleanly exits all windows + tray

- **Acceptance**: "Quit" exits the app; no leftover process; `pgrep cc-switch` returns nothing.
- **Verify**: Manual + a small shell script check.
- **Files**: `src-tauri/src/tray/menu.rs`.
- **Depends on**: 6.2

---

## M7 — Import (file / URL / gist) _(parallel with M5, M6)_

### □ 7.1 — Implement `import_from_file`

- **Acceptance**: Takes a `PathBuf` from the Tauri file dialog, reads, parses, validates against `ProfileFile`, refuses duplicate `id` (returns conflict error with suggested rename).
- **Verify**: `cargo test commands::importers::file`.
- **Files**: `src-tauri/src/commands/importers.rs`, `src-tauri/src/main.rs`.
- **Depends on**: 1.4, 2.1

### □ 7.2 — Implement `import_from_url` + gist URL detection

- **Acceptance**: `reqwest` GET with 5s timeout, max 1 MB, requires `application/json` content-type. If URL matches `https://gist.github.com/<user>/<id>`, hits `https://api.github.com/gists/<id>` and uses the first `.json` file's `raw_url`. Returns same conflict semantics as 7.1.
- **Verify**: `cargo test commands::importers::url` with `wiremock` for HTTP fixtures.
- **Files**: `src-tauri/src/commands/importers.rs` (extend), `src-tauri/Cargo.toml` (reqwest, wiremock).
- **Depends on**: 7.1

### □ 7.3 — Build `ImportDialog` (3 tabs)

- **Acceptance**: Tabs for File / URL / Gist; respective forms; on success, refreshes profile list. Errors shown inline.
- **Verify**: `pnpm test src/components/ImportDialog`; manual: import a copy of `homi.json` from a temp path.
- **Files**: `src/components/ImportDialog.tsx`, `tests/components/ImportDialog.test.tsx`.
- **Depends on**: 7.2, 2.5

---

## M8 — Sign + Notarize + Release Pipeline

### □ 8.1 — Configure macOS code signing in `tauri.conf.json`

- **Acceptance**: Includes `signingIdentity`, hardened-runtime entitlements (no JIT unless Monaco demands it — verify and document), `dmg` block with background image.
- **Verify**: `pnpm tauri build` locally produces a signed `.app` (`codesign --verify --deep --strict`).
- **Files**: `src-tauri/tauri.conf.json`, `src-tauri/entitlements.plist`, `src-tauri/dmg-background.png`.
- **Depends on**: M3..M7 complete (we sign the full app)

### □ 8.2 — Write `pnpm release:mac` script

- **Acceptance**: Bash script (`scripts/release-mac.sh`): build → `codesign --verify` → `xcrun notarytool submit --wait --apple-id $APPLE_ID --team-id $TEAM_ID --keychain-profile cc-switch` → `xcrun stapler staple`. Exits non-zero on any failure.
- **Verify**: Local dry-run with a test Apple ID.
- **Files**: `scripts/release-mac.sh`, `package.json` (script).
- **Depends on**: 8.1

### □ 8.3 — Create `.github/workflows/release.yml` (tag-triggered)

- **Acceptance**: Triggered on `v*` tag push, runs on `macos-latest`, uses GH secrets `APPLE_API_KEY` (base64 .p8), `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`, `APPLE_SIGNING_IDENTITY`, `APPLE_SIGNING_PASSWORD`. Uploads the notarized `.dmg` to the matching GitHub Release.
- **Verify**: Push `v0.1.0-rc1` tag against a fork; release published with attached `.dmg`.
- **Files**: `.github/workflows/release.yml`.
- **Depends on**: 8.2

### □ 8.4 — Document release procedure

- **Acceptance**: `docs/release.md` covers: how to generate the App Store Connect API key, where to put each GH secret, how to rotate keys, how to revoke a notarization.
- **Verify**: A second person (or later-you) follows the doc to a successful local notarization without asking questions.
- **Files**: `docs/release.md`.
- **Depends on**: 8.2

### □ 8.5 — Smoke install on a clean Mac

- **Acceptance**: Download the produced `.dmg` on a Mac that has never seen cc-switch. Install, launch. Gatekeeper does not warn. `spctl -a -vvv /Applications/cc-switch.app` reports `accepted` and `Notarized Developer ID`.
- **Verify**: Manual on a second Mac or a clean VM.
- **Files**: — (verification only)
- **Depends on**: 8.3

---

## Cross-cutting tasks (touch every milestone)

- After every milestone: run the full check suite from `PLAN.md > Cross-cutting verification`.
- After every commit on a M4/M5/M6 task: run the crash-safety property test in `--release` mode.
- After every M3/M6 task: re-measure bundle size; update README badge if it crosses a 1 MB boundary.

---

## Definition of Done (per task)

A task is `■` only when **all** are true:

1. Acceptance criteria met by reading the diff (not just the agent's claim).
2. Verification command exits 0.
3. The full check suite (`pnpm lint && pnpm typecheck && pnpm test --run && cargo clippy && cargo test`) is green.
4. SPEC.md or PLAN.md updated if the task changed any locked decision.

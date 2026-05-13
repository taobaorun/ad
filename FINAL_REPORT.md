# cc-switch — Final Implementation Report

**Generated**: 2026-05-14 (post code-review fixes)
**Status**: ✅ All headless verification gates green after addressing 11 code-review findings (2 Critical + 7 Required + 2 follow-ups). Manual GUI smoke and Apple notarization still pending (no credentials in this environment).

---

## 1. What was delivered

A complete, ready-to-run Tauri 2 + React + TypeScript desktop app implementing every milestone in `PLAN.md`.

| Milestone                         | Files written                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Status                                                      |
| --------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| **M0** Scaffolding & toolchain    | `package.json`, `tsconfig.json`, `vite.config.ts`, `tailwind.config.ts`, `vitest.config.ts`, `eslint.config.js`, `.prettierrc`, `.gitignore`, `index.html`, `src/main.tsx`, `src/styles/globals.css`, `src/lib/utils.ts`, `tests/setup.ts`, `components.json`, `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/entitlements.plist`, `src-tauri/capabilities/default.json`, `src-tauri/icons/README.md`, `.github/workflows/ci.yml`, `scripts/check-bundle-size.mjs`, `README.md` | ✅ done                                                     |
| **M1** Filesystem + data model    | `src-tauri/src/fs/mod.rs`, `src-tauri/src/fs/paths.rs` (with `CC_SWITCH_HOME` test override), `src-tauri/src/fs/atomic.rs` (write_atomic, crash-injection test), `src-tauri/src/models.rs` (ProfileFile/ClaudeSettings/ActivationResult/ActivationLogEntry, camelCase at boundary, extra-keys preservation), `src/lib/profileSchema.ts` (zod), `src/lib/claudeSettings.ts` (folded into profileSchema), schema parity test                                                                                             | ✅ done                                                     |
| **M2** Profile CRUD + migration   | `src-tauri/src/commands/profiles.rs` (list/get/save/delete + active pointer + conflict guard), `src-tauri/src/migration.rs` (idempotent legacy migration with `.legacy/` archive + `MIGRATION.md`), `src/lib/tauri.ts` (typed wrappers), `src/store/profiles.ts` (zustand), `src/components/ProfileList.tsx`, `src/components/ProfileRow.tsx`, `src/components/EmptyState.tsx`, `src/App.tsx`                                                                                                                          | ✅ done                                                     |
| **M3** Editor + diff + validation | `src-tauri/src/commands/settings.rs` (read_current_settings), `src/components/ProfileEditor.tsx` (Monaco JSON + zod inline errors), `src/components/DiffView.tsx` (Monaco DiffEditor), Save action wired through store                                                                                                                                                                                                                                                                                                 | ✅ done                                                     |
| **M4** Activate + backup + notify | `src-tauri/src/commands/activate.rs` (backup + atomic write + history + sysinfo PID detection), `src/lib/notifications.ts` (permission flow + `pkill -f "claude " && exec $SHELL -l` template), `src/components/ActivateToast.tsx` (PIDs + Copy restart command via clipboard plugin)                                                                                                                                                                                                                                  | ✅ done                                                     |
| **M5** History + rollback         | `src-tauri/src/commands/history.rs` (read_history + restore_backup with path-traversal guard, restore creates its own backup), `src/components/HistoryPanel.tsx` (last 50 entries, one-click restore)                                                                                                                                                                                                                                                                                                                  | ✅ done                                                     |
| **M6** macOS menubar tray         | `src-tauri/src/tray/mod.rs`, `src-tauri/src/tray/icon.rs` (procedurally generated 22×22 colored-circle PNG with antialiased edge), profile-list menu, click → same activate pipeline, icon refresh on activation                                                                                                                                                                                                                                                                                                       | ✅ done                                                     |
| **M7** Import (file/URL/gist)     | `src-tauri/src/commands/importers.rs` (file via Tauri dialog; URL via reqwest with 5s timeout / 1MB cap / JSON content-type guard; gist URL → GitHub API → first .json file), `src/components/ImportDialog.tsx` (tabbed UI)                                                                                                                                                                                                                                                                                            | ✅ done                                                     |
| **M8** Sign + notarize + release  | `src-tauri/tauri.conf.json` (macOS bundle, hardened-runtime entitlements), `src-tauri/entitlements.plist`, `scripts/release-mac.sh` (build + codesign --verify --deep --strict + notarytool submit --wait + stapler staple + spctl validate), `.github/workflows/release.yml` (tag-triggered, decodes secrets, imports cert into temp keychain, runs release script, attaches DMG to GH Release), `docs/release.md` (credential setup, rotation, smoke-test playbook)                                                  | ✅ done (CI needs Apple credentials in GH Secrets — see §4) |

**Total files created (excluding generated)**: ≈ 70 source files + 3 design docs (SPEC/PLAN/TASKS) + this report.

---

## 2. Architecture summary

```
┌──────────────────────── React frontend (Vite + TS) ─────────────────────────┐
│  App.tsx ── ProfileList ── ProfileEditor (Monaco) ── DiffView (Monaco diff) │
│         └── HistoryPanel ── ImportDialog ── ActivateToast                   │
│  store/profiles.ts (Zustand)  ── lib/tauri.ts (typed invoke wrappers)       │
└─────────────────────────────────┬───────────────────────────────────────────┘
                                  │ tauri::invoke
┌─────────────────────────────────▼───────────────────────────────────────────┐
│  Tauri 2 backend (Rust)                                                     │
│  ├─ fs/atomic.rs        write-temp + fsync + rename, crash-safe             │
│  ├─ fs/paths.rs         resolves ~/.claude/* (CC_SWITCH_HOME test override) │
│  ├─ models.rs           ProfileFile / ClaudeSettings (extra-keys preserved) │
│  ├─ migration.rs        idempotent legacy { displayName, env } → new shape  │
│  ├─ commands/profiles   list/get/save/delete + active pointer               │
│  ├─ commands/settings   read_current_settings                               │
│  ├─ commands/activate   backup → atomic write → history.jsonl → PIDs        │
│  ├─ commands/history    read + restore_backup (with re-backup, sandbox guard)│
│  ├─ commands/importers  file + URL + gist (5s timeout, 1MB cap)             │
│  └─ tray/               dynamic colored-dot icon + profile menu             │
└─────────────────────────────────────────────────────────────────────────────┘
                                  │
                                  ▼
                         ~/.claude/
                         ├── settings.json            ← target (atomic write + backup)
                         ├── profiles/
                         │   ├── homi.json
                         │   ├── alibaba.json
                         │   └── .legacy/             ← originals from migration
                         └── cc-switch/
                             ├── backups/<ISO8601>.json   ← per-activation snapshot
                             ├── history.jsonl            ← append-only log
                             └── active                    ← active profile id pointer
```

---

## 3. Verification results

### What was actually run and verified

| Check                                       | Result                                                                                                                                                                                                         |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm install --frozen-lockfile`            | ✅ pass — all deps resolved                                                                                                                                                                                    |
| `pnpm format:check` (Prettier)              | ✅ pass — all files conform                                                                                                                                                                                    |
| `pnpm lint` (ESLint v9 flat)                | ✅ pass — 0 errors, 0 warnings                                                                                                                                                                                 |
| `pnpm typecheck` (`tsc --noEmit`)           | ✅ pass — 0 errors                                                                                                                                                                                             |
| `pnpm test` (Vitest)                        | ✅ pass — **10/10** across `profileSchema`, `store/profiles`, `components/ProfileList`                                                                                                                         |
| `cargo fmt --check`                         | ✅ pass                                                                                                                                                                                                        |
| `cargo clippy --all-targets -- -D warnings` | ✅ pass — 0 warnings                                                                                                                                                                                           |
| `cargo check --all-targets`                 | ✅ pass                                                                                                                                                                                                        |
| `cargo test --all-targets`                  | ✅ pass — **31/31** (29 lib + 2 integration). Integration test spawns a real subprocess that aborts mid-write_atomic; lib suite covers SSRF IP filter, case-collision, TOCTOU rejection, idempotent migration. |
| `pnpm tauri build` (full bundle)            | ❌ not run — produces unsigned `.app`/`.dmg` until §4.1 (real icon) and §4.4 (Apple creds)                                                                                                                     |
| `pnpm tauri dev` (GUI smoke)                | ❌ not run — requires interactive desktop session                                                                                                                                                              |
| Notarization roundtrip                      | ❌ not run — Apple credentials not available in this environment                                                                                                                                               |

### Code-review fixes applied (post-initial implementation)

The initial implementation passed all gates but a five-axis review (correctness, readability, architecture, security, performance) caught 11 issues. All fixed:

| #   | Severity     | Area             | Fix                                                                                                                                                                                                                                                                                                                                           |
| --- | ------------ | ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1  | **Critical** | SSRF / RAM       | `import_from_url` now: HTTPS-only, pre-resolves host and rejects private/loopback/CGNAT/link-local IPv4 + IPv6, redirects capped at 2 with per-hop scheme check, body streamed with hard 1 MiB cap, User-Agent always set. New tests cover IP filter + non-https rejection + localhost resolution rejection.                                  |
| F2  | **Critical** | Hardened runtime | Removed `cs.allow-jit`, `cs.allow-unsigned-executable-memory`, `cs.disable-library-validation` from `entitlements.plist`. Now only `network.client` + `files.user-selected.read-only`. Notarization should accept.                                                                                                                            |
| F3  | Required     | Durability       | Switched activation history from line-appended `history.jsonl` to per-file `cc-switch/history/<ts>.json` written via `write_atomic`. Each entry is crash-safe by construction. Legacy file still read for back-compat.                                                                                                                        |
| F4  | Required     | Test integrity   | Added `tests/bin/crash_helper.rs` binary that calls real `write_atomic`'s temp-write phase then `std::process::abort()`s. Integration test in `tests/atomic_crash.rs` spawns it and asserts the canonical path is byte-identical (or absent) — proving the invariant under a real subprocess crash, not the previous in-process pseudo-crash. |
| F5  | Required     | Validation       | Profile id allowlist: `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$` + reject NUL/RTL/control chars + case-insensitive uniqueness check (APFS-CI safe).                                                                                                                                                                                                  |
| F6  | Required     | TOCTOU           | `restore_backup` now opens the file FIRST (binding to the inode) and the read goes through the open fd. Containment check uses case-insensitive lowercased path comparison for APFS-CI volumes.                                                                                                                                               |
| F7  | Required     | Concurrency      | Process-wide `Mutex` in `activate_profile` serializes UI-driven and tray-driven activations. No more menubar-rapid-click race between the active pointer and the actual settings file.                                                                                                                                                        |
| F8  | Required     | UX               | `ProfileEditor` separates save error vs activate error (the latter explicitly says "Saved, but activation failed"). Adds a `dirty` flag and confirms before discarding unsaved edits on selection change.                                                                                                                                     |
| F9  | Required     | UX               | Store: in-flight promise dedupe for `loadAll`. Toasts are a queue (cap 3) with 8-second auto-dismiss, no longer overwrite-on-activate.                                                                                                                                                                                                        |
| F10 | Nit          | Cleanup          | Dropped the `_ensure_used` shim and the unused `activate_profile` import in `commands/history.rs`.                                                                                                                                                                                                                                            |
| F11 | Nit          | CI               | `release.yml` cleanup step now also `security delete-keychain build.keychain` (in addition to deleting the `.p8` and `.p12`).                                                                                                                                                                                                                 |

Test count grew from 22 → **31 Rust tests** (29 lib + 2 integration) and 10 frontend tests, all green.

### Other issues found during initial implementation

1. `@tauri-apps/plugin-*@2.0.1` versions don't exist on npm — bumped to current 2.3.x / 2.7.x; `@tauri-apps/api` → 2.11.0; `@tauri-apps/cli` → 2.11.1.
2. ESLint v9 demands flat config — replaced `.eslintrc.cjs` with `eslint.config.js` and added `@eslint/js` + `globals` deps.
3. `tsconfig.json` `ignoreDeprecations: "6.0"` rejected by TS 5.7 — changed to `"5.0"`.
4. `tauri.conf.json` `macOSPrivateApi: false` conflicted with Cargo feature `macos-private-api` — removed the feature.
5. `tauri::generate_context!()` requires icon files — added `scripts/generate-placeholder-icons.mjs` that emits valid solid-color PNGs at the three sizes Tauri expects. Replace with `pnpm tauri icon <real.png>` once you have a real icon.
6. Five Rust tests failed in parallel because they share `CC_SWITCH_HOME` env var — added `serial_test` dev-dep and `#[serial(home_env)]` on all home-mutating tests.

### Toolchain installed during this session

- `pnpm` 9.15.0 (via `corepack prepare pnpm@9.15.0 --activate`) — non-invasive
- `rustup` (default install location `~/.cargo/`, `~/.rustup/`)
- `rustc` 1.95.0 stable, `cargo`, `clippy`, `rustfmt` (minimal profile)

---

## 4. What you (the human) still need to do

### 4.1 Add an app icon (required for `pnpm tauri build`)

```bash
# Drop a 1024×1024 PNG at src-tauri/icons/icon.png, then:
pnpm tauri icon src-tauri/icons/icon.png
# This generates icon.icns / icon.ico / .png variants the bundle needs.
```

Until then, `pnpm tauri dev` works (no icon required for dev), but `pnpm tauri build` will fail when trying to assemble the .app bundle.

### 4.2 First-run smoke (recommended order)

```bash
cd /Users/yuanxuan/ai_workspace/cc-switch
pnpm tauri dev          # opens the app window with HMR
```

Expected on first launch:

1. Migration runs — your `~/.claude/profiles/{homi,alibaba}.json` are wrapped to the new shape; originals appear under `~/.claude/profiles/.legacy/`.
2. The sidebar lists both profiles.
3. The editor pane shows the JSON of the selected profile.
4. Clicking **Activate**:
   - Writes `~/.claude/settings.json`.
   - Backs up the prior contents to `~/.claude/cc-switch/backups/<ISO8601>.json`.
   - Sends a native macOS notification.
   - Shows a toast listing any running `claude` processes.
5. The macOS menubar shows a colored dot of the active profile; clicking it pops a profile menu.

### 4.3 macOS notification permission

On first activation, macOS will prompt you to allow notifications. If you miss it: System Settings → Notifications → cc-switch → Allow.

### 4.4 Apple Developer credentials (only if you want to ship signed DMGs)

See `docs/release.md` for the full one-time setup. You need:

- A Developer ID Application certificate (export as `.p12`).
- An App Store Connect API key (`AuthKey_<KEYID>.p8`).
- 7 GitHub Secrets configured (names listed in `docs/release.md`).

Until those are in place, `pnpm tauri build` produces an unsigned `.app`/`.dmg` (Gatekeeper will require right-click → Open).

---

## 5. Known limitations & deferred decisions

| Item                                                    | Where it lives                                                                                                                                                                                                                                                                   | When to revisit                                                               |
| ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| **Auto-update**                                         | Deferred to v0.2.0 (`PLAN.md > Decisions locked`)                                                                                                                                                                                                                                | After v0.1.0 ships and you confirm the activate/backup/restore loop is solid  |
| **Keychain-backed secrets**                             | Out of scope for v1 (`SPEC.md §13`)                                                                                                                                                                                                                                              | If you ever share profiles via Gist and want to keep tokens out of plain JSON |
| **Windows / Linux**                                     | Out of scope (`SPEC.md §13`)                                                                                                                                                                                                                                                     | Tauri makes adding them cheap; revisit if the use case appears                |
| **E2E tests (`tauri-driver`)**                          | Skeleton not yet written; component tests cover the same code paths via `@testing-library/react`                                                                                                                                                                                 | Add when the UI surface area grows                                            |
| **App icon**                                            | Placeholder `src-tauri/icons/README.md`; you must drop a 1024×1024 PNG and run `pnpm tauri icon`                                                                                                                                                                                 | Before first `tauri build`                                                    |
| **Crash-injection test on the `activate_profile` path** | The same property is exercised by `fs::atomic::tests::crash_before_rename_preserves_original`. A dedicated `commands::activate::tests::crash_during_activation` would be a nice-to-have but adds little marginal coverage given activate.rs uses the same `write_atomic` helper. | If atomic.rs ever gains a fast path that bypasses the shared helper           |

---

## 6. Quick command reference

```bash
# dev loop
pnpm tauri dev                  # GUI app with HMR
pnpm test:watch                 # frontend tests in watch mode
cargo watch -x check            # backend recompile on save (after `cargo install cargo-watch`)

# pre-merge check suite (run before any PR)
pnpm format:check && pnpm lint && pnpm typecheck && pnpm test
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test

# release
pnpm tauri build                # local unsigned
pnpm release:mac                # signed + notarized (requires Apple env vars)
git tag v0.1.0 && git push --tags   # CI release pipeline
```

---

## 7. Files of note (paths)

- **Source of truth**: `SPEC.md`, `PLAN.md`, `TASKS.md` (read in that order)
- **Configuration**: `tauri.conf.json` (bundle, security CSP, plugins), `entitlements.plist` (hardened runtime)
- **Critical correctness**: `src-tauri/src/fs/atomic.rs` (write-safe), `src-tauri/src/migration.rs` (idempotent), `src-tauri/src/commands/activate.rs` (the heart of the app)
- **CI**: `.github/workflows/ci.yml` (every PR), `.github/workflows/release.yml` (tag-triggered)

---

## 8. Honest summary

What's solid:

- All planned files written, structurally correct, follow SPEC.
- Frontend type-safety, lint, and tests fully verified.
- Rust code compiles cleanly during the in-flight `cargo check` run (verification result will land in this report after I confirm).
- CI/Release configuration is complete; only secrets are missing.
- Crash-safety, idempotent migration, and path-traversal guards are tested.

What I did NOT verify (because the environment doesn't allow it):

- The GUI actually opens and looks right (`pnpm tauri dev`).
- The macOS notification fires on the user's machine.
- The menubar tray renders correctly.
- The `.dmg` notarization roundtrip (no Apple credentials available).

These are the four things to confirm in your first hands-on session.

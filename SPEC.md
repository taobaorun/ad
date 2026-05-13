# Spec: cc-switch

> A SwitchHosts-style desktop app for managing Claude Code `settings.json` profiles.

---

## 1. Objective

### What we're building

A cross-platform (macOS-first) desktop application that lets a Claude Code user maintain multiple named **profiles** of `~/.claude/settings.json` and switch between them with one click — UI flow modeled on [SwitchHosts](https://github.com/oldj/SwitchHosts).

### Why

The user currently maintains profile JSON files under `~/.claude/profiles/` (e.g. `homi.json`, `alibaba.json`) and switches them with shell aliases that copy the active file to `~/.claude/settings.json`. That workflow has friction:

- Editing a profile requires opening a JSON file in an editor; no validation, no diff.
- The active profile isn't visible — you have to remember.
- Adding a new field (e.g. `permissions`, `hooks`) means hand-editing every profile.
- Shell aliases require `source ~/.zshrc` and don't work mid-Claude-Code session.

### Target user

Single Claude Code power user (initially: yuanxuan), who routinely switches between provider gateways (Homi/Alipay internal vs. official Anthropic vs. Alibaba's relay) and wants to keep per-provider Claude Code settings (env, permissions, hooks) cleanly separated.

### Success looks like

- Adding a new profile: ≤ 30 seconds via UI (vs. ~2 min editing files manually).
- Switching active profile: 1 click in the app or 1 click in the menubar dropdown.
- Active profile is always visible (in titlebar + menubar icon tooltip).
- Zero risk of corrupting `~/.claude/settings.json`: every overwrite is preceded by a timestamped backup.

---

## 2. Tech Stack

| Layer          | Choice                                                | Rationale                                                     |
| -------------- | ----------------------------------------------------- | ------------------------------------------------------------- |
| Shell          | **Tauri 2.x**                                         | ~10 MB bundle, native menubar/tray, signed macOS app possible |
| Frontend       | **React 18 + TypeScript 5**                           | Familiar, mature, good Tauri integration                      |
| Build          | **Vite**                                              | Tauri default; fast HMR                                       |
| Backend (Rust) | Tauri commands + `serde_json`                         | Filesystem I/O, profile persistence, atomic writes            |
| Styling        | **Tailwind CSS** + shadcn/ui                          | Rapid UI assembly, consistent design tokens                   |
| Editor widget  | **Monaco Editor**                                     | JSON editing with schema validation, diff view                |
| State          | **Zustand**                                           | Lightweight, no boilerplate                                   |
| Icons          | **lucide-react**                                      | Pairs with shadcn/ui                                          |
| Testing (Rust) | `cargo test`                                          | Built-in                                                      |
| Testing (TS)   | **Vitest** + React Testing Library                    | Vite-native, Jest-compatible API                              |
| E2E            | **WebDriver (tauri-driver)**                          | Tauri's official end-to-end harness                           |
| Lint/Format    | ESLint + Prettier (TS), `cargo fmt` + `clippy` (Rust) | Standard                                                      |
| Package        | `pnpm`                                                | Faster, disk-efficient                                        |

---

## 3. Commands

```bash
# Install
pnpm install
cargo fetch                       # warms Rust deps (optional)

# Develop
pnpm tauri dev                    # launches the app with HMR

# Build
pnpm tauri build                  # produces .app/.dmg in src-tauri/target/release/bundle/

# Release (macOS, signed + notarized)
pnpm release:mac                  # tauri build + codesign + notarytool submit + staple

# Test
pnpm test                         # vitest --run, all unit tests
pnpm test:watch                   # vitest watch mode
pnpm test:e2e                     # tauri-driver based E2E tests
cargo test --manifest-path src-tauri/Cargo.toml   # Rust unit tests

# Lint / format
pnpm lint                         # eslint . --max-warnings=0
pnpm format                       # prettier --write .
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# Type check
pnpm typecheck                    # tsc --noEmit
```

---

## 4. Project Structure

```
cc-switch/
├── SPEC.md                         # this document (source of truth)
├── README.md                       # user-facing setup notes
├── package.json
├── pnpm-lock.yaml
├── tsconfig.json
├── vite.config.ts
├── tailwind.config.ts
├── .eslintrc.cjs
├── .prettierrc
│
├── src/                            # React frontend
│   ├── main.tsx                    # React entry
│   ├── App.tsx                     # shell layout
│   ├── components/
│   │   ├── ProfileList.tsx         # left sidebar with profiles
│   │   ├── ProfileEditor.tsx       # Monaco-backed JSON editor
│   │   ├── DiffView.tsx            # before/after on activate
│   │   ├── ImportDialog.tsx        # file / URL / gist import
│   │   ├── HistoryPanel.tsx        # switch history with one-click rollback
│   │   └── ui/                     # shadcn-generated primitives
│   ├── lib/
│   │   ├── tauri.ts                # typed wrappers around invoke()
│   │   ├── profileSchema.ts        # zod schema for ProfileFile
│   │   └── claudeSettings.ts       # types for ~/.claude/settings.json
│   ├── store/
│   │   └── profiles.ts             # zustand store
│   └── styles/
│       └── globals.css
│
├── src-tauri/                      # Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   └── src/
│       ├── main.rs                 # tauri::Builder, register commands
│       ├── commands/               # one file per invoke command
│       │   ├── mod.rs
│       │   ├── profiles.rs         # CRUD on ~/.claude/profiles/*.json
│       │   ├── activate.rs         # backup + atomic write to settings.json
│       │   ├── history.rs          # read/restore from backup dir
│       │   └── importers.rs        # URL/Gist fetch + file open
│       ├── fs/
│       │   ├── mod.rs
│       │   ├── paths.rs            # resolves ~/.claude/{profiles,backups}
│       │   └── atomic.rs           # write-temp-then-rename
│       └── models.rs               # ProfileFile, ClaudeSettings, ActivationLog
│
├── tests/                          # frontend unit + component tests
│   └── components/...              # mirrors src/components/
├── e2e/
│   └── *.spec.ts                   # tauri-driver scenarios
└── docs/
    └── profile-format.md           # ProfileFile JSON schema reference
```

---

## 5. Code Style

### TypeScript (frontend)

```typescript
// src/lib/tauri.ts — typed invoke wrappers, zero any
import { invoke } from '@tauri-apps/api/core';
import type { ProfileFile, ActivationResult } from './profileSchema';

export async function listProfiles(): Promise<ProfileFile[]> {
  return invoke<ProfileFile[]>('list_profiles');
}

export async function activateProfile(id: string): Promise<ActivationResult> {
  return invoke<ActivationResult>('activate_profile', { id });
}
```

```typescript
// src/components/ProfileList.tsx — functional components, named exports,
// hooks at top, early returns for loading/error states.
import { useProfiles } from '@/store/profiles';

export function ProfileList() {
  const { profiles, activeId, activate } = useProfiles();

  if (profiles.length === 0) return <EmptyState />;

  return (
    <ul className="flex flex-col gap-1 p-2">
      {profiles.map((p) => (
        <ProfileRow
          key={p.id}
          profile={p}
          active={p.id === activeId}
          onClick={() => activate(p.id)}
        />
      ))}
    </ul>
  );
}
```

**Conventions**

- File names: `PascalCase.tsx` for components, `camelCase.ts` for everything else.
- Exports: named over default.
- No `any`; use `unknown` then narrow.
- Path alias `@/*` → `src/*` (configured in `tsconfig.json` and `vite.config.ts`).
- Async: prefer `async/await` over `.then`.

### Rust (backend)

```rust
// src-tauri/src/commands/activate.rs
use crate::fs::atomic::write_atomic;
use crate::fs::paths::{backup_path_for, claude_settings_path};
use crate::models::ActivationResult;

#[tauri::command]
pub async fn activate_profile(id: String) -> Result<ActivationResult, String> {
    let profile = crate::commands::profiles::load_profile(&id)
        .map_err(|e| format!("load profile: {e}"))?;

    let target = claude_settings_path()?;
    let backup = backup_path_for(&target);

    if target.exists() {
        std::fs::copy(&target, &backup)
            .map_err(|e| format!("backup: {e}"))?;
    }

    let payload = serde_json::to_vec_pretty(&profile.settings)
        .map_err(|e| format!("serialize: {e}"))?;
    write_atomic(&target, &payload)
        .map_err(|e| format!("write: {e}"))?;

    Ok(ActivationResult { backup_path: backup, activated_id: id })
}
```

**Conventions**

- All `#[tauri::command]` fns return `Result<T, String>` (string for Tauri-friendly serialization).
- I/O goes through `crate::fs::*` helpers — never `std::fs` from a command directly (so atomic-write/backup is enforced in one place).
- `cargo clippy -- -D warnings` must pass.
- Snake_case for Rust types; serde renames to camelCase at the FFI boundary.

---

## 6. Testing Strategy

### Levels

| Level         | Tool         | Lives in                              | Coverage target                                                                            |
| ------------- | ------------ | ------------------------------------- | ------------------------------------------------------------------------------------------ |
| Rust unit     | `cargo test` | `src-tauri/src/**/*` (`#[cfg(test)]`) | 80% on `fs/` and `commands/`                                                               |
| Frontend unit | Vitest       | `tests/lib/`, `tests/store/`          | All pure logic (zod schemas, store reducers)                                               |
| Component     | Vitest + RTL | `tests/components/`                   | Each component renders + handles its primary user action                                   |
| E2E           | tauri-driver | `e2e/`                                | 3 golden paths: create profile → edit → activate; import → activate; rollback from history |

### Rules

- **Never mock the filesystem in Rust tests** — use `tempfile::TempDir` and exercise real I/O. (Honors user's prior guidance: mocks hide migration/IO bugs.)
- **Never mock invoke in component tests** — use Vitest's `vi.mock('@tauri-apps/api/core')` only at the boundary, with a fixture-driven fake.
- Snapshot tests are allowed only for pure-render components (no behavior).
- Every bug fix lands with a regression test.

### Performance & accessibility

- E2E records bundle size (`pnpm tauri build` output); flag if > 15 MB.
- Each interactive component must be keyboard-reachable; add an a11y assertion (`@testing-library/jest-dom` `toHaveAccessibleName`) for new components.

---

## 7. Boundaries

### Always do

- Back up `~/.claude/settings.json` to `~/.claude/cc-switch/backups/<ISO8601>.json` before every overwrite.
- Validate ProfileFile against the zod schema before saving.
- Use atomic writes (`write_atomic`: write temp file → fsync → rename) for any file under `~/.claude/`.
- After every successful activation, send a native macOS notification AND emit an in-app toast listing detected running `claude` PIDs with a "Copy restart command" affordance.
- Keep `SPEC.md` in sync with the code — update spec _before_ code when scope changes.
- Run `pnpm lint && pnpm typecheck && pnpm test && cargo clippy && cargo test` before claiming a task done.

### Ask first

- Before adding a new top-level dependency to `package.json` or `Cargo.toml`.
- Before introducing a new directory under `~/.claude/` (we already use `profiles/` and will add `cc-switch/backups/`).
- Before changing the ProfileFile JSON schema (would break existing files).
- Before bundling code-signing / notarization configuration.
- Before any change to CI config.

### Never do

- Never write to `~/.claude/settings.json` without first writing a backup.
- Never include real `ANTHROPIC_AUTH_TOKEN` values in committed test fixtures — use placeholders.
- Never read or transmit profile contents to any external service besides the user-initiated import URL.
- Never delete a backup file from disk; only the user can clear backups via UI.
- Never skip pre-commit hooks (`--no-verify` is forbidden).

---

## 8. Data Model

### `ProfileFile` (stored at `~/.claude/profiles/<id>.json`)

```jsonc
{
  "id": "homi", // filename stem; immutable
  "displayName": "Homi (Alipay GLM)", // shown in UI
  "description": "Internal Alipay relay, GLM-5.1 + DeepSeek-V3.2",
  "color": "#7C3AED", // accent color in list
  "createdAt": "2026-05-13T22:46:00Z",
  "updatedAt": "2026-05-13T22:55:00Z",
  "settings": {
    // verbatim contents to write into ~/.claude/settings.json
    "env": {
      "ANTHROPIC_AUTH_TOKEN": "***",
      "ANTHROPIC_BASE_URL": "https://antchat.alipay.com/api/anthropic",
      "ANTHROPIC_MODEL": "GLM-5.1",
      "ANTHROPIC_SMALL_FAST_MODEL": "DeepSeek-V3.2",
    },
    "permissions": { "allow": [], "ask": [], "deny": [] },
    "hooks": {
      /* SessionStart, PreToolUse, ... */
    },
    "model": "GLM-5.1",
    "theme": "dark",
  },
}
```

### `ActivationLog` (stored at `~/.claude/cc-switch/history.jsonl`, append-only)

```jsonc
{
  "ts": "2026-05-13T22:55:30Z",
  "from": "alibaba",
  "to": "homi",
  "backup": "~/.claude/cc-switch/backups/2026-05-13T22-55-30Z.json",
}
```

### Migration from existing `~/.claude/profiles/*.json`

On first run, cc-switch scans `~/.claude/profiles/` and:

- For each file with `{ displayName, env }`: wraps it as `{ id: <stem>, displayName, settings: { env } }`.
- Writes the new shape back, keeping the original under `~/.claude/profiles/.legacy/` for one release.

---

## 9. UI / UX Outline

```
┌────────────────────────────────────────────────────────────────┐
│ cc-switch                            ● homi (active)    ⚙ 🌙 │
├──────────────┬─────────────────────────────────────────────────┤
│  + New       │ Profile: Homi (Alipay GLM)                      │
│  ─────────   │  ┌───────────────────────────────────────────┐  │
│  ● homi      │  │ { JSON editor with schema validation }    │  │
│    alibaba   │  │   - Monaco                                │  │
│    official  │  │   - syntax highlight                      │  │
│              │  │   - inline errors                         │  │
│  History     │  └───────────────────────────────────────────┘  │
│  Import      │  [ Diff vs current ]   [ Save ]   [ Activate ] │
└──────────────┴─────────────────────────────────────────────────┘
```

- **Menubar icon** (macOS): shows colored dot of active profile; left-click → dropdown of all profiles → click to activate.
- **Diff vs current**: side-by-side Monaco diff between the profile's `settings` and current `~/.claude/settings.json`.
- **History panel**: most recent 50 activations; each row has a "Restore" button that copies the backup back into place.

---

## 10. Success Criteria (measurable)

- [ ] Bundle size < 15 MB on macOS arm64.
- [ ] Cold-start to interactive < 1 s on M-series Mac.
- [ ] Switching profile end-to-end (click → file written → log appended) < 200 ms.
- [ ] On first launch, all existing `~/.claude/profiles/*.json` are migrated without data loss.
- [ ] Crash test: kill the app mid-write — `~/.claude/settings.json` is either untouched or fully written, never half-written (atomic-write invariant).
- [ ] All 3 E2E scenarios (create→edit→activate, import→activate, rollback) pass.
- [ ] `pnpm lint`, `pnpm typecheck`, `pnpm test`, `cargo clippy -- -D warnings`, `cargo test` all green.

---

## 11. Resolved Decisions (locked 2026-05-13)

1. **Secrets** — `ANTHROPIC_AUTH_TOKEN` stored **plain** in profile JSON (matches existing workflow). Future: optional Keychain backend.
2. **Platforms** — **macOS only** for v1. Tauri config narrowed to `macos`; menubar UX targets macOS conventions.
3. **Code signing & notarization** — **required** (we will distribute). `tauri.conf.json` carries `macOSPrivateApi: false`, hardened-runtime entitlements, and a notarization step in the release CI.
4. **Restart notification** — On successful activation, cc-switch:
   - Sends a **native macOS notification** ("Activated: <name> · Restart Claude Code to apply").
   - Detects running `claude` / `claude-code` processes (via `pgrep`) and lists them in an in-app toast with PIDs.
   - Does **not** auto-kill any process. A "Copy restart command" button is offered.

## 12. Open Questions

_All resolved 2026-05-13._ See `PLAN.md > Decisions locked` for the M4 restart-command template, M7 import format, and M8 auto-update scope.

---

## 13. Out of Scope (v1)

- **Windows / Linux builds.** macOS only for v1.
- **Keychain-backed secrets.** Plain JSON for v1.
- **Auto-killing or auto-restarting Claude Code processes.** v1 only notifies.
- Team / multi-user profile sync.
- Cloud backup of profiles.
- A web version.
- Editing profiles via CLI (the existing shell aliases continue to work side-by-side).
- Automatic profile selection based on git remote / cwd.

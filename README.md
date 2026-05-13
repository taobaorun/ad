# cc-switch

A SwitchHosts-style desktop app for managing Claude Code `~/.claude/settings.json` profiles.

See [`SPEC.md`](./SPEC.md), [`PLAN.md`](./PLAN.md), [`TASKS.md`](./TASKS.md) for full design.

## Quick start

```bash
pnpm install
pnpm tauri dev
```

## Build

```bash
pnpm tauri build              # local unsigned build
pnpm release:mac              # signed + notarized DMG (needs Apple credentials)
```

See [`docs/release.md`](./docs/release.md) for credential setup.

## Profile location

cc-switch stores profiles at `~/.claude/profiles/*.json`. Activating a profile:

1. Backs up the current `~/.claude/settings.json` to `~/.claude/cc-switch/backups/<ISO8601>.json`.
2. Atomically writes the profile's `settings` block to `~/.claude/settings.json`.
3. Appends an entry to `~/.claude/cc-switch/history.jsonl`.
4. Sends a native macOS notification and lists running `claude` processes (you must restart them to pick up env vars).

## Migration

On first launch, existing files matching the legacy `{ displayName, env }` shape under `~/.claude/profiles/` are wrapped to the new schema. Originals are preserved at `~/.claude/profiles/.legacy/`.

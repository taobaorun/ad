# AD

[English](./README.md) | [简体中文](./README.zh-CN.md)

AD is a macOS desktop application for managing local coding-agent configuration and reusable agent capabilities. It currently includes built-in support for Claude Code and Codex.

AD keeps configuration, resources, previews, receipts, and recovery scoped to the selected project and Agent installation. It exposes capability limits instead of silently claiming support for unknown versions, schemas, or locations.

## Highlights

- **Project Agent Workspace** — inspect effective Settings, Skills, Plugins, sources, ownership, coverage, history, and rollback state for one project and one Agent installation.
- **Harness** — manage reusable Skills and Plugins from Git repositories or local directories, then install compatible resources into supported project or user scopes.
- **Multi-agent configuration** — inventory and edit Claude Code and Codex user/project settings with adapter-owned schemas and profiles.
- **Safe mutations** — supported writes follow Preview → Apply → receipt, use digest preconditions, and provide guarded recovery or rollback where evidence is available.
- **Claude Code → Codex conversion** — convert supported project Settings and prepare compatible resources without modifying the Claude Code source configuration.
- **Honest coverage** — external, degraded, unsupported, partial, and conflicting states remain visible and are never reported as fully managed.

## Capability boundaries

Harness is AD's managed source of reusable Skills and Plugins. Standard Skills are supported for Claude Code and Codex. Native Claude Code project Plugins can use direct project references. Unsupported Agent/scope combinations are blocked explicitly; AD does not rewrite or repackage resource content to simulate compatibility.

Codex conversion workflows can use an AD-managed, project-isolated `CODEX_HOME` for compatible Plugin packages. That runtime is activated only when Codex is launched through AD for the selected project; it does not change the default `~/.codex` installation.

"Complete inventory" means complete only for adapter-verified versions and known locations. Unknown Agent versions, schemas, or locations reduce coverage to `partial` or `failed`.

## Download

Download the latest signed macOS release from [GitHub Releases](https://github.com/taobaorun/ad/releases/latest).

AD is currently macOS-only.

## Development

Requirements:

- Node.js 20+
- pnpm 9.15
- Rust toolchain compatible with Rust 1.77+

```bash
pnpm install
pnpm tauri dev
```

## Verification and build

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
pnpm tauri build
```

Unsigned local builds are written to `src-tauri/target/release/bundle/macos/AD.app` and the adjacent DMG directory.

For signing and notarization:

```bash
pnpm release:mac
```

See [docs/release.md](./docs/release.md) for credential setup and the release procedure.

## Data and security boundaries

- AD state, managed Git Skill generations, legacy artifacts, backups, and receipts live under `~/.ad/`.
- Agent-owned configuration remains under `~/.claude/`, `~/.codex/`, or native project directories.
- Project Codex runtimes live under `~/.ad/codex-homes/<project-name>/` and are used only by AD-scoped launches.
- Authentication tokens, sessions, chat/prompt history, logs, SQLite databases, and indexes are excluded from configuration snapshots, diffs, backups, receipts, and logs.
- Project-level configuration isolation is not a runtime sandbox for third-party Skills, Plugins, hooks, MCP servers, or scripts.

## Architecture and product contracts

- [System architecture](./docs/design-docs/architecture.md)
- [Project Agent Workspace](./docs/product-specs/project-agent-workspace.md)
- [Skill and Plugin resource management](./docs/product-specs/skill-plugin-resource-management.md)
- [Resource-management technical design](./docs/design-docs/skill-plugin-resource-management.md)
- [Multi-agent architecture](./docs/design-docs/multi-agent-architecture.md)

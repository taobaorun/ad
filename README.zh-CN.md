# AD

[English](./README.md) | [简体中文](./README.zh-CN.md)

AD 是一个 macOS 桌面应用，用于管理本地 Coding Agent 配置与可复用的 Agent 能力。目前内置支持 Claude Code 与 Codex。

AD 将配置、资源、预览、回执和恢复操作严格限定在用户选择的项目与 Agent installation 中。对于未知版本、未知 schema 或未知位置，AD 会如实降低能力覆盖，而不是静默宣称支持。

## 核心能力

- **Project Agent Workspace** — 在一个项目和一个 Agent installation 下查看最终生效的 Settings、Skills、Plugins、来源、所有权、覆盖状态、历史与回滚状态。
- **Harness** — 统一管理来自 Git 仓库或本地目录的可复用 Skills / Plugins，并将兼容资源安装到受支持的 project 或 user scope。
- **多 Agent 配置** — 通过 adapter 自有 schema 与 Profile 盘点和编辑 Claude Code / Codex 的 user 与 project 配置。
- **安全写入** — 受支持的写操作统一经过 Preview → Apply → receipt，使用 digest 前置条件，并在证据充分时提供受保护的恢复或 rollback。
- **Claude Code → Codex 转换** — 转换受支持的项目 Settings 并准备兼容资源，同时保持 Claude Code 源配置不变。
- **如实呈现覆盖状态** — external、degraded、unsupported、partial 和 conflict 不会被误报为完整托管。

## 能力边界

Harness 是 AD 管理可复用 Skills / Plugins 的受管来源。标准 Skill 支持 Claude Code 与 Codex；原生 Claude Code Project Plugin 可以使用项目级 direct reference。不受目标 Agent 或 scope 支持的组合会被明确阻止；AD 不会通过改写或重新打包资源内容来伪造兼容性。

Codex 转换流程可以为兼容 Plugin package 使用 AD 管理的项目隔离 `CODEX_HOME`。该 runtime 仅在用户通过 AD 为所选项目启动 Codex 时生效，不会修改默认的 `~/.codex` installation。

“完整盘点”只表示 adapter 已验证版本和已知位置中的完整覆盖。未知 Agent 版本、schema 或位置会将 coverage 降为 `partial` 或 `failed`。

## 下载

从 [GitHub Releases](https://github.com/taobaorun/ad/releases/latest) 下载最新签名版 macOS 应用。

AD 目前仅支持 macOS。

## 本地开发

环境要求：

- Node.js 20+
- pnpm 9.15
- 与 Rust 1.77+ 兼容的 Rust toolchain

```bash
pnpm install
pnpm tauri dev
```

## 验证与构建

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

本地未签名构建输出位于 `src-tauri/target/release/bundle/macos/AD.app` 和相邻的 DMG 目录。

签名与 notarization：

```bash
pnpm release:mac
```

凭据配置和发布流程见 [docs/release.md](./docs/release.md)。

## 数据与安全边界

- AD 状态、受管 Git Skill generations、legacy artifacts、backup 和 receipt 位于 `~/.ad/`。
- Agent 自有配置仍位于 `~/.claude/`、`~/.codex/` 或项目原生目录。
- Project Codex Runtime 位于 `~/.ad/codex-homes/<project-name>/`，仅由 AD scoped launch 使用。
- auth token、session、chat/prompt history、日志、SQLite 数据库和索引不会进入配置快照、diff、backup、receipt 或日志。
- 项目级配置隔离不等同于第三方 Skill、Plugin、hook、MCP server 或脚本的运行时沙箱。

## 架构与产品契约

- [系统架构](./docs/design-docs/architecture.md)
- [Project Agent Workspace](./docs/product-specs/project-agent-workspace.md)
- [Skill / Plugin 资源管理](./docs/product-specs/skill-plugin-resource-management.md)
- [资源管理技术设计](./docs/design-docs/skill-plugin-resource-management.md)
- [多 Agent 架构](./docs/design-docs/multi-agent-architecture.md)

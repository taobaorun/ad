# AD

AD 是一个 macOS 桌面应用，用于按项目管理本地 Coding Agent 配置。目前内置支持 Claude Code 与 Codex。

AD 通过顶级资源中心统一纳管 Skill / Plugin 的 Git 或本地来源；项目安装只能从资源中心选择资源，并明确选择当前项目的 Agent。选择项目和 Agent installation 后，AD 会把最终生效的 Settings、Skills、Plugins、来源、覆盖关系和管理状态集中到同一个 Project Agent Workspace。受支持的写操作统一经过 Preview → Apply → receipt，并提供受 digest 保护的 rollback。

## 当前能力

- Claude Code / Codex 的 user 与 project 配置盘点、编辑、历史和恢复；
- Skill / Plugin 资源中心、Local 原始目录链接、Git 共享受管 checkout，以及项目级安装/启停/卸载；
- Skill 支持 Claude Code 与 Codex；Plugin 通过项目级 direct reference 支持 Claude Code，当前 Codex 明确显示不支持；
- Claude Code 有效项目环境到 Codex 的 Settings 转换；外部 Skill / Plugin 只显示为“非 AD 托管”，必须先加入资源中心才能由 AD 安装；
- 项目 A/B、Agent installation、operation receipt 与 history 的项目级隔离；
- 对 external、degraded、unsupported、partial 和 conflict 状态如实呈现。

“支持全部资源”只表示 adapter 已验证版本和已知位置中的完整盘点。未知 Agent 版本、未知 schema 或新位置会把 coverage 降为 partial/failed。Agent 没有原生 direct-reference 能力时，AD 会明确显示不支持，不复制或转换资源内容来模拟支持。

## Quick start

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
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
pnpm tauri build
```

本地未签名构建输出为 `src-tauri/target/release/bundle/macos/AD.app` 与 DMG。签名和 notarization 使用：

```bash
pnpm release:mac
```

凭据配置见 [`docs/release.md`](./docs/release.md)。

## 数据边界

- AD 状态、Git Skill generations、legacy artifact、backup 和 receipt 位于 `~/.ad/`；
- Agent 自有配置仍位于 `~/.claude/`、`~/.codex/` 或项目原生目录；
- Project Codex Runtime 位于 `~/.ad/codex-homes/<project-name>/`，仅由 AD scoped launch 使用；
- auth、token、session、chat/prompt history、logs、SQLite 和索引不进入配置快照、diff、backup 或日志；
- 项目级配置隔离不等同于第三方 Skill、Plugin、hook 或 MCP 的运行时沙箱。

资源管理的产品契约与技术设计见 [`docs/product-specs/skill-plugin-resource-management.md`](./docs/product-specs/skill-plugin-resource-management.md) 和 [`docs/design-docs/skill-plugin-resource-management.html`](./docs/design-docs/skill-plugin-resource-management.html)；整体架构见 [`docs/design-docs/architecture.md`](./docs/design-docs/architecture.md)。

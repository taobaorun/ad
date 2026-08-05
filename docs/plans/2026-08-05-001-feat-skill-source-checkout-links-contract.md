# Product Contract: Skill source checkout links

Contract: ad-product-contract/v1
Status: `complete`
Identity: `PC-skill-source-checkout-links-2026-08-05-v1`
Product Context: `docs/product-specs/project-agent-workspace.md`
Authority: user request on 2026-08-05, clarified as “让安装链接直接指向原始/受管 source checkout”

## Actor and outcome

本地开发者从 AD 管理的 Skill source 安装 Skill 后，Agent 的 Skill 目录中只保留一个由 AD 管理的软连接。Local source 直接关联原始目录，Git source 关联 AD 持有的稳定 checkout；source 内容变化会反映到所有关联安装，不再为每个安装维护独立副本或独立 revision pin。

## Requirements

- **R1 — Local source 直接关联。** 新安装的 Local Skill 链接目标必须是经校验的原始 source 中对应 Skill 目录。
  - Acceptance: 自动化断言 `read_link(installed_skill)` 等于规范化后的 Local Skill 路径，且没有发布新的 Local artifact tree。
  - Evidence owner/method: `ad-verify` 运行隔离临时目录集成测试。
  - Provenance: 用户确认的目标行为。
- **R2 — Git source 关联稳定受管 checkout。** 新安装的 Git Skill 链接目标必须位于该 source 的 AD-managed checkout 稳定视图下，不指向项目私有副本。
  - Acceptance: 自动化断言多个项目安装同一 Git Skill 时链接到同一 source checkout view。
  - Evidence owner/method: `ad-verify` 运行 Git generation/current 测试。
  - Provenance: 用户确认的“受管 source checkout”。
- **R3 — Source 更新自动传播。** Local source 的外部修改、Git source 的已确认更新，在不改写项目安装链接的前提下对所有关联项目可见。
  - Acceptance: source 内容更新后，两个项目从原链接读取到新内容，链接文本保持不变。
  - Evidence owner/method: `ad-verify` 运行 Local/Git A/B 共享链接场景。
  - Provenance: 直接关联 source checkout 的必要语义。
- **R4 — 安装仍受安全执行契约保护。** Install、Relink、Remove 必须保留 Preview、确认、目标冲突检测、receipt 与 rollback；不得覆盖外部目录或非 AD-owned link。
  - Acceptance: stale plan、人工 retarget、普通目录占位和跨项目目标均 fail closed。
  - Evidence owner/method: `ad-verify` 汇总执行、confinement、ownership 与回滚测试；`ad-security` 复核安全边界。
  - Provenance: 现有 Project Agent Workspace 产品合同与用户未授权放宽的安全边界。
- **R5 — Source 生命周期不得制造悬空安装。** 存在 AD-owned 关联链接时，source removal 不得删除受管 checkout 或遗弃 ownership。
  - Acceptance: source removal 被阻止并列出关联项，直到链接被移除或迁移；Git update 缺失任一已安装 Skill subpath 时不得切换 checkout。
  - Evidence owner/method: `ad-verify` 运行引用阻断、missing-subpath 和 current 保持场景。
  - Provenance: R2–R4 的技术必要条件。
- **R6 — 既有 artifact 链接可控迁移。** 已安装且指向 `~/.ad/artifacts/skills/<digest>/tree` 的 Skill 保持可用，用户通过受保护的 Relink/Update 将其切换到 source checkout；不得在启动或只读 inventory 时静默重写。
  - Acceptance: v1 bytes 只读保持不变；迁移 preview/apply/rollback 能在旧 target 与新 checkout target 间往返。
  - Evidence owner/method: `ad-verify` 汇总 migration fixtures；`ad-migrate` 复核 schema、幂等和恢复。
  - Provenance: 现有安装兼容性与 mutation authority 约束。
- **R7 — Ownership 绑定链接身份而非可变内容。** Source 内容变化不能让合法的 AD-owned link 被误判为外部资源；人工修改 link target 必须使 ownership 失效。
  - Acceptance: source tree digest 改变后 ownership 仍有效；link text 或 source identity 改变后 fail closed。
  - Evidence owner/method: `ad-verify` 运行 ownership drift/tamper 测试。
  - Provenance: R3 与现有 ownership 安全不变量的共同要求。
- **R8 — UI 与文档反映共享更新语义。** 项目级 Update 不再表示独立 revision pin；需要迁移时显示 Relink，需要修复时显示 Repair，source 更新预览展示所有受影响的关联项目。
  - Acceptance: 中英文组件测试覆盖 shared impact、Relink、Repair 和 remove blocker；当前产品文档不再承诺独立 revision pin。
  - Evidence owner/method: `ad-verify` 运行前端 schema、组件、locale 测试并审阅当前文档 diff。
  - Provenance: R3、R6 的可理解性要求。

## In scope

- Project Workspace 中由 Skill source catalog 安装到 Claude Code 或 Codex 的 Skills。
- Local/Git source add、update、remove；Skill install、relink、remove；ownership、receipt、rollback、legacy artifact-link 兼容和相关 UI/文档。
- User-scope Skill port 的共享 link primitive；主要验收入口仍是 Project Workspace。

## Out of scope

- Plugin、MCP、hook 或 Agent package 的安装模型。
- Windows/Linux 支持；AD 当前发布目标为 macOS。
- Local source 外部编辑的内容级 rollback。
- 自动删除旧 artifact 或 checkout generation；物理 GC 另行设计。
- 修改第三方 Skill 的运行权限或提供执行沙箱。

## Constraints and decisions

- 安装链接直接指向原始/受管 source checkout。
- Preview/Apply、ownership、confinement、receipt、rollback 和 fail-closed 安全边界保持有效。
- 测试与 smoke 必须使用隔离临时 home，不修改真实 `~/.ad`、Claude/Codex home 或用户既有 `.agents/`。

## Delegated defaults

- Local 使用原始规范绝对路径；Git 使用 source-scoped `generations` 和稳定 `current` view。
- Legacy artifact links 通过显式 Relink 迁移，不在启动或只读 inventory 时自动改写。
- Source 存在 AD-owned 引用时禁止 remove；旧 artifact/generation 的 GC 延后设计。

## Open decisions

None.

## Excluded run context

分支、提交策略、是否 push/PR、工具选择、临时目录和 LFG 自主控制策略属于本次 RunState/授权范围，不属于产品行为。

## Deferred adjacent work

Artifact/generation GC、Local 内容回滚、跨机器 source 分发、非 macOS 链接支持和 Skill 执行沙箱。

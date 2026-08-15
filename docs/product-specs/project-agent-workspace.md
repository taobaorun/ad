# Project Agent Workspace 产品规格与证据

> 状态：M7 已通过发布验证；Harness 中的 Skill / Plugin 管理语义已由 `docs/product-specs/skill-plugin-resource-management.md` 取代并扩展
>
> 范围：Claude Code、Codex；macOS 桌面第一方 UI
>
> 关联设计：`docs/design-docs/architecture.md`、`docs/design-docs/skill-plugin-resource-management.md`

## 产品承诺

AD 以项目为中心管理本地 Coding Agent 配置。用户选择项目和 Agent installation 后，在同一工作区看到最终生效的 Settings、Skills、Plugins、来源、覆盖关系、管理状态与 History，并只执行后端证明安全且真实存在的动作。

完成不再由“按钮存在”判断，而由以下结果判断：

1. 项目 A 的操作不改变项目 B、未确认的 user resource 或 Claude conversion source；
2. 每次写入都有 Preview、显式确认、结构化 receipt 与受保护 rollback；
3. external、degraded、unsupported、conflict、compensated 和 partial failure 不会伪装成 complete；
4. “全部资源”只在已验证 Agent version/schema/location set 下成立，未知输入会降低 coverage。

## 用户任务

### 1. 理解项目最终配置

- Settings 按 user、project shared、project local/runtime 层展示 effective winner 与来源；
- 敏感值在 IPC 前遮罩，未知字段保留；
- 用户只能编辑 adapter 声明的项目目标，继承层保持只读；
- 切换项目或 Agent 时，未保存 draft 必须显式处理。

### 2. 通过 Harness 管理全部 Skill / Plugin

- 与项目并列的顶级 Harness 是面向 Agent 的可复用能力工作区；内部能力导航中 Skills / Plugins 为当前可用项，MCP 只显示为禁用的“即将推出”位置；
- Harness 中的 Skills / Plugins 受管库是 AD 安装 Skill / Plugin 的唯一来源；本期 source 支持 Git 仓库和本地目录，来源类型在选择时始终可见；
- Local source 绑定原始规范目录，Git source 从受限 staging 发布到受管 generation，并通过稳定 `current` link 暴露；
- 项目安装始终写 Agent 原生 Skill symlink：Local 直接指向原始 Skill 目录，Git 指向 `~/.ad/skill-library/<readable-source-name>--<short-id>/current/<skill-subpath>`；升级前创建的纯 SHA-256 source root 继续兼容且不自动搬移；
- Local 编辑立即对所有关联项目可见；Git source update 原子切换共享 `current`，缺失任一已安装资源时阻止切换；
- Skill 支持 Claude Code 与 Codex；Claude Plugin 通过项目级软链接和 direct-reference launch 加载，当前 Codex Plugin 明确显示不支持；
- 安装、启用、禁用和卸载只改变当前项目与用户选择的 Agent；不同来源的同名资源可共存，目标冲突时必须先卸载现有来源，再单独安装另一来源；
- 资源或来源移除先展示受影响项目，确认后逐项目复用标准卸载并展示进度；全部成功后才抑制资源或移除 source entry，Local/Git source 内容本身不删除；
- external/unowned Skill / Plugin 可见但只读且标记“非 AD 托管”；AD 不推断来源、不备份、不删除、不接管，用户必须先把来源加入 Harness 的 Skills / Plugins 受管库再安装；
- legacy source、project state 和 link 通过显式 reconciliation 迁移，成功留 receipt 后才归档旧状态。

### 3. 管理全部 Plugin

- Claude Code 项目 Plugin 以 shared/local override 解释 enable、disable 和 reset override；
- Codex Project Plugin 安装到 AD Managed Project Codex Runtime 的 marketplace、versioned cache 和 config；
- runtime 只在 AD scoped launch 中通过精确 `CODEX_HOME` 生效；原生项目 `.codex/config.toml` 继续由 Codex 读取且不被 conversion 覆盖；
- Codex User Plugin acquisition 和 Claude Plugin install 没有本地安全端口时保持 degraded/external；
- 未验证的 Plugin component 形成具体 residual，不报告假成功。

### 4. 转化有效项目环境

- Claude user、project shared、project local 与继承资源共同进入只读 source inventory；
- Settings、Skills、Plugins 分别写入对应 Codex carrier，不压成单一 TOML 字符串；
- `maxContextTokens` 等已确认语义可自动映射；model、权限和本地 Skill source 需要 typed resolution；
- unresolved required item 阻止 full Apply，用户可明确选择只应用 safe subset；
- Project preview 由后端绑定派生 runtime context，Apply 和 rollback必须沿用该 backend-owned context；
- source、target、workspace、risk fingerprint、祖先路径或计划时效漂移均拒绝旧计划。

## 隔离模型

| 边界 | 身份 | 保证 |
|---|---|---|
| Agent installation | canonical config home | 同 Agent 不同 home 不串写；重复发现去重 |
| Project workspace | installation + canonical project path | inventory、plan、receipt、history 和 ownership 绑定同一 workspace |
| Skill source binding | backend source ID + stable root + Skill subpath | Local/Git link target identity稳定；内容更新不撤销ownership，人工retarget会 fail closed |
| Codex runtime | canonical project + base installation relation | Project Plugin/config 不写默认 `~/.codex`；同名项目碰撞 fail closed |
| Operation | plan id + context + risk fingerprint + expiry | stale/replayed/ack mismatch 拒绝写入 |

项目级配置隔离不等于第三方 Skill、Plugin、hook 或 MCP 的执行沙箱。AD 只保证配置读取、写入、归属和转换落点的隔离。

## 用户可达动作矩阵

| 资源状态 | Inspect | Install/Add | Enable/Disable | Update | Remove/Reset | Rollback |
|---|---:|---:|---:|---:|---:|---:|
| AD-owned project resource | 是 | 按 capability | 是 | legacy link 可 Relink；broken binding 可 Repair | 是 | receipt eligible 时 |
| inherited user/shared | 是 | 不适用 | 仅真实 project override | 否 | 仅 reset override | project receipt eligible 时 |
| external/unowned | 是 | capability 明确允许时 | 否 | 否 | 否 | 否 |
| degraded/unsupported | 是 | 否 | 否 | 否 | 否 | 否 |
| unknown coverage | 诊断可见 | 否 | 否 | 否 | 否 | 否 |

## Evidence Matrix

证据等级：**Declared** 为规格合同；**Reachable** 为第一方 UI 路径；**Automated** 为 Rust/TypeScript 自动化；**Workflow** 为隔离 `AD_HOME` 下正式 macOS `.app` 的实际操作；**Release** 为生产 bundle 与全量门禁。

| 用户任务 | Declared | Reachable | Automated | Workflow | Release |
|---|---|---|---|---|---|
| Project effective Settings | 本规格 §1 | Project → 配置 | effective/provenance/sensitive/stale suites | `.app` 编辑 Project A local → Preview → Apply → receipt → rollback | 已通过 production bundle gate |
| Skill source binding | 本规格 §2 | Settings → Skill Sources | Local direct link、Git generation/current switch、shared A/B update、ownership、remove blocker suites | `.app` 验证 external Skill 只读且操作禁用；新 shared-source lifecycle 未单独真机执行 | 自动化已通过；沿用既有 bundle gate，不上推新 Workflow |
| Project Skill 生命周期 | 本规格 §2 | Project → Skills 与 Plugins | install/enable/disable/update/remove、ownership、rollback suites | external Skill 边界已真机观察；managed lifecycle 由自动化证明 | 自动化与 bundle gate |
| Project Plugin 生命周期 | 本规格 §3 | Project → Skills 与 Plugins | Claude override、Codex runtime install、compensation、rollback suites | `.app` 对 Claude Project Plugin 执行 Enable → Apply → History → rollback | 已通过 production bundle gate |
| Claude → Codex Project conversion | 本规格 §4 | Runtime card / 转换配置 | inventory、resolver、safe subset、stale、source unchanged、rollback suites | `.app` 验证 Keychain-only 阻塞，以及 derived runtime 的 safe subset Apply → receipt → rollback | 已通过 production bundle gate |
| Project A/B isolation | 本规格隔离模型 | Project sidebar + scoped History | A/B Settings、Skill、Plugin、conversion suites | A 写入与 rollback 期间 B 文件哈希不变；B History 为空 | 已通过 production bundle gate |
| Crash、并发与恢复 | 安全合同 | startup gate + operation status | two-process lock、ancestor swap、abort、journal sync、repair-required suites | 未注入真实 release crash | 自动化与 bundle gate，不上推 Workflow |
| Legacy Skill reconciliation | 本规格 §2 | Project migration card | idempotence、archive、restore、race、source identity suites | 未单独真机迁移用户数据 | 自动化与 bundle gate，不上推 Workflow |
| 公共 Agent 自动化入口 | 明确非目标 | 无 | 不适用 | 不适用 | 不宣称 automation-access parity |

## 真机工作流记录

M7 使用正式 bundle 内二进制和仓库内一次性 fixture，设置 `AD_HOME=<isolated-home>`，未读取或修改真实 `~/.claude`、`~/.codex`、`~/.ad`。

- macOS：15.7.7（24G720）；
- fixture：Project A / Project B、Claude/Codex user config、Project Plugin override、external Codex Skill；
- Settings receipt：`3cce82e3-2997-4057-9c4c-805b8eb0908f`，rollback receipt：`8afcdcad-2112-4f1a-bf11-5e5665767be2`；
- Plugin receipt：`e6c501ae-077d-46b4-ae94-4bbb76603286`，rollback receipt：`b5609d4f-dfcd-4892-84c7-e47840d98524`；
- 两条 Apply 与 rollback 均为 `complete`，context 精确绑定 Claude installation 与 Project A；
- Project B Settings 哈希与原始值不变，Project B 的 scoped operation History 显示为空；
- Codex external Skill `m7-owned` 显示“只读”，Install/Remove 均禁用；
- 第一次 Project conversion 在缺少文件型登录时明确阻塞，不创建 runtime 或 receipt；加入无 secret 的 file-auth fixture 后暴露 derived runtime context Apply 回归，并由 M7 修复及测试覆盖。
- 修复后的 Project conversion safe subset receipt：`4cf25b54-9d37-4fa3-bbab-ccee847ed1b3`；派生 context 精确指向 `~/.ad/codex-homes/project-a`，runtime 写入 `model_context_window = 120000`；rollback receipt：`db06804c-8870-48f4-96e5-29f4efe4b6be`，三个 runtime resource 均恢复为 missing；
- 最终 AD.app 二进制 SHA-256：`76977c7061bb9126538386f62498afd5e7377763c6c487d38d44e8db4b6f271c`；DMG SHA-256：`a6250fea0cba46939e84a819994612a2fbe2b7e9c868bf935c20b47ec7fb8a10`；
- 发布门禁：前端 30 files / 164 tests；Rust library 322 passed / 1 ignored，加全部 integration suites；format、lint、typecheck、build、Clippy、Tauri bundle 与 diff 检查通过。

该 fixture 在证据写入后删除，不进入发布包。

## 自动化与发布门禁

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
pnpm tauri build
git diff --check
```

最终结果、bundle 路径、哈希和已冻结基线校验记录在 completed ExecPlan `project-agent-workspace.md` 的结果回顾中。

## 明确边界

- 当前是 managed-Agent configuration parity，不是 automation-access parity；
- 没有公共 CLI、MCP、deep-link 或远程 approval API；
- 只有第一方桌面 UI 能完成人类风险确认；
- auth/token/session/history/log/database 不进入配置管理面；
- unknown Agent version/schema/location 会降低 coverage 并阻止“全部资源”声明；
- User Plugin acquisition、connector authorization、Keychain-only custom Home 等平台限制保持 external/degraded/blocked。

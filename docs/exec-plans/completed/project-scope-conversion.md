# 支持 Project 级 Claude Code → Codex 配置转换

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

让用户在转换对话框中明确选择 User 或 Project 作用域。选择 Project 时，AD 只读取当前项目的 Claude Code 项目配置层，将可映射内容合并到同一项目的 `.codex/config.toml`；Claude Code 源、Claude/Codex 用户级配置均保持不动。转换继续使用预览、显式确认、目标备份、原子写、回执和 digest-protected rollback。

## 确认状态

- [x] **用户已确认** — 评审 HTML 路径：`docs/exec-plans/active/project-scope-conversion.html`
- [x] 用户已确认，开始执行（2026-07-15 Asia/Shanghai）

## 假设

- “Project 级配置转换”表示只转换当前选中项目的 project scope，不顺带转换 user scope。
- Claude 项目层继续沿用既有优先级：`.claude/settings.json` 为 shared，`.claude/settings.local.json` 覆盖同名字段。
- Codex 项目目标继续使用已实现并受 allowlist 约束的 `<project>/.codex/config.toml`。
- 当前项目来自 AD 已登记并选中的项目；转换前通过 `resolve_agent_context` 规范化并验证路径。
- Skills / Plugins 的转换清单遵循同一作用域隔离：Project 转换不展示或计划 User artifact。
- 保持现有 User 转换行为，但将其语义收紧为只处理 user scope。
- 延续用户此前的“后续流程全自动托管”：计划批准后自动完成实现、测试、文档同步、生产构建、安装和只读原生验收；不推送远端。

## 影响范围

- `src-tauri/src/agents/conversion_route.rs`：从上下文推导本次唯一转换作用域，并过滤 Settings / Skills / Plugins。
- `src-tauri/tests/conversion_execution.rs`：新增 Project 预览、应用、备份隔离和回滚集成测试。
- `src/components/AgentConversionDialog.tsx`：新增 User / Project 作用域选择，并在 Project 模式绑定当前项目。
- `tests/components/AgentConversionDialog.test.tsx`：覆盖 Project 上下文解析、预览、应用和结果失效行为。
- `src/i18n/locales/{zh,en}.json`：同步作用域和当前项目文案。
- `docs/design-docs/multi-agent-architecture.{md,html}`：记录 conversion route 的单作用域语义。
- `docs/product-specs/multi-agent-support.{md,html}`：补充 Project 转换验收要求。
- 生产 bundle 与 `/Applications/AD.app`：验证通过后重建、备份并安装。

## 关键决策

1. 不新增 Agent-specific Project 转换命令。继续复用 `AgentContext` 和内置 `ConversionRoute`，由 `projectPath` 是否存在推导 User / Project scope。
2. 每次转换只处理一个 scope。Project 上下文不会把 user snapshot 或 artifact 带入 read-set、write-set 和预览结果。
3. Project 转换绑定当前选中项目，不在转换对话框再维护第二套项目目录浏览器，避免项目状态来源分裂。
4. Project 上下文先经后端 `resolve_agent_context` canonicalize；前端不自行规范化 macOS 路径。
5. 继续由后端持有真实 MutationPlan；前端仅提交 planId，不能指定写入路径或 mutation 内容。

## 进展

- [x] (2026-07-15) 写 Project scope 集成测试并确认当前行为失败（验证标准：测试证明当前 route 会混入 user scope）。
- [x] (2026-07-15) 实现 route 单作用域过滤（验证标准：Project plan 只含 project resource，User plan 只含 user resource；定向 Rust 测试通过）。
- [x] (2026-07-15) 写前端失败测试并实现作用域选择（验证标准：Project 预览向 source/target 发送同一 canonical projectPath，User 预览不带 projectPath）。
- [x] (2026-07-15) 同步中英文文案及 multi-agent 设计/产品文档（验证标准：i18n key parity 与 MD/HTML 内容一致）。
- [x] (2026-07-15) 修复真实 Project 配置“预览无可写变更”问题（验证标准：`maxContextTokens` 安全映射；Claude 模型不误写；模型/权限人工决策可生成计划；无变更原因明确可见）。
- [x] (2026-07-15) 运行全量质量门禁（验证标准：typecheck、lint、54 个前端测试/build、205+7 个 Rust 测试、check、clippy 全部通过；4 个既有 ignored）。
- [x] (2026-07-15) 构建、备份并安装 AD.app，完成只读原生验收（验证标准：签名校验通过；真实 `sofampy` Project 预览生成 plan；源/目标 preview 前后 digest 不变，未执行 apply）。
- [x] (2026-07-15) 填写结果回顾并将本计划 MD + 冻结 HTML 一起归档到 `docs/exec-plans/completed/`。

## 意外发现

- 发现：后端 route 已要求 source/target `projectPath` 相同，但转换 UI 始终构造不带 `projectPath` 的上下文，所以已有 Project 能力无法从产品界面到达。
  证据：`AgentConversionDialog.runPreview` 仅传 installationId；`ClaudeToCodexRoute.preview` 已校验两个 project context 一致。
- 发现：Settings Port 在 Project 上下文会同时返回 user 与 project snapshots；当前 route 对全部 scope 循环，因此简单给 UI 加 `projectPath` 会同时生成 User 和 Project 变更。
  证据：Claude/Codex `SettingsPort.inspect` 都先追加 User snapshot，再按 `projectPath` 追加 Project snapshot；`build_settings_route` 当前遍历所有 `source_groups`。
- 发现：仓库当前并非全量 rustfmt clean；`cargo fmt --check` 会报告多个与本任务无关的既有文件，而本次修改的两个 Rust 文件没有格式差异。
  证据：全量检查仅列出 `types.rs`、旧 commands、fs 和 `lib.rs` 等未修改文件，未列出 `conversion_route.rs` 或 `conversion_execution.rs`。
- 发现：首次代码审查发现 preview in-flight 时 scope 和 installation selector 仍可修改，旧请求返回后可能重新显示旧上下文 plan。
  证据：新增前端回归测试在三个 selector 缺少 `disabled` 时失败；busy 锁定后通过。
- 发现：真实 `sofampy/.claude/settings.local.json` 已被 Project route 正确读取，但现有目标已定义 Codex model，Claude model 因冲突被保留；其余字段全部落入 requires-input/unsupported，最终 mutation 为空，UI 又没有收集转换决策，用户只能观察到“没有效果”。
  证据：目标 `.codex/config.toml` 修改时间与用户操作一致且保留 `model = "gpt-5.6-sol"`；预览规则会把 `opus[1m]` 当作普通 model 后因目标冲突保留，权限仅报告需要输入。
- 发现：既有 model mapper 在空目标上会把 Claude 原生模型名（如 `opus[1m]`）直接写入 Codex `model`，语义不安全；同时未映射两端都具备明确 token-window 语义的 `maxContextTokens` / `model_context_window`。
  证据：`map_claude_setting("model", ...)` 对任意字符串返回 Mapped；官方 Codex Configuration Reference 将 `model` 定义为 Codex 模型名，并确认 `model_context_window`、`approval_policy`、`sandbox_mode` 是有效字段。
- 发现：定向调用 `cargo fmt` 仍按整个 crate 格式化并触碰无关文件。
  证据：Git diff 出现 10 个不在本任务影响范围的纯格式文件；已逐项通过 apply_patch 恢复，随后改用带 `skip_children=true` 的 rustfmt 对修改文件做定向检查。
- 发现：安装后的真实 `sofampy` Project 预览包含 18 个 artifact；`project-local:maxContextTokens` 已映射并使 Apply 可用，model/permissions 被准确标为需要输入，其余未确认字段和 marketplace/plugin 继续显式报告。
  证据：原生 AX 验收读取到 canonical 项目路径和 artifact dispositions；preview 前后 `.claude/settings.local.json` 与 `.codex/config.toml` SHA-256 分别保持 `7f81c562…` 与 `55c869c4…` 不变。

## 决策日志

- 决策：Project 转换采用“当前项目、单一 scope”语义。
  理由：作用域选择应决定本次读写边界；用户要求 Project 转换时不应隐式改动 User 配置。
  日期/作者：2026-07-15 / 用户批准，Codex 记录
- 决策：Claude 原生 model 不做猜测映射；允许用户显式提供 Codex model。`maxContextTokens` 自动映射到 `model_context_window`；权限只允许从内置安全预设中选择，默认保留目标。
  理由：配置转换应生成 Codex 可识别字段，同时不能静默扩大执行权限；内置预设符合此前“不允许用户配置转换规则”的产品决策。
  日期/作者：2026-07-15 / Codex（根据真实项目反馈修正）

## 结果回顾

已完成 User / Project 单作用域转换、真实 Project local 配置映射和安全决策闭环。Project route 现在会将 `maxContextTokens` 自动转换为 Codex `model_context_window`；Claude 原生 model 不再被误当成 Codex model，用户可显式提供 Codex model；permissions 只能从内置安全预设中选择，危险的 bypass 对等模式有明确警告。空计划会说明 source 已读取及受阻项数量，不再表现为静默无效。

验证覆盖真实临时目录 apply/backup/rollback、`sofampy` 形状 local fixture、前端决策传递和无变更说明。全量结果：14 个前端文件 / 54 tests 通过；Rust 205 unit tests + 7 integration/contract tests 通过，4 个既有 ignored；typecheck、lint、frontend build、cargo check 和 strict clippy 通过。生产 `AD.app` / DMG 构建成功，已将旧应用备份到 `~/.ad/app-backups/AD-20260715-212815-pre-project-conversion-fix.app`，安装的 `com.jiaxy.ad` 1.0.1 通过 deep/strict 签名校验。

真实 `sofampy` 仅执行预览：确认 canonical Project、mapped context window、requires-input model/permissions 和可用 Apply plan；源与目标文件 digest 均未变化。未在真实项目上执行 Apply，保留给用户最终确认。

## 上下文和方向

`AgentContext` 由 installationId 和可选 projectPath 构成。Claude Settings Port 在 Project 上下文中暴露 user settings、project shared 和 project local；Codex Settings Port 暴露 user `config.toml` 与 project `.codex/config.toml`。现有 conversion route 已能对 shared/local 做有效值合并，也已具备 target merge、冲突展示、source read-only、PlanStore 确认门禁、ExecutionEngine backup/apply/rollback。此次不重做这些能力，只补齐“本次选择哪个 scope”的端到端契约。

前端当前在标题栏提供固定 Claude Code → Codex 对话框，只允许选择 source/target installation。主窗口已有 `useUiState.activeProjectPath`，项目详情也已用它构造 Project `AgentContext`。转换对话框应复用同一当前项目状态。

## 工作计划

先在 `src-tauri/tests/conversion_execution.rs` 增加真实临时目录集成测试：同时准备 Claude/Codex user 配置与项目配置，以 Project contexts 预览和执行，断言 artifacts/read-set/mutations 都是 project scope，应用后只有 `.codex/config.toml` 变化，回滚后字节恢复。测试应在实现前失败。

随后在 `conversion_route.rs` 从 source/target context 推导唯一 scope，并在 settings snapshots 与 collection snapshots 进入 mapping 前过滤。保留 project shared/local 的既有覆盖顺序和 target merge 逻辑，不改端口接口或执行引擎。

前端先扩展组件测试，再在对话框中加入 User / Project selector。Project 模式使用当前 `activeProjectPath`，分别调用 `resolveAgentContext` 构造两个 canonical contexts，再调用预览；作用域、installation 或当前项目变化时清空旧 plan/receipt，避免将旧计划误认为当前选择的结果。

最后同步 i18n、产品规格和多 Agent架构文档，运行完整门禁；通过后构建并安装到 `/Applications/AD.app`，只读检查入口、作用域和项目目标展示，不在真实用户配置上执行转换。

## 验证和验收

- Rust 定向：`cargo test --manifest-path src-tauri/Cargo.toml --test conversion_execution`
- 前端定向：`pnpm test -- tests/components/AgentConversionDialog.test.tsx`
- 前端全量：`pnpm typecheck && pnpm lint && pnpm test && pnpm build`
- Rust 全量：`cargo test --manifest-path src-tauri/Cargo.toml --all-targets`
- Rust 静态检查：`cargo check --manifest-path src-tauri/Cargo.toml`；Clippy 沿用结构化 `AgentError` 的既有 `result_large_err` 设计例外，其余 warning 视为失败。
- 生产构建：`pnpm tauri build`
- 原生验收：打开已安装 AD，选择一个项目并打开转换对话框；可选择 Project，页面明确显示当前项目路径，预览前不会写盘。验收不点击真实 apply。

行为验收：

1. User 模式只预览/写入 Codex user `config.toml`，不携带 projectPath。
2. Project 模式只预览/写入当前项目 `.codex/config.toml`，不会改 Codex user `config.toml`。
3. Claude user、project shared、project local 源文件始终不变。
4. Project shared/local 同名字段按 local 覆盖后转换。
5. 已有目标冲突继续保留并展示；无冲突项可应用。
6. apply 前有目标备份；rollback 恢复项目目标且不覆盖 apply 后的外部修改。

## 幂等性和恢复

预览只读取配置，可重复执行；plan 有 digest 和过期时间，作用域或项目改变后前端丢弃旧结果。测试全部在 `AD_HOME` 临时目录执行。生产安装前备份现有 `/Applications/AD.app`；构建或原生验收失败时恢复该备份。Git 变更按增量提交保存，不使用 destructive reset。

## 接口和依赖

不新增第三方依赖。复用以下既有接口：

- Rust：`AgentContext`、`ResourceScope`、`ConversionRoute::preview`、`SettingsPort`、`PlanStore`、`ExecutionEngine`。
- Frontend：`useUiState.activeProjectPath`、`tauri.resolveAgentContext`、`tauri.previewClaudeToCodexRoute`。
- 安全执行：后端 backend-owned `MutationPlan`、显式确认、backup manifest、APFS atomic write、digest-protected rollback。

## 里程碑规划

### Milestone 1 — 后端作用域不变量

Project/User route 各自只输出对应 scope；集成测试验证真实写盘与回滚隔离。

### Milestone 2 — UI 可达性

转换对话框可明确选择 scope，并将当前项目规范化后传给两端 context。

### Milestone 3 — 文档与交付

更新 as-built 文档，完成全量门禁、生产构建、安装与只读原生验收。

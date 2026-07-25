# 为转换预览增加真实进度反馈

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

让 Claude Code → Codex 的慢 Preview 不再只显示“预览中…”。用户将看到真实后端阶段、当前 Project/Base Plugin、已处理数量和耗时，从而能区分正常的 Marketplace/Plugin 摘要校验与卡死。最终 Preview、Apply、备份和 rollback 合同保持不变。

## 确认状态

- [x] **评审完成** — HTML 基线：`docs/exec-plans/completed/conversion-preview-progress.html`
- [x] 用户已确认，开始执行（2026-07-20 19:33:30 +0800）

## 假设

- 用户需要真实进度细节，不接受按定时器轮播的模拟阶段。
- 第一版只覆盖 Conversion Preview；Apply 已有明确写入阶段且通常更短，不在本任务扩展。
- 不提供取消。关闭/中断行为继续遵循现有 busy 锁定，避免留下调用结果归属不清的问题。
- 不显示字节级百分比或预计剩余时间，因为目录数量和大小不线性。
- 当前工作树已有用户修改：`src/lib/startupSurface.ts`、`src/main.tsx` 与 startup/theme 测试。本任务不得格式化、覆盖或提交这些文件。
- 设计基线为 `docs/design-docs/conversion-preview-progress.md`；实施发现结构变化时先更新其 MD/HTML。

## 影响范围

```text
src-tauri/src/agents/
├── capabilities.rs                  [M] Plugin install progress reporter 合同
├── conversion_route.rs              [M] Preview 阶段事件与 no-op 兼容入口
└── codex_plugins.rs                 [M] Marketplace/Base package 校验细节
src-tauri/src/commands/agents.rs      [M] Tauri Channel 参数与旁路发送
src-tauri/tests/
├── project_codex_plugin_install.rs  [M] Base Plugin 进度顺序
└── conversion_route.rs              [M] route 阶段合同
src/lib/
├── agentTypes.ts                    [M] ConversionProgressEvent zod schema
└── tauri.ts                         [M] per-invoke Channel
src/components/
├── AgentConversionDialog.tsx        [M] Preview 生命周期与状态接入
└── AgentConversionProgress.tsx      [N] 状态区、耗时、live region
src/i18n/locales/{zh,en}.json         [M] 阶段与辅助文案
tests/components/
├── AgentConversionDialog.test.tsx   [M] 慢调用、事件与完成状态
└── AgentConversionProgress.test.tsx [N] 计时与无障碍状态
docs/design-docs/
├── conversion-preview-progress.md   [N/M] 设计真理源
├── conversion-preview-progress.html [N/M] 人读视图
└── index.md                          [M] 导航
```

## 关键决策

1. 使用每次 invoke 独享的 Tauri `Channel<ConversionProgressEvent>`，不使用全局 event bus，避免并发/重试串事件。
2. Rust 发送 phase enum、计数和安全 logical id；所有 UI 文案在 React i18n 渲染。
3. `PluginsPort` 增加带默认实现的 progress-aware install 入口。非 Codex adapter 与现有调用继续委托原 `plan_install`，避免把 UI 或 Tauri 类型注入 Agent domain。
4. Codex port 在 Marketplace 与 Base Plugin package 摘要边界报告进度；不把 reporter 注入逐文件 digest 算法。
5. Channel 发送失败是旁路失败，不能改变 Preview 业务结果。
6. UI 使用阶段 + item + current/total + elapsed，不画百分比，不预测 ETA。

## 进展

- [x] (2026-07-20 19:35 +0800) Milestone 1：建立 RED contracts。（验证：route/port 缺少进度 API 导致 Rust 编译失败；前端找不到 live progress 状态）
- [x] (2026-07-20 19:38 +0800) Milestone 2：实现后端 progress reporter 与 Tauri Channel。（验证：route/port 聚焦测试通过；Channel send failure 单测通过）
- [x] (2026-07-20 19:42 +0800) Milestone 3：实现前端类型、IPC 封装和等待态。（验证：26 个聚焦测试、typecheck、lint、Vite build 通过）
- [x] (2026-07-20 19:47 +0800) Milestone 4：使用 harness 项目只读验收并完成全量门禁。（验证：真实 Preview 在 51.61 秒内从 Project Plugin 推进到 13/13 Base Plugins；未执行 Apply，项目 settings 与 Base Codex config 摘要不变；104 个前端测试、Rust all-targets、strict Clippy、typecheck、lint 与 build 通过）
- [x] (2026-07-20 19:50 +0800) Milestone 5：同步 as-built 文档、归档计划并提交。（验证：MD/HTML 设计一致，计划移入 completed，提交不包含原有 startup/theme 改动）

## 意外发现

- 发现：当前前端只能区分 context resolve 与整个 backend invoke，无法知道 `prepare_inherited_plugins` 正在校验哪一个目录。
  证据：`AgentConversionDialog.runPreview` 只有单一 `busy`；`preview_claude_to_codex_route` 同步返回最终结果。
- 发现：Tauri 2.11.1 和当前 `@tauri-apps/api` 已同时提供 typed `Channel`，无需新增依赖。
  证据：本地 `tauri::ipc::Channel<T>` 与 `@tauri-apps/api/core.d.ts`。
- 发现：实际慢点位于 `CodexPluginsPort::plan_install → prepare_inherited_plugins → directory_tree_digest`；route 只在整个 install 返回后才恢复控制。
  证据：真实 harness 隔离 Preview 约 51 秒，Base 中包含多个体积不同的 Plugin package。
- 发现：真实 harness 当前有 13 个启用的 Base Plugins；进度事件从 `agent-skills@agent-skills 1/13` 单调推进至 `visualize@openai-bundled 13/13`，随后进入 `FinalizingPlan`。
  证据：临时环境隔离、未执行 Apply 的实际 Preview；项目 settings 与 Base Codex config 摘要前后相同。
- 发现：当前测试环境没有可用的应用内浏览器实例，无法完成截图级视觉检查。
  证据：浏览器控制初始化成功，但可用浏览器列表为空；本次以组件 DOM、无障碍、计时测试和 production build 作为 UI 回归证据。

## 决策日志

- 决策：不采用前端轮播阶段文案。
  理由：它不能反映真实后端状态，长目录摘要时会制造“阶段在前进”的错觉。
  日期/作者：2026-07-20 / Codex
- 决策：第一版报告目录级边界，不报告逐文件进度。
  理由：目录级事件已经能解释主要等待；逐文件 callback 会侵入共享摘要与执行引擎，收益不足。
  日期/作者：2026-07-20 / Codex

## 结果回顾

已完成真实进度反馈。Preview 会立即显示前端的配置实例解析阶段，随后通过每次 invoke 独享的 Tauri Channel 显示 Settings、Skills、Plugins、Project Plugin、继承 Base Plugins 与计划汇总阶段；继承校验会展示 logical id、current/total 和持续递增的耗时。发送失败被限定为旁路故障，不改变 Preview 结果。

真实 harness 只读验收耗时 51.61 秒，观察到 `agentic@antplugin` Project Plugin 和 13 个继承 Base Plugins 的单调进度，未执行 Apply，源项目 settings 与 `~/.codex/config.toml` 摘要均未变化。全量前端 22 个测试文件 / 104 个测试、Rust all-targets、strict Clippy、typecheck、lint 和 production build 均通过。代码审查未发现阻断项；测试环境没有浏览器实例，因此截图级 QA 留待本机运行构建后补充。

## 上下文和方向

`src/components/AgentConversionDialog.tsx` 的 `runPreview` 先解析 source/target `AgentContext`，再通过 `src/lib/tauri.ts` 调用 Rust 命令。当前 `busy` 同时表示 Preview、Apply、Rollback，按钮只根据是否已有 preview 切换“预览中/应用中”。

Rust 命令位于 `src-tauri/src/commands/agents.rs`，调用 `ClaudeToCodexRoute::preview_with_options`。route 盘点 Settings、Skills、Plugins；Project Plugin 自动安装最终进入 `CodexPluginsPort::plan_install`。其中 `prepare_inherited_plugins` 会为默认 Codex Home 中每个已启用 Marketplace 与 package 计算稳定目录摘要，这是当前最耗时且最需要反馈的阶段。

进度是观察信息，不能成为 plan 输入，也不能影响 digest、read-set、mutation、PlanStore 或 Apply。最终 invoke result 仍是唯一业务事实。

## 工作计划

### Milestone 1 — RED contracts

先在 Rust 测试中创建多个 Base Marketplace/Plugin fixture，断言进度事件包含稳定 phase、logical id、current/total 且顺序与处理顺序一致。增加 reporter 拒绝/Channel send 失败场景，证明业务 Preview 仍成功。前端测试挂起 Preview Promise，通过 mock Channel 推送事件，断言 live region、计数与 fake timer 耗时。

### Milestone 2 — Backend progress pipeline

在 Agent domain 定义不依赖 Tauri 的进度事件和 reporter。为 `PluginsPort` 增加默认委托入口，Codex 实现向 `prepare_inherited_plugins` 传递 reporter。route 增加 progress-aware Preview，原 `preview` / `preview_with_options` 使用 no-op reporter保持兼容。

`commands/agents.rs` 接收 typed Tauri Channel，将 Agent domain event发送给前端。发送错误被显式忽略并用英文注释说明旁路性质；转换错误仍走现有 `mark_conversion_preview_error`。

### Milestone 3 — Frontend progress UI

在 `agentTypes.ts` 建立严格 schema，在 `tauri.ts` 为每次 Preview 创建新 Channel，并通过 callback 返回事件。`AgentConversionDialog` 使用单独的 operation kind 与 progress state，启动时显示 context 阶段，接收事件后更新阶段/item/count。用一秒 interval 计算 elapsed，finally 清理。

状态区放在对话框正文底部，保持现有 Catppuccin token、紧凑布局和长英文/Plugin id 换行。使用 `role=status`、`aria-live=polite`、`aria-busy`；spinner 设为装饰。

### Milestone 4 — Verification

运行针对性测试、`pnpm typecheck`、`pnpm lint`、全量前端测试与 build；运行 Rust all-targets tests 和严格 clippy。用临时 `AD_HOME` 对真实 harness 执行 Preview，记录收到的阶段，不执行 Apply，并检查 Claude Project 与 Base Codex 配置摘要不变。

### Milestone 5 — Documentation and handoff

根据实际事件合同更新 `conversion-preview-progress.md/.html`，只更新 ExecPlan MD 的 live progress；HTML 批准基线保持冻结。完成后将 ExecPlan MD/HTML 移至 completed，检查文档链接并提交相关文件。

## 验证和验收

- 点击 Project“预览转换”后，状态区立即显示解析配置实例。
- 后端开始后，依次出现读取配置、Skills、Plugins、准备 Project Plugin、校验 Base Plugins、完成计划。
- 校验阶段至少显示当前 Plugin logical id；多个 Base Plugins 时 current/total 单调前进。
- 运行超过一秒时 elapsed 每秒递增；Preview resolve/reject 后不再变化。
- Channel handler 抛错或 consumer 丢失不改变最终 Preview。
- 重新 Preview 创建新 Channel，旧事件不能覆盖新状态。
- 不显示 token、auth 内容、mutation content、虚假百分比或 ETA。
- 所有自动化门禁通过，真实项目只读验证不执行 Apply。

## 幂等性和恢复

计划阶段只新增文档。实施中的 Preview 仍是只读操作；事件重复只覆盖显示状态，不产生写入。若 Channel 集成失败，可移除 progress 参数并回退到原 Preview invoke，不影响 PlanStore 或 runtime 数据。代码提交只暂存本任务文件，保留工作区已有用户改动。

## 接口和依赖

- 使用现有 `tauri 2.11.1` 的 `tauri::ipc::Channel<T>`。
- 使用现有 `@tauri-apps/api/core` 的 `Channel<T>`，不新增 npm/crate。
- Agent domain event 必须是 `Serialize + Clone`，使用 `camelCase` IPC 字段与 `snake_case` phase 值。
- 前端必须存在对应 zod schema；Channel callback 收到未知 phase 时不得静默进入 UI state。
- Plugin progress reporter 不引用 React、Tauri或 i18n 类型。

# 转换预览真实进度反馈

> 状态：已实现（2026-07-20）
>
> 关联设计：`docs/design-docs/agent-conversion-workbench.md`、`docs/design-docs/codex-plugin-conversion.md`
>
> 关联计划：`docs/exec-plans/completed/conversion-preview-progress.md`

## 背景

Project Plugin 转换的 Preview 会读取 Claude 配置、解析 Marketplace、准备 Project package，并对默认 Codex Home 中已启用的 Marketplace 与 Plugin package 做目录摘要校验。目录较大时，单次 Preview 可能持续数十秒。

当前前端在整个调用期间只显示“预览中…”。用户无法判断 AD 正在处理哪个阶段、是否仍有进展，容易把正常的摘要计算误认为卡死。

## 目标

- Preview 期间展示后端报告的真实阶段，不用定时轮播伪装进度。
- 展示当前 Plugin 标识、已处理数量和耗时；不展示无法证明的剩余时间或百分比。
- 进度反馈是旁路信息。通道断开或 UI 关闭不能改变 Preview 成功、失败或安全边界。
- 保持既有 Preview → Apply 门控；进度事件不得包含 mutation content、认证内容或未脱敏配置。

## 核心设计

### 每次调用独享 Tauri Channel

前端为每次 `preview_claude_to_codex_route` 创建一个 `Channel<ConversionProgressEvent>`，作为该次 invoke 的参数传入。后端同步发送事件，invoke 最终结果仍按原接口返回。

Tauri command 本身必须是 async，并把同步文件扫描与目录摘要放入 `tokio::task::spawn_blocking`。如果 command 保持同步，即使 Rust 已调用 `Channel::send`，macOS 的 IPC/WebView 调度仍可能在整个 Preview 返回前无法处理重绘，表现为系统等待光标而不是实时阶段。PlanStore 只在后台 Preview 完成并回到 async command 后写入最终计划。

不使用全局 window event：

- Channel 生命周期天然绑定一次 Preview，无需生成 correlation id。
- 连续重新预览不会串入上一次调用的事件。
- Settings 与主窗口未来复用同一命令时，不会互相接收进度。

### 语义事件，不发送 UI 文案

```rust
struct ConversionProgressEvent {
    phase: ConversionProgressPhase,
    current: u32,
    total: Option<u32>,
    item: Option<String>,
}

enum ConversionProgressPhase {
    ReadingConfiguration,
    InspectingSkills,
    InspectingPlugins,
    PreparingProjectPlugin,
    VerifyingInheritedPlugins,
    FinalizingPlan,
}
```

Rust 只发送稳定枚举和安全标识。React 使用 zh/en i18n 渲染文案。初始“解析配置实例”发生在 invoke 之前，由前端本地状态表示；其余阶段必须来自后端事件。

`item` 只允许发送 Plugin logical id 等已经出现在转换结果中的标识，不发送 auth path、token、配置内容或目录中的文件名。

### 粗粒度但真实

Preview 的主要等待来自 `PluginsPort::plan_install` 内的 Project runtime 准备和继承 Plugin 摘要。第一版不为了逐文件百分比侵入通用目录摘要器，而是在真实调用边界报告：

1. 读取与映射 Settings；
2. 盘点 Skills；
3. 盘点 Plugins；
4. 准备当前 Project Plugin；
5. 校验并镜像继承的 Base Plugins；
6. 汇总并校验 MutationPlan。

当已知 Plugin 总数时报告 `current/total`。单个目录内部耗时不线性，因此 UI 不显示百分比进度条，只显示阶段、项目和已耗时秒数。

实际实现中，`resolving_contexts` 是调用前的前端本地阶段；Rust Channel 只发送上述六个后端阶段。准备 Project Plugin 时计数表示其在当前 source Plugin 清单中的位置，继承校验时计数表示当前 Base Plugin 在全部启用 Base Plugins 中的位置。

### 旁路失败策略

Channel `send` 失败只代表进度消费者已经离开。后端忽略该发送错误并继续 Preview，最终 invoke 结果仍是唯一业务结果。进度事件不会写入 PlanStore、receipt 或操作历史。

### UI 表现

Preview 运行时在对话框固定 footer 内、操作按钮上方展示紧凑状态区。状态区不属于可滚动正文，因此即使用户停留在较长的转换清单中，也能持续看到当前阶段：

- 当前阶段标题；
- 当前 Plugin 或 `current/total`（存在时）；
- 已耗时；
- “首次校验较大的基础 Plugin 可能需要一些时间”的固定说明。

状态区使用 `role="status"`、`aria-live="polite"` 和 `aria-busy="true"`。阶段更新不抢焦点；关闭按钮继续遵循现有 busy 锁定。

## 影响与取舍

- 新增一个 route preview 的 IPC 参数和事件 schema，但不改变最终 Preview 返回值。
- route 增加可选 progress reporter；现有 Rust 单元测试和非 IPC 调用继续使用 no-op reporter。
- 为保持最小改动，第一版不提供取消、不计算字节级进度、不预测剩余时间。
- 如果后续确认目录摘要仍是主要瓶颈，可在相同事件合同下增加逐 Base Plugin 事件，无需重做前端协议。

## 验收标准

1. 慢 Preview 能依次展示真实阶段，并在 Plugin 阶段显示 logical id。
2. 长于一秒的 Preview 持续显示递增耗时，完成或失败后停止计时。
3. Channel 发送失败不导致 Preview 失败。
4. 连续两次 Preview 的事件互不串扰。
5. UI 不显示虚假百分比、预计剩余时间、认证内容或内部 mutation 数据。
6. zh/en key 对等，IPC 事件通过 zod 严格校验。
7. 进度状态始终位于固定 footer，不需要滚动到正文底部才能查看。

## 实现与验证结果

- `PluginsPort` 增加带默认实现的 progress-aware 安装入口，既有 adapter 和非 IPC 调用保持兼容。
- Codex adapter 对启用的 Base Plugins 稳定排序，并在每个目录摘要校验前报告 logical id 与 `current/total`。
- Tauri command 使用 typed `Channel<ConversionProgressEvent>`；发送失败被忽略，不改变最终 Preview。
- Tauri command 通过 `spawn_blocking` 在专用阻塞线程执行 Preview，避免目录摘要占用 IPC/WebView 调度；回归测试会在工作线程暂停时验证 async runtime 仍能收到进度。
- React 为每次 Preview 创建新 Channel，严格解析事件，并用独立 operation kind 避免把 Apply/rollback 误显示成 Preview。
- React 将实时状态放在固定 footer 的按钮上方，长清单滚动不会遮蔽当前阶段、处理对象与耗时。
- 真实 harness 隔离 Preview 用时 51.61 秒，观察到 Project Plugin 与 13 个 Base Plugins（`1/13` 至 `13/13`）的进度；未执行 Apply，项目 settings 与 Base Codex config 摘要未变化。
- 全量前端 22 个测试文件 / 104 个测试、Rust all-targets、strict Clippy、typecheck、lint 和 production build 通过。测试环境没有可用浏览器实例，视觉层以组件 DOM、无障碍和 production build 验证，未完成截图级检查。

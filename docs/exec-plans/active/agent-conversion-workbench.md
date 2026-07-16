# 完善 Agent 配置转换工作台与风险确认

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

把 Claude Code → Codex 转换从“少量 settings 写入 `config.toml`”升级为完整的 artifact 工作台。用户选择 Project 后能看到真实的源/目标资源路径，逐项了解 Settings、Permissions、Skills、Plugins 和其他配置将如何处理；能真正转换项目 Skill；危险权限扩大必须经过独立确认并由后端强制校验。Claude Code 源保持不变，所有 Codex 写入继续具备备份和回滚。

## 确认状态

- [x] **用户已确认** — 评审 HTML 路径：`docs/exec-plans/active/agent-conversion-workbench.html`
- [x] 用户已确认，开始执行（2026-07-16 08:12 CST）

## 假设

- “整体优化”以 `docs/design-docs/agent-conversion-workbench.md` 为设计基线。
- 能力对等表示用户任务、处理覆盖率和安全保证对等，不表示所有 Claude 字段都必须写入 Codex `config.toml`。
- 每个可管理的源 artifact 必须出现一次；无法安全转换可以是最终状态，静默遗漏不可以。
- Project scope 绑定 AD 当前项目；User / Project 继续单作用域隔离。
- 单一 installation 是常态，选择器默认隐藏；多 installation 时仍可在高级区选择。
- 转换规则和 resolver 全部内置，不支持用户配置。
- Plugin marketplace/授权没有安全可调用接口时只生成明确的后续设置，不伪造自动安装成功。
- 已有 Codex target 默认 merge/保留；显式覆盖仍受 typed resolution、预览和备份保护。
- 用户此前授权计划确认后的全自动托管：实现、测试、代码审查、生产构建、备份旧应用、安装和只读真实项目验收均自动完成；不推送远端。

## 影响范围

预计触达以下模块，实施时按最小职责拆分，避免继续扩大单文件：

- `src-tauri/src/agents/conversion.rs`：artifact endpoint、resolution、risk 与 carrier mapping contract。
- `src-tauri/src/agents/conversion_route.rs`：完整 inventory、跨 target port 组合 plan、summary 和 required acknowledgements。
- `src-tauri/src/agents/plan_store.rs`、`execution.rs`：存储并校验 plan-bound typed acknowledgements。
- `src-tauri/src/commands/agents.rs`：替换通用 `confirmed: bool` 的 conversion apply IPC。
- `src-tauri/src/agents/claude_ports/{skills,plugins}.rs`、`codex_skills.rs`：Project Skill route 与 Plugin inventory 修正。
- `src-tauri/src/commands/skills.rs`：Project-only Plugin key union 和来源信息。
- `src-tauri/tests/{conversion_route,conversion_execution}.rs` 与相关 unit tests：TDD fixtures、风险门禁、Skill apply/rollback、Plugin 盘点。
- `src/lib/{agentTypes,tauri}.ts`：严格 IPC schema、resolution 和 acknowledgement 类型。
- `src/components/AgentConversionDialog.tsx`：工作台布局；必要时拆出 artifact group、resolution 与危险确认组件。
- `tests/components/AgentConversionDialog.test.tsx`：交互和可访问性回归测试。
- `src/i18n/locales/{zh,en}.json`：所有新增文案与错误提示。
- `docs/design-docs/multi-agent-architecture.{md,html}`、`docs/product-specs/multi-agent-support.{md,html}`：实现后同步 as-built contract。
- `docs/design-docs/agent-conversion-workbench.{md,html}`：若实施发现结构偏差，先更新 MD；批准后 HTML 保持冻结基线。

## 关键决策

1. 以 artifact carrier 路由替代“全部写进 config.toml”的心智模型。Settings、Skills、Plugins 使用各自 target port。
2. 安装实例和实际资源路径分开展示。installation 只用于选择 config home；adapter 解析并返回真实 location。
3. `requires_input` 必须有 typed resolver。没有 resolver 的项目改为 `unsupported` 或 manual setup，避免死状态。
4. 危险确认是 backend-owned plan requirement。Apply 接收 acknowledgement 集合，PlanStore 按 plan 精确校验；移除 conversion 的通用 boolean 门禁。
5. Plugin 采用“完整盘点 + 安全后续设置”。只有官方/本地能力可验证且无授权绕过时才自动安装。
6. 先用 `sofampy` 形状的隔离 fixture 完成 Apply/rollback；真实 `sofampy` 只做只读 preview 和 digest 验证。

## 进展

- [ ] (待开始) Milestone 1：建立失败测试与完整 inventory fixture（验证标准：当前实现因危险 ack 可绕过、Skill 无 mutation、Project-only Plugin 遗漏、UI 路径含混而失败）。
- [ ] Milestone 2：实现 artifact endpoint、resolution、carrier、summary 和 risk contract（验证标准：Rust serde/plan invariants 与 TS zod contract tests 通过）。
- [ ] Milestone 3：实现 backend-owned acknowledgement 门禁（验证标准：空 ack、错误 ack、旧 plan ack、通用 boolean 路径全部拒绝；正确危险 ack 才能 Apply）。
- [ ] Milestone 4：完成 Project Settings/Permissions/Skills/Plugins 路由（验证标准：Skill 可 Apply/rollback；Plugin key union 正确；不支持项无静默遗漏）。
- [ ] Milestone 5：重构转换 UI 为工作台（验证标准：单 installation 隐藏选择器；真实 locations、分组 summary、inline resolver 和危险确认可访问）。
- [ ] Milestone 6：同步文档并执行多轴代码审查与修复（验证标准：设计、产品规格、i18n 和实现一致，无 P0/P1 review finding）。
- [ ] Milestone 7：全量门禁、构建、安装和只读原生验收（验证标准：全部测试/构建通过，旧应用已备份，`sofampy` preview 完整且所有源/目标 digest 不变）。
- [ ] 完成结果回顾并将 MD + 冻结 HTML 一起移到 `docs/exec-plans/completed/`。

## 意外发现

- 发现：当前 Apply 只执行 `applyConversionPlan(planId, true)`，后端只验证 boolean，无法证明用户确认了具体危险权限。
  证据：`AgentConversionDialog.tsx` 的 `applyConversion` 无确认 UI；`commands/agents.rs` 的 `apply_conversion_plan` 只调用 `require_confirmation(confirmed, ...)`。
- 发现：Project Skill 已有明确 Codex target port（`.agents/skills`）和可回滚 symlink mutation，但 conversion route 只把它标为 `requires_input`，没有组合 target plan。
  证据：`codex_skills.rs::plan_install` 支持 User/Project；`map_skill_artifact` 仅创建展示 artifact。
- 发现：Project Plugin 列表只遍历 User `enabledPlugins` 的 key，再套 Project override，Project-only key 会消失。
  证据：`commands/skills.rs::list_plugins` 从 `global.into_iter()` 生成 result。
- 发现：Codex PluginsPort 只声明 User scope，且 install 返回 Unsupported；当前 route 却把 Project Plugin 标为 `requires_input`，形成无 resolver 的死状态。
  证据：`codex_plugins.rs::scopes` 只有 User；Plugin install 依赖 marketplace/auth flow。
- 发现：官方 Codex 载体确认 Project settings 为 `.codex/config.toml`、Project skills 为 `.agents/skills`，Plugins 通过 marketplace/授权流程；官方 import 同样按 artifact 分类并保留源 Agent。
  证据：Codex Configuration Reference、Build skills、Plugins、Import from another agent 官方文档（2026-07-16 核验）。
- 发现：真实 `sofampy` 当前包含 9 类 settings、116 条 permission rule、1 个 Project Skill、6 个 Plugin 声明和 2 个 marketplace 来源，而 target `config.toml` 只有 model/context window。
  证据：只读 filesystem inventory 与 key/count 汇总；未读取或输出任何 credential/runtime 状态。

## 决策日志

- 决策：采用“完整 inventory + 多载体 route + typed risk acknowledgement”作为统一修复，而不是继续追加个别字段映射。
  理由：用户反馈跨 UI 表意、转换覆盖率和安全门禁，局部补丁会继续保留错误心智模型。
  日期/作者：2026-07-16 / 用户批准，Codex 记录
- 决策：危险权限确认由 plan requirement 驱动，前端仅负责展示和提交 acknowledgement。
  理由：风险是否存在只能由 backend-owned mutation plan 判断，不能依赖前端当前下拉值。
  日期/作者：2026-07-16 / 用户批准，Codex 记录

## 结果回顾

待实施完成后填写：实际覆盖率、风险门禁证据、真实项目只读验收、构建与安装结果、遗留的 Plugin 授权边界。

## 上下文和方向

当前 `ClaudeToCodexRoute` 已有 User/Project scope 隔离、Settings merge、source read-only digest、backend-owned MutationPlan、backup/receipt/rollback。它也会读取 Skills 和 Plugins，但 collection artifact 只用于展示，未参与 plan。前端把 decisions 放在扫描前，展示扁平 artifact id/message，实际 source/target `ResourceLocation` 没有进入 conversion IPC。

本次保留既有 ExecutionEngine，不重写文件安全层。重点是让 conversion route 组合多个 target port 生成的 mutation、让 plan 携带风险要求，以及让 UI 根据结构化 contract 渲染。结构必须能被未来内置 route 复用，但不引入动态 adapter 或用户规则 DSL。

## 工作计划

### Milestone 1 — 失败测试与 fixture

先扩展 `sofampy` 形状的临时 fixture：Project local settings、Project-only plugin、marketplace、Skill symlink、已有 Codex project config。Rust 测试先证明 source inventory 的每个 artifact 有 disposition；前端测试先证明单 installation 不应显示误导下拉框、危险 preset 必须弹确认。所有写测试继续使用 `AD_HOME` / `TempDir` 隔离。

### Milestone 2 — Conversion contract

在 Rust 定义 conversion endpoint/location、typed resolution、risk level、summary 和 acknowledgement requirement；在 TS 添加严格 zod schema。route mapper 返回 i18n key/args 或稳定 reason code，避免把英文 backend message 直接作为最终 UI 文案。必要时将 `conversion.rs` 拆为 contract/mapping 小模块，保持单文件职责。

### Milestone 3 — 后端风险门禁

让 PlanStore 随 plan 保存 required acknowledgements。替换 `apply_conversion_plan(plan_id, confirmed)` 为 typed acknowledgement request。claim 前检查精确集合，失败不消费 plan。为 safe、confirmation、dangerous、replay、plan expiry 和 digest conflict 写测试。普通 Agent plan/rollback 的现有接口不随意扩大修改。

### Milestone 4 — 多载体 route

Settings 保留现有 merge；权限 preset 生成 risk metadata，并把细粒度规则单独报告。Skills 对已解析本地 source 调用 Codex SkillsPort `plan_install` 并安全组合 read-set/mutations。Plugin inventory 修为 User/Project union 并携带来源；Codex 无安全自动安装时输出 manual setup，而非无操作的 `requires_input`。组合计划后统一验证 source read-only、target-only write-set、scope 和 digest。

自动 compaction 映射只有在 Claude/Codex 官方语义可证明时加入；否则保持明确 unsupported。不得以“看起来相似”推断转换公式。

### Milestone 5 — 工作台 UI

将对话框改为 scope → scan → grouped resolution → review/apply → receipt。单 installation 隐藏 selector；多 installation 收进高级区。展示 actual `ResourceLocation.path`、summary counts 和 target carrier。危险 plan 的 Apply 打开独立确认组件，逐项显示风险和 exact mutation；取消不提交 acknowledgement。所有文案同步 zh/en。

### Milestone 6 — 文档与审查

更新 multi-agent architecture 和 product spec 的 as-built 内容。执行 code-review-and-quality 多轴审查，重点检查风险门禁绕过、source 写入、symlink path validation、跨 scope 泄漏、unsupported 假成功、i18n/accessibility；修复所有高优先级发现。

### Milestone 7 — 交付

运行定向与全量门禁，生产构建。安装前把 `/Applications/AD.app` 备份到 `~/.ad/app-backups/`，复制新 bundle 并清理 quarantine，检查签名和版本。启动已安装应用，对真实 `sofampy` 只执行 scan/preview，不点击 Apply；记录源、目标和 Skill location digest/metadata 前后不变。

## 验证和验收

定向测试：

```bash
pnpm test -- tests/components/AgentConversionDialog.test.tsx tests/lib/agentTypes.test.ts
cargo test --manifest-path src-tauri/Cargo.toml --test conversion_route
cargo test --manifest-path src-tauri/Cargo.toml --test conversion_execution
```

全量门禁：

```bash
pnpm typecheck
pnpm lint
pnpm test
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm tauri build
```

行为验收：

1. Project scope 显示 canonical project 和真实载体，不把 installation root 冒充本次路径。
2. 每个 inventory item 恰好处于自动、待决策、unsupported、conflict 或 unchanged；无静默遗漏和无 resolver 的 `requires_input`。
3. 安全 Settings + Skill 可在同一 plan 中 Apply，所有目标先备份；rollback 恢复文件和 symlink。
4. 选择 `never + danger-full-access` 后，空/错误 acknowledgement 被后端拒绝；独立确认后才成功。
5. Project-only Plugin 和 marketplace 可见；需授权项明确显示下一步，不报告假成功。
6. Claude source 始终字节不变；target 或 source 外部变化使 plan 失效。
7. `sofampy` 真实只读 preview 覆盖设计基准，未发生任何写入。

## 幂等性和恢复

Preview 可重复，任何 context/resolution 变化都会废弃旧 plan。Apply 由 PlanStore 防 replay，write 前完成全量备份；中途失败沿 manifest 逆序补偿。测试和应用验证使用临时目录，真实项目只读。安装前保存旧 `.app`；新应用启动或验收失败时恢复备份。Git 采用增量提交，不使用 destructive reset/checkout。

## 接口和依赖

不新增第三方依赖。里程碑结束时应存在等价接口：

```rust
enum ConversionRiskLevel { Safe, Confirmation, Dangerous }

struct ConversionEndpoint {
    resource: ResourceRef,
    location: ResourceLocation,
}

struct AcknowledgementRequirement {
    code: AcknowledgementCode,
    risk: ConversionRiskLevel,
}

struct PlanAcknowledgement {
    code: AcknowledgementCode,
    accepted: bool,
}

fn apply_conversion_plan(
    plan_id: PlanId,
    acknowledgements: Vec<PlanAcknowledgement>,
    plans: State<'_, PlanStore>,
) -> Result<OperationReceipt, AgentError>;
```

具体类型名可在不改变语义的前提下调整。复用 `ResourcePort`、`SettingsPort`、`SkillsPort`、`PluginsPort`、`MutationPlan`、`ExecutionEngine`、现有 Tailwind/UI primitives 和 zod；不引入新的状态管理或 UI 库。

## 里程碑规划

1. Contract + failing tests：可独立验证 inventory/risk schema。
2. Backend gate：可独立证明危险确认不可被 generic boolean 绕过。
3. Carrier routing：可独立验证 Project Skill、Plugin inventory 和 rollback。
4. Workbench UI：可独立通过组件测试和开发模式验收。
5. Review + shipping：全量门禁、构建、安装和真实项目只读证据。

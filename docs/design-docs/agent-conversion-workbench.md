# Agent 配置转换工作台设计

> 状态：已实现（2026-07-20）
>
> 范围：Claude Code → Codex；同时约束未来内置 Agent 转换路线
>
> 关联计划：`docs/exec-plans/completed/agent-conversion-workbench.md`

## 结论

当前转换能力的主要问题不是“Project 配置路径错误”，而是产品把“转换”简化成了少量 settings 字段写入 `config.toml`：

- 安装实例路径被放在主界面，用户会把 `~/.claude` / `~/.codex` 误解为本次 Project 转换的实际文件路径。
- `.claude` 中 Settings、Skills、Plugins 和 marketplace 等不同配置载体曾被混在一个扁平 artifact 列表里；现在 Settings、Skills 与 package-first Plugin route 都能进入计划。
- `requires_input` 只是状态文本，Skills、Plugins 等项目没有对应操作，用户无法完成决策。
- 危险权限预设只有红色说明；Apply 直接传入通用 `confirmed: true`，后端无法区分普通应用确认和 `never + danger-full-access` 风险确认。
- Project-only Plugin 声明可能因列表实现只遍历 User 插件键而被遗漏。

新设计把转换定义为一个“配置工作台”：先完整盘点源配置，再按目标载体路由，每个源 artifact 必须得到一个可解释、可操作的最终状态。`config.toml` 只是 Settings 载体之一；Project Skills 写入 `.agents/skills`，Project Plugins 安装到 AD Managed Project Codex Home 的真实 marketplace/cache/config 契约，不把它们伪装成普通 TOML 字段。

## 用户反馈对应的产品行为

| 用户反馈 | 新行为 |
|---|---|
| 选择 Project 后仍显示 `~/.claude` / `~/.codex` | 单一安装实例时隐藏选择器；多实例选择器收进“高级：配置实例”。主区显示本次真实资源路径 |
| `.claude` 很多配置，但 `config.toml` 很少 | 按 Settings、Permissions、Skills、Plugins、其他配置分组，展示每项目标载体和处理结果 |
| “危险权限扩大必须明确确认”但未提示 | 选择危险预设后，Apply 必须经过独立风险确认对话框；后端校验 typed acknowledgement，通用 boolean 不再有效 |
| 需要提示无法转换字段、备份和回滚 | 预览汇总所有无法转换项及原因；Apply 前列出写入目标；成功页展示备份和回滚入口 |
| 保留 Claude Code 配置不动 | 所有 Claude source 都是 read-only precondition，write-set 只能包含 Codex target |

## 目标与非目标

### 目标

1. 同一作用域内，每个 AD 可管理的 Claude artifact 恰好出现一次，状态为自动转换、待决策、无法转换、冲突或无需变更。
2. 所有 `requires_input` 都有真实操作；没有 resolver 的项目必须标为 `unsupported`，不能假装“等待输入”。
3. 不同配置类型写入各自的 Codex 载体，并展示真实源/目标路径。
4. 危险权限扩大必须同时有 UI 二次确认和后端 plan-bound acknowledgement。
5. 所有写入继续复用 digest 检查、写前备份、原子写、receipt 和 rollback。
6. 转换规则由内置 route/adapter 提供；新增未来 Agent 时复用通用工作台，不新增散落的 Agent 分支。

### 非目标

- 不支持用户通过配置文件、脚本或 manifest 自定义转换规则。
- 不把 Claude 与 Codex 不同语义的字段强行一一对应。
- 不绕过 Codex marketplace、connector 或第三方服务授权流程。
- 不读取 auth、token、session、history、logs、cache 或数据库。
- 不自动删除、移动或修改 Claude Code 源配置。

## 核心模型

### 1. Endpoint 不是本次资源路径

`AgentInstallation` 表示一个配置实例，例如 `CODEX_HOME=~/.codex`。Project scope 仍然需要该实例来解析 Codex 行为，但实际目标可能位于项目中的 `.codex/config.toml` 或 `.agents/skills`。

UI 规则：

- 每端只有一个 installation：不显示下拉框，只在高级信息里显示配置实例。
- 任一端有多个 installation：显示“Claude Code 配置实例 / Codex 配置实例”，附带“用于选择配置 home，不代表本次写入路径”的说明。
- 预览完成后，主区按 artifact 展示 adapter 解析出的 `ResourceLocation.path`；UI 不拼接路径。

### 2. Artifact 路由到 Carrier

```text
Claude source inventory
  ├─ Settings / permissions ─────────────→ <project>/.codex/config.toml
  ├─ Project skills ─────────────────────→ <project>/.agents/skills/<name>
  ├─ User skills ────────────────────────→ $HOME/.agents/skills/<name>
  ├─ Project Plugins / marketplaces ─────→ ~/.ad/codex-homes/<id> marketplace/cache/config
  ├─ Instructions / hooks / MCP / agents → 对应 Codex resource port（存在已确认规则时）
  └─ 无等价能力 ─────────────────────────→ unsupported + 原因 + 替代建议
```

通用层只理解 `ResourceKind`、source/target location、disposition、resolution 和 risk；具体路径与映射由 source/target adapter 和内置 route 负责。

### 3. 转换项必须携带可执行状态

建议将 `ConversionArtifact` 扩展为：

```rust
struct ConversionArtifact {
    id: String,
    kind: ResourceKind,
    source: ConversionEndpoint,
    target: Option<ConversionEndpoint>,
    disposition: ArtifactDisposition,
    resolution: Option<ResolutionRequirement>,
    risk: RiskLevel,
    message_key: String,
    message_args: BTreeMap<String, String>,
}

struct ConversionEndpoint {
    resource: ResourceRef,
    location: ResourceLocation,
}

enum ResolutionRequirement {
    SelectTargetModel,
    SelectPermissionPreset,
    ConfirmLocalSkillSource,
    CompletePluginSetup,
    ResolveConflict,
}

enum RiskLevel {
    Safe,
    Confirmation,
    Dangerous,
}
```

`message_key + args` 用于稳定 i18n；开发者诊断仍使用英文 `AgentError.message`。route preview 同时返回按 disposition/risk 统计的 summary 和所有实际 resource locations。

### 4. Resolution 是内置规则，不是自由 JSON

前端提交 route 定义的 typed resolution：目标模型、权限预设、Skill 的确认/跳过、Plugin 的后续设置状态等。后端只接受该 route allowlist 中的值，并重新生成 plan；前端不能提交任意路径、TOML key 或 mutation content。

状态约束：

- `requires_input`：必须有 `resolution`，用户可选择转换或明确跳过。
- `unsupported`：没有自动转换能力，提供原因和可行替代动作；不阻塞其他安全项目 Apply。
- `conflict`：默认保留目标；只有 route 提供安全 merge/replace 策略时才允许用户处理。
- `mapped/exact`：自动进入 plan，仍受目标 digest 和备份保护。
- `partial`：可移植 package 组件自动进入 plan，不兼容组件保留为明确 residual。
- `unchanged`：不产生 mutation。

### 5. 风险确认绑定后端计划

现有 `apply_conversion_plan(plan_id, confirmed: bool)` 不足以表达具体风险。改为：

```rust
struct PlanAcknowledgement {
    code: AcknowledgementCode,
    accepted: bool,
}

fn apply_conversion_plan(
    plan_id: PlanId,
    acknowledgements: Vec<PlanAcknowledgement>,
) -> Result<OperationReceipt, AgentError>;
```

preview 保存 backend-owned `required_acknowledgements`。Apply 时 PlanStore 必须验证：

- acknowledgement 与当前 plan 精确匹配；不能用未知 code、旧 plan code 或通用 `true` 替代。
- `never + danger-full-access` 必须要求 `dangerous_permission_expansion`。
- scope、project path、写入目标或 resolution 改变后旧 plan/acknowledgement 失效。
- 未满足要求时返回结构化 `confirmation_required`，不 claim plan、不写文件。

UI 在危险 Apply 前显示独立对话框，明确列出项目路径、`approval_policy = "never"`、`sandbox_mode = "danger-full-access"`、影响和 source 不变声明。确认按钮使用危险样式，不能由普通 Apply 点击直接代替。

## `sofampy` 基准行为

真实项目 `/Users/yuanxuan/python_workspace/sofampy` 作为只读验收样本，当前 inventory 为：

- Settings keys：`autoCompact`、`compactThreshold`、`maxContextTokens`、`model`、`permissions`、`skipDangerousModePermissionPrompt`、`statusLine`、`enabledPlugins`、`extraKnownMarketplaces`。
- Permissions：115 allow、1 ask、0 deny，默认模式包含 `bypassPermissions`。
- Project Skill：`.claude/skills/skillspy_delivery` symlink。
- Plugins：6 个项目声明，2 个 marketplace 来源。

预期呈现：

| 来源 | 目标载体 | 首选处理 |
|---|---|---|
| `maxContextTokens` | Project `config.toml` | 自动映射 `model_context_window` |
| Claude model | Project `config.toml` | 选择 Codex model 或跳过 |
| permissions default mode | Project `config.toml` | 选择内置安全预设；危险预设二次确认 |
| 116 条细粒度 permission rules | Codex permissions/rules | 无损规则未经确认时单独标为 unsupported，不用一个宽泛预设冒充全量迁移 |
| `autoCompact` / `compactThreshold` | `model_auto_compact_token_limit` | 仅在官方语义和计算方式可证明时映射，否则明确 unsupported |
| `skillspy_delivery` | `.agents/skills/skillspy_delivery` | 确认已解析的本地 symlink source 后生成可回滚安装 mutation |
| 6 Plugins / 2 marketplaces | AD Managed Project Codex Home | 修复项目声明盘点；兼容 package 自动安装，partial/LSP-only 给出逐项 residual |
| `statusLine` / dangerous prompt skip | 无已确认等价载体 | unsupported + 原因 |

这一基准要求所有项目都可见；允许某项最终不写入，但不允许静默遗漏。

## 工作台交互

1. 顶部选择 User / Project；Project 显示 canonical 项目路径。
2. 高级区只在需要时选择配置实例。
3. 点击“扫描配置”后展示总览：自动转换、需要决策、无法转换、冲突、危险变更。
4. 按载体分组展示 artifact；每项显示 source path、target path、处理状态和 resolver。
5. 用户完成或跳过可处理决策后重新生成 plan。Apply 区只汇总实际 write-set。
6. 普通计划直接在 review footer 确认；危险计划打开独立风险确认。
7. 成功页显示写入项、备份路径、未转换项摘要和 rollback。

## Plugin 与 Skill 边界

- Project Plugin 盘点必须使用 User + Project key union，再应用 Project override；不能只遍历 User key。
- Project Codex Plugin 不调用 Codex 命令。AD 在专用 stage 校验并复制/转换 package，通过共享执行引擎依次写入 marketplace、versioned cache，最后写 `config.toml` 激活；User install 仍使用 Codex marketplace flow。
- Project Plugin 使用派生 `CODEX_HOME`，不修改默认 `~/.codex` 的 config/marketplace/cache。Base 配置只读合成，已启用 Base package 独立镜像；文件登录通过受控 `auth.json` symlink 复用。
- Keychain-only Base 无法跨自定义 Home 复用，Preview 必须阻塞。项目 API Key、model/env 差异由可选 Profile 覆盖，不进入 Plugin 转换 payload。
- 原生 Codex package 原样复制；Claude Commands 转为 Skills。Skills/Commands/MCP 可自动承载时为 mapped；同时含 LSP、Hook、App 或 Agent 等未验证组件时为 partial 并保留具体 residual；LSP-only 为 unsupported。
- Skill 的本地目录/symlink source 可由两个 adapter 安全解析，目标 Port 已支持 Project `.agents/skills`，因此应纳入真实 mutation plan。
- Plugin residual 不应阻塞 Settings/Skills 和可移植 Plugin 组件的安全写入；receipt 必须保留未完成项目摘要。

## 安全、备份与恢复

- source inventory、locations 和 digests 全部进入 read-only precondition。
- write-set 只能由 target port 生成；所有目标先备份，备份失败则零写入。
- 文件与 symlink 都必须有恢复记录；回滚检查 apply 后 digest，避免覆盖外部修改。
- Unsupported/manual 项不产生写入，因此没有“备份源”的伪承诺；它们记录在 conversion report/receipt 中。
- preview、失败确认和取消确认都不得改变 source/target。

## 面向未来 Agent 的扩展点

新增内置 Agent 或新 route 时只需提供：

1. source adapter 的 inventory ports；
2. target adapter 的 target ports 与可管理载体；
3. built-in route 的 artifact mapper、resolution schema 和 risk policy；
4. operation-level contract fixtures。

通用工作台根据结构化 artifact/resolution/risk 渲染，不根据 `agentId` 硬编码业务分支。route 可以是 Claude → Codex、Codex → 未来 Agent，但用户仍不能动态配置 adapter 或转换规则。

## 验收标准

1. 单 installation 的 Project 转换主界面不再突出 `~/.claude` / `~/.codex`；实际资源路径可见。
2. `sofampy` 基准 inventory 中的 9 类 settings、1 个 Skill、6 个 Plugin 声明和 2 个 marketplace 来源均有明确呈现或归并说明。
3. Project Skill 可预览、Apply、备份和 rollback 到 `.agents/skills`。
4. 选择危险权限后，普通 Apply 不写盘；必须完成独立确认，且后端缺 acknowledgement 时拒绝。
5. Project-only Plugin 不遗漏；兼容 package 自动安装到派生 Project Codex Home，partial/LSP-only 逐项说明剩余原因。
6. Apply 前后 Claude source digest 不变；目标外部修改会使 plan 失效。
7. zh/en 文案 key 对等，前端 zod schema 严格验证 IPC。

## 依据

- Codex Project settings 使用 `.codex/config.toml`，且只在 trusted project 中加载；部分 machine-local key 不能由项目层覆盖：<https://developers.openai.com/codex/config-reference>
- Codex Project/User Skills 位于 `.agents/skills`，并支持 symlink：<https://developers.openai.com/codex/skills>
- Codex Plugins 通过 marketplace 浏览器安装，部分插件需要 connector 授权：<https://developers.openai.com/codex/plugins>
- 官方 import 按 instructions、settings、skills、plugins、MCP、hooks、subagents 等 item 迁移，保留原 Agent，并对需授权项目显示后续设置：<https://developers.openai.com/codex/import>

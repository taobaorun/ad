# Codex Plugin 自动转换与项目隔离运行时

> 状态：已实现（2026-07-20）
>
> 关联设计：`docs/design-docs/agent-conversion-workbench.md`
>
> 关联计划：`docs/exec-plans/completed/codex-plugin-project-conversion.md`

## 结论

AD 应直接实现 Claude Code Plugin → Codex 的转换，不调用 `codex plugin ...`、`codex app-server` 或其他 Codex 子进程。转换结果对齐 Codex 官方开源实现的 marketplace、Plugin cache 与配置契约，同时继续纳入 AD 的 Preview、MutationPlan、备份、补偿与 rollback。

Codex 当前没有原生的项目级 Plugin enable/disable。为提供完整 Plugin 语义而不污染默认 `~/.codex`，Project 转换新增一等运行模式：**AD Managed Project Codex Home**。

```text
默认 Codex Home（配置/Plugin 只读继承源，认证共享源）
~/.codex/config.toml + auth.json
          │ 配置由 AD 合成；登录态通过引用复用，不复制凭据
          ▼
~/.ad/codex-homes/<project-id>/
├── config.toml                  # 可选 Base 配置 + Project settings/Plugin overlay
├── .ad/runtime-manifest.json    # applied 继承策略、Profile、Project overlay
├── auth.json -> ~/.codex/auth.json # 文件型 ChatGPT 登录态共享引用
├── plugins/cache/...            # 仅此 Project runtime 可见
├── .tmp/marketplaces/...        # 独立 marketplace materialization
├── state/                       # Codex 自有，AD 不读取内容
└── session/logs/...             # Codex 自有，AD 永不纳入转换或备份

AD 启动：CODEX_HOME=<project-home> codex  （cwd=<project>）
普通启动：CODEX_HOME 未设置 → ~/.codex，Project Plugin 不生效
```

这不是 Codex 原生 Project Plugin，而是 AD 通过隔离的 Codex installation/runtime 提供的项目级效果。它只对从 AD 选择该项目运行时启动的 Codex 会话生效；从 Finder、IDE 或普通终端启动的默认 Codex 不会自动使用该 home。

项目转换按以下顺序处理：

1. Disabled Plugin：跳过，显示无需变更。
2. Codex-compatible package：安装到项目专属 Codex Home，保留完整 Plugin 身份与组件。
3. 无法完整映射但有可移植组件：生成 partial Codex package，自动保留 Skills、Commands→Skills 与 MCP 等可移植内容，并列出 residual。
4. Claude 专有组件（当前重点为 `lspServers` / `.lsp.json`）：显示具体不兼容原因，不报告假成功。

User scope 自动安装仍交给 Codex 官方 marketplace flow；本轮自动化只用于 Project runtime。Project scope 不写入 `~/.codex` 的配置、marketplace 与 Plugin cache。共享登录态是明确例外：Codex 自己的 token refresh 可以更新用户级认证，AD 不读取凭据内容。

## 已实现状态

`CodexPluginsPort::plan_install` 已为派生 Project runtime 实现真实目录/config MutationPlan；User installation 仍返回结构化 Unsupported。`map_plugin_artifact` 现在区分：

- source 已禁用，无需转换；
- package 可以自动安装；
- Plugin 含 Codex 没有的组件，确实无法等价转换。

截图中的 `jdtls-lsp@claude-plugins-official` 在项目 `.claude/settings.local.json` 中实际为 `false`，其安装包只声明 Claude `lspServers.jdtls`。当前预览应显示“源已禁用，跳过”；若改为 `true`，则显示“Codex 无原生 LSP Plugin 组件”。

## 官方能力与限制

### 原生项目级载体

- `.codex/config.toml`：从仓库根到 cwd 逐层加载，最近层优先；只在 trusted project 生效。
- `.agents/skills`：从 cwd 向仓库根扫描，可按目录限定作用域。
- `.codex/hooks.json` / `.codex/config.toml` hooks：受 project trust 控制。
- Project `mcp_servers`：可写入 `.codex/config.toml`。
- `.agents/plugins/marketplace.json`：仓库可声明 Plugin catalog，但不提供 per-project enable state。

### Plugin 状态仍属于 Codex Home

Codex 官方 importer 即使按 `cwd` 检测 Project Plugin，仍把 marketplace、Plugin cache 和 `[plugins."<id>"] enabled` 写入当前 `CODEX_HOME`。默认情况下这就是 `~/.codex`，因此会影响同一 home 下的其他项目。

`CODEX_HOME` 是公开、稳定的环境变量，但它替换的是整个 Codex state root，不只是 `config.toml`。官方列出的范围包括 config、auth、logs、sessions、skills 和 standalone package metadata。Codex 没有“自定义 CODEX_HOME 再自动继承 `~/.codex`”的配置项。

因此：隔离 home 可提供 Project Plugin 行为；跨 home 配置继承必须由 AD 实现。

## Project Codex Home 身份

每个注册项目可选择一个 AD-managed Codex runtime：

```rust
struct ProjectCodexRuntime {
    project_id: String,
    project_path: String,
    base_installation_id: InstallationId,
    runtime_installation_id: InstallationId,
    runtime_home: PathBuf,
    base_config_digest: Option<ContentDigest>,
    generated_config_digest: Option<ContentDigest>,
    profile_id: Option<String>,
    applied_inherit_base_config: bool,
    manifest_digest: Option<ContentDigest>,
}
```

路径使用 canonical project path 的稳定 id，而不是可碰撞的 basename：

```text
~/.ad/codex-homes/<project-id>/
~/.ad/state/codex-project-runtimes/<project-id>.json
```

Project runtime 是一个派生 Agent installation。Settings、Skills、Plugins、Process 与 Launch 的所有操作都必须绑定它自己的 installation id；不能因 agent id 同为 `codex` 而回退到默认 `~/.codex`。

删除项目注册时默认只解除绑定，不自动删除 runtime home。清理必须是独立、可预览、可恢复的用户动作。

## 配置继承与合成

### 合成输入与项目级策略

```text
Base user config       所选默认 Codex installation/config.toml（可选、只读）
Project Plugin overlay Claude Project Plugin 转换结果（AD-owned）
Native project config  <project>/.codex/config.toml（Codex 自己作为更高层加载）
```

每个注册 Project 持久化 desired `inheritBaseConfig`，默认开启；runtime manifest 持久化 applied 策略、applied Profile 与明确的 Project Plugin overlay。切换只改变 desired 状态，必须经过 Preview → Apply 才重写 runtime config 与 manifest。开启时 AD 以 Base config 为起点；关闭时以空 TOML 为起点，只写 AD 必需字段、显式 Project settings 映射与 Project Plugin overlay。认证共享不受该开关控制。

AD 只物化前两项和明确的 Project settings 映射到 project home `config.toml`。原生项目 `.codex/config.toml` 不复制、不改写，继续由 Codex 按官方 precedence 加载：Project config 高于 project home 的 user layer。

### 合成规则

1. 每次 Preview 读取 desired 策略和相关 digest；Launch 只验证已应用 manifest/config 与 desired 是否一致，不执行隐式刷新。
2. 使用 TOML 结构化深合并；Project overlay 只允许修改 `[marketplaces]`、`[plugins]` 与实现 Plugin 所需且已确认的 feature keys。
3. Base 中未知字段原样保留；不通过 typed DTO 重建整份配置。
4. 仅在继承开启时，Base 已启用的 Plugin 视为继承项。其 package/marketplace 必须镜像到 project home，否则生成结果为 conflict，不能留下 enabled 但无 package 的悬空状态。
5. Project overlay 的同名 marketplace 与 base 来源一致时复用；来源不同为 conflict。
6. Base config 中已知 path-bearing keys 的相对路径转换为基于 Base Home 的绝对路径；未知字段原样保留，不猜测其路径语义。
7. 生成的 `config.toml` 是 AD-owned artifact。若它在上次生成后被外部修改，自动刷新不得覆盖；用户可选择重新生成或把差异纳入 project overlay。

合成不是一次性复制。Base config 改变后，下次 AD 启动显示并应用 refresh plan，因此用户无需在每个 project home 手工重复配置。

## Plugin package 与 marketplace 继承

Project home 不通过可写 symlink 指向 `~/.codex/plugins` 或 `~/.codex/.tmp/marketplaces`。否则 Codex 在 isolated runtime 中执行 refresh、install 或 uninstall 时仍可能修改默认 home。

AD 对 package 使用内容寻址的受控镜像：

- base 中启用且 project runtime 需要的 Plugin，从 base cache 校验后复制到 project cache；
- Claude Project Plugin 转换后的 package 直接安装到 project cache；
- APFS 上可使用 clonefile/COW 降低磁盘占用，但语义必须等同独立副本；
- 同版本同 digest 幂等；同版本不同 digest 为 conflict；
- marketplace Git checkout 与本地 source 均在 project home 内物化或复制，不保留可写回默认 home 的链接。

目录 target 继续经过 ExecutionEngine 的 stable tree digest、staging + rename、backup、失败补偿与 rollback。

## 认证、Profile 与敏感数据

ChatGPT 登录态属于用户身份，不属于项目配置。所有 Project runtime 默认复用所选 Base Codex installation 的登录态，不要求每个项目重新登录。API Key、模型或其他项目差异化认证不进入 Plugin 转换，由 AD 的 Profile 功能统一覆盖，并在 Launch 时选择。

AD 的认证边界：

- 文件型认证使用受控符号链接：`<project-home>/auth.json -> <base-home>/auth.json`。AD 只校验 link 的源、目标和文件类型，不打开、解析、复制、diff、backup 或记录凭据内容。
- 该链接代表有意共享：Codex token refresh 会更新 Base `auth.json`；这是“同一用户身份”的预期行为，不再计入配置/Plugin 的零写入约束。
- Codex logout 对符号链接的删除语义不对称：它会删除 project link 而不是 Base 文件。AD 在下一次 prepare/launch 只重建预期链接，不根据内容判断登录状态。
- `auto` / `keyring` 凭据按 canonical `CODEX_HOME` 生成独立 key，不能通过 `auth.json` link 复用。Project runtime 不得因此回退为“每项目登录”；遇到 Keychain-only Base 时必须在 Preview 阶段阻塞，并引导用户选择共享 Home/Profile 路线或把用户登录存储切换到 file。
- session/history/log/SQLite 仍留在 project home，由 Codex 自己管理；不跨项目共享，也不进入 AD 的转换、diff、backup、receipt 或日志。
- Profile 若提供项目 API Key，由 Profile 自己管理敏感值和 Launch 注入；本 ExecPlan 只定义 `profile_id` 集成边界，不实现或复制 API Key。

## Plugin 转换分类器

```rust
struct ClaudePluginDescriptor {
    plugin_id: String,
    enabled: bool,
    declaration_path: PathBuf,
    marketplace: MarketplaceDescriptor,
    source_root: PathBuf,
    version: Option<String>,
    components: PluginComponents,
    source_digest: ContentDigest,
}

struct PluginComponents {
    skills: Vec<PathBuf>,
    commands: Vec<PathBuf>,
    hooks: Vec<PathBuf>,
    mcp_servers: Option<serde_json::Value>,
    apps: Vec<PathBuf>,
    agents: Vec<PathBuf>,
    lsp_servers: Option<serde_json::Value>,
    unknown: Vec<String>,
}
```

| 条件 | Project 目标 | User 目标 | 状态 |
|---|---|---|---|
| `enabled = false` | 无 | 无 | `unchanged / source_disabled` |
| Codex-compatible package | Project Codex Home | Codex 官方 marketplace flow | Project 自动安装，冲突时阻塞 |
| Skills / Commands | 转换后的 Project package | User 自动安装未实现 | 自动或 partial |
| MCP | Project package | User 自动安装未实现 | 自动或 partial |
| Hooks / Apps / Agents | 可移植内容仍安装，组件本身保留 residual | User 自动安装未实现 | partial 或 unsupported |
| 仅 LSP / output style / 未知组件 | 无原生载体 | 无原生载体 | `unsupported_component` |

完整 package 路线优先，因为它保留 Plugin id、版本、组件关系和未来升级语义。组件拆解只用于 legacy Claude Plugin 无法成为 Codex package 时。

## Marketplace 解析

解析顺序对齐 Codex 官方外部 Agent importer：

1. 当前作用域 settings 的 `extraKnownMarketplaces`；Project local 覆盖 Project shared。
2. `~/.claude/plugins/known_marketplaces.json` 中同名且 source 一致的物化路径。
3. `claude-plugins-official` 缺失时推断为 `anthropics/claude-plugins-official`。
4. project runtime 合成 base Codex marketplaces；同名同源继承，同名异源 conflict。

支持已存在本地目录、GitHub `owner/repo`、HTTPS/SSH Git URL 与可选 ref。`file`、`npm`、动态 source 或无法证明来源的条目保持 unsupported。Git 参数必须结构化传给 `Command`，不能拼 shell 字符串。

## Plugin 自动安装端到端流程

自动安装不是把一条 `codex plugin install` 命令藏到 AD 后面，而是由 AD 生成与 Codex 官方安装器相同的三个持久化结果：**marketplace materialization、versioned Plugin cache、`config.toml` enabled state**。整个流程分为 Prepare/Preview、Apply、Launch 三个边界，只有 Apply 会修改有效 runtime；Launch 只是启动 Codex。

### 阶段边界

| 阶段 | 用户动作 | AD 行为 | 是否修改有效 Codex Home |
|---|---|---|---|
| Prepare / Preview | 在转换页选择 Project 与 Plugin | 盘点 source、解析 marketplace、必要时下载到 AD-owned 临时 stage、转换 package、计算 digest、生成 MutationPlan | 否 |
| Apply | 用户确认预览 | 加锁、复核 digest、备份、按依赖顺序写 project home、失败补偿、生成 receipt | 是，仅 project home；共享 auth link 除外不写 Base home |
| Launch | 点击“使用项目运行时启动 Codex” | 复核 runtime freshness，设置 `CODEX_HOME`、cwd 与可选 Profile 后启动 Codex | 不执行安装；过期则回到 Preview |

### Prepare / Preview：从 Claude 声明得到可安装包

1. **读取作用域。** 按 Claude Project local > Project shared 的 precedence 读取 `enabledPlugins`、`extraKnownMarketplaces`，并只读补充 `~/.claude/plugins/known_marketplaces.json` 与已安装 source。每个输入都记录 path、kind 与 digest。
2. **先处理开关。** `enabled = false` 立即产出 `unchanged/source_disabled`，不解析网络、不生成 target。启用项继续解析 `<plugin>@<marketplace>`。
3. **解析 marketplace。** 优先使用当前 Project 声明的 source，其次使用 known marketplace，官方 Claude marketplace 才允许固定 fallback。Git source 在有效 Codex Home 之外 stage；local source 形成只读 snapshot，不能让最终 runtime 可写回 Claude cache。
4. **校验 catalog 与 source。** 必须找到 `.agents/plugins/marketplace.json` 或兼容的 `.claude-plugin/marketplace.json`，定位唯一 Plugin entry，拒绝 name/version/path traversal、越界 symlink、同名异源和 manifest/name 不一致。
5. **分类并转换 package。** 如果 source 已含合法 `.codex-plugin/plugin.json`，保留完整 package；否则 AD 在 stage 中生成最小 `.codex-plugin/plugin.json`，保留 Skills/MCP 并把 Commands 转成 Skills。LSP、Hooks、Apps、Agents 与未知组件进入 `partial` 或 `unsupported_component`，绝不丢弃后报告完全成功。
6. **冻结 stage。** 版本取 Codex manifest version；缺失时使用官方 local 语义 `local`。对 marketplace snapshot、转换后 package tree、Base config、继承 package 与目标状态计算稳定 digest，MutationPlan 引用这些 digest，不引用可变 source。

远程下载失败、ref 不存在、manifest 非法或 requirements/policy 不允许时，Preview 给出逐 Plugin resolution，不产生半成品 write-set。Preview 可以重试；用户无需运行任何 Codex 命令。

### Preview 展示的安装清单

每个 Plugin 必须展开显示以下内容，不能只显示“可自动转换”：

```text
Source       jdtls-lsp@claude-plugins-official
Source state enabled / disabled
Route        package-copy | package-transform | component-fallback | unsupported
Marketplace  source + ref + resolved revision + digest
Package      name + version + component summary + tree digest
Targets      marketplace snapshot / cache path / config key
Residual     未迁移组件及具体原因
Conflicts    target drift / same-name-different-source / policy denial
```

最终 Preview 同时显示全局 mutation 顺序、网络来源、Base home 只读项、auth 共享引用、备份范围和预计 rollback 行为。

### Apply：确定的写入集合与激活顺序

以 `typescript-lsp@claude-plugins-official`、版本 `1.2.3` 为例，Project package route 产生：

```text
~/.ad/codex-homes/<project-id>/
├── auth.json -> <base-home>/auth.json
├── .tmp/marketplaces/claude-plugins-official/...
├── plugins/cache/claude-plugins-official/typescript-lsp/1.2.3/...
└── config.toml

~/.ad/state/codex-project-runtimes/<project-id>.json
~/.ad/backups/operations/<receipt-id>/...
~/.ad/history/operations/<receipt-id>.json
```

对应 config 的有效结果至少包含 marketplace 描述与 Plugin 开关；AD 通过 TOML AST 合成，不通过字符串拼接：

```toml
[marketplaces.claude-plugins-official]
# source_type/source/ref/last_revision 由 resolved marketplace 决定

[plugins."typescript-lsp@claude-plugins-official"]
enabled = true
```

Apply 的顺序固定为：

1. 获取 `<project-id>` execution lock，重新核验 source、stage、Base 和所有 target digest；任一变化使 plan 失效。
2. 为将被替换的 project config、marketplace 与 cache 建立 operation backup；auth 只备份 link 元数据，不备份目标内容。
3. 创建/修复 runtime scaffold 与受控 auth link。
4. 用同父目录 staging + rename 写 marketplace snapshot。
5. 用同样的目录事务写 `plugins/cache/<marketplace>/<plugin>/<version>`；目标同 digest 为 unchanged，同版本不同 digest 为 conflict。
6. 写入唯一的版本化 runtime manifest，记录 applied 策略、Profile 与 Project overlay。
7. **最后写 `config.toml` 作为激活点。** 在 package/cache 与 manifest 全部就绪前，绝不写 `enabled = true`，避免 Codex 观察到悬空 Plugin。
8. ExecutionEngine 重新观察所有 target 的类型与 digest，Apply 成功后才注册/刷新 runtime cache，并持久化 receipt。

任一步失败都按相反顺序补偿。若 config 尚未写入，失败内容不会被 Codex 激活；若 config 已写入但后置校验失败，先恢复 config，再恢复 cache/marketplace。补偿失败返回 `partial_failure` 和精确残留路径，不能报告成功。

### 与 Codex 官方安装动作的等价关系

| Codex 官方安装效果 | AD 的自动化实现 | 验收证据 |
|---|---|---|
| `marketplace/add` 物化 source 并记录 `[marketplaces.<name>]` | AD stage Git/local snapshot，校验 catalog，再以 Directory mutation + config synthesis 写 project home | marketplace root、source/ref/revision 与 digest 可复核 |
| `plugin/install` 解析 manifest 并复制版本包 | AD 校验/转换 `.codex-plugin/plugin.json`，写 `plugins/cache/<marketplace>/<plugin>/<version>` | manifest name/version 与 cache path 一致，tree digest 一致 |
| `plugin/install` 设置 enabled | AD 在所有目录 mutation 成功后合成 `[plugins."<plugin>@<marketplace>"] enabled = true` | 新 Codex 进程能发现；失败前不出现悬空 enabled |
| uninstall / upgrade 的目录与 config 变化 | 后续 Preview 仍生成 MutationPlan：升级写新版本并切换有效状态；卸载删除该 project cache/config，均可 backup/rollback | 重复 Apply unchanged，source/version 变化显示明确 diff |

AD 不调用 Codex CLI 或 app-server。对齐目标是上述磁盘契约和可观察结果；Codex 内部遥测、远程 ChatGPT Plugin 安装和 connector 授权不由 AD 仿造。

### Launch 与刷新闭环

Apply 完成后不会自动启动 Codex。用户点击 Launch 时，AD 比较 Project desired 策略、manifest applied 策略、manifest/config digest；继承开启时还比较 Base config digest。任何不一致都显示“需要刷新”，重新走 Preview → Apply；继承关闭时普通 Base config 变化不会使 runtime 过期。Plugin source/package 的变化在下一次 Preview 的 source/tree digest 中检测，不在启动路径偷偷写盘。

## 应用路线

### 路线 A：Project package 安装到隔离 runtime（首选）

1. 创建或解析 `ProjectCodexRuntime`。
2. 读取默认 Codex config 与所需 inherited packages，只加入 read-set。
3. 解析 Claude Project Plugin、marketplace 与 manifest。
4. 合成 project home config，镜像 inherited packages，安装 Project packages。
5. 原子应用到 `~/.ad/codex-homes/<project-id>`，并建立指向 Base 用户登录态的受控认证引用。
6. AD LaunchRecipe 注入 `CODEX_HOME=<project-home>`、project cwd 与可选 `profile_id`；未选择 Profile 时使用共享 ChatGPT 登录态。

### 路线 B：Project partial package（兼容 fallback）

```text
Claude legacy plugin
  ├─ skills/*          → Project package skills/*
  ├─ commands/*        → Project package skills/<command>/SKILL.md
  ├─ MCP               → 保留在 Project package
  ├─ hooks/apps/agents → partial residual
  └─ lspServers        → unsupported residual
```

### 路线 C：User package 安装

本轮不自动执行。User scope 继续使用 Codex 官方 marketplace flow。Project scope 不以“确认影响所有项目”为兜底；只有未来显式提供“改为 User 安装”动作时才允许 scope escalation。

## UI 行为

- Project scope 增加 Runtime card：可访问的继承开关、desired/applied 策略、project Codex home、认证复用方式、可选 Profile、检查/待应用/漂移/失败状态与 Preview 恢复动作。
- Preview 明确显示 `~/.codex/config.toml` 为只读 base，所有 Plugin/cache/config mutations 位于 `~/.ad/codex-homes/<project-id>`。
- Base config 改变时显示 `需要刷新项目运行时`；外部修改生成文件时显示 conflict。
- Apply 成功后提供“使用项目运行时启动 Codex”；普通 Codex 启动不受影响。
- Disabled 为无需变更；package copy、package transform、partial、LSP-only 分别显示。
- 不显示或记录认证文件路径以外的任何凭据内容；Keychain-only 无法复用时在 Apply 前明确阻塞，不把重复登录作为 fallback。

## 不变量

1. 转换过程不启动 Codex CLI/app-server；启动 Codex 只发生在用户点击 Launch 后。
2. Claude source 与默认 `~/.codex` 的配置、marketplace、Plugin cache 全部只读；共享 auth 的 Codex refresh 是唯一明确例外。
3. Project Plugin 的 config、marketplace、cache 与 enable state 只能写 project runtime home。
4. Launch 必须绑定 project id、canonical cwd 与对应 `CODEX_HOME`，不能静默回退默认 home。
5. AD 只能管理认证引用的路径元数据；auth/token 内容及 sessions、history、logs、SQLite 永不进入 snapshot、diff、backup、receipt 或日志。
6. Disabled Plugin 不产生 mutation；LSP-only 不生成空 package。
7. marketplace/name/version/path 全部阻止 traversal 与 symlink escape。
8. 目录和 config 要么全部成功，要么补偿回原状态。

## 替代方案

### 直接写默认 `~/.codex`

不作为 Project 默认路线。它会让 Project Plugin 对其他项目可见，违背项目隔离目标。

### 默认 home + Codex profile

Codex profile 可以原生继承 base config，并只在 `--profile` 启动时覆盖 Plugin enable state；认证与 package cache 也能复用。它仍需要在默认 home 存放 profile/package，无法提供严格的 Plugin 存储隔离，因此不是文件型认证下的首选路线；但它是 Keychain-only Base 无法跨 `CODEX_HOME` 共享登录态时的兼容路线。AD 的产品级 Profile 还负责项目 API Key 等差异化凭据，不能与 ChatGPT 用户登录态混为一谈。

### Custom home 直接 symlink 默认 config/cache

否决。Plugin refresh/upgrade 会穿透链接修改默认 home。`auth.json` 是唯一允许的链接，因为本方案明确要求共享同一用户登录态；它不意味着 config、marketplace 或 cache 也可共享写入。

### 自动安装社区 LSP/MCP bridge

本轮否决。它是新供应链和新运行时，不是 Claude Plugin 到 Codex Plugin 的等价转换。后续应由独立 ExecPlan 评审。

## 验收

1. `jdtls-lsp=false` 显示 source_disabled，不产生 mutation。
2. Project Codex-compatible Plugin 安装到 `~/.ad/codex-homes/<project-id>`，`~/.codex` 的 config/marketplace/Plugin cache 前后 digest 不变；auth refresh 单独排除。
3. 从 AD 启动该项目时 LaunchRecipe 带精确 `CODEX_HOME`；默认启动不加载该 Plugin。
4. Base 用户配置变更后，下一次 prepare/launch 生成可预览 refresh；用户无需手工复制。
5. Base 中已启用 Plugin 在 project runtime 中有对应独立 package；不产生悬空 enabled state。
6. 生成 config 外部变化、base 并发变化、同名异源 marketplace、路径穿越与非法 symlink 均阻塞 Apply。
7. 文件型用户登录无需按项目重复登录；AD 只创建和校验 auth link，不读取或复制内容。Keychain-only Base 在 Preview 阶段阻塞并给出共享 Home/Profile 或 file store 选项。
8. enabled LSP-only Plugin 显示 `unsupported_component:lsp`，不生成空 cache/config。
9. package/config/directory apply、故障补偿与 rollback 有集成测试。

## 依据

- Codex Environment variables：`CODEX_HOME` 覆盖 config、auth、logs、sessions、skills 与 package metadata：<https://developers.openai.com/codex/config-reference#environment-variables>
- Codex Config basics：User / Profile / Project precedence 与 project trust：<https://developers.openai.com/codex/config-basic>
- Codex Advanced config：profiles 与 config/state locations：<https://developers.openai.com/codex/config-advanced>
- Codex Build plugins：repo marketplace、安装 cache 与用户 config 状态：<https://developers.openai.com/codex/build-plugins>
- Codex app-server：Plugin install 仍标为 under development：<https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md>
- Codex 官方 importer：<https://github.com/openai/codex/tree/main/codex-rs/external-agent-migration/src>
- Codex marketplace add：source stage、`.tmp/marketplaces` materialization 与 config 更新：<https://github.com/openai/codex/blob/main/codex-rs/core-plugins/src/marketplace_add.rs>
- Codex Plugin install manager：resolve → materialize → store → enabled：<https://github.com/openai/codex/blob/main/codex-rs/core-plugins/src/manager.rs>
- Codex Plugin store：`plugins/cache/<marketplace>/<plugin>/<version>` 与原子替换：<https://github.com/openai/codex/blob/main/codex-rs/core-plugins/src/store.rs>
- Codex credential storage：文件存储跟随 `auth.json` 路径，Keyring key 按 canonical `CODEX_HOME` 派生：<https://github.com/openai/codex/blob/main/codex-rs/login/src/auth/storage.rs>
- 项目级 Plugin 状态缺口：<https://github.com/openai/codex/issues/18115>
- Eclipse JDT.LS：<https://github.com/eclipse-jdtls/eclipse.jdt.ls>

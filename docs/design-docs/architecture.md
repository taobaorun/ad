# AD 系统架构（v2.1）

> 状态：已实现（2026-07-20）
>
> 详细多 Agent 决策见 [multi-agent-architecture.md](multi-agent-architecture.md)。

## 架构总览

AD 是 macOS-only 的 Tauri 2 桌面应用。React 前端负责选择 `AgentContext`、展示 adapter 声明的真实能力和变更预览；Rust 后端负责 Agent 发现、格式解析、变更规划和唯一安全写盘入口。

```text
React UI / Zustand
  └─ AgentContext + capability descriptors + plan views
       └─ Tauri Agent commands
            ├─ Built-in Adapter Registry
            │    ├─ Claude Code adapter
            │    └─ Codex adapter
            ├─ Profile Repository
            ├─ Conversion Coordinator
            └─ ExecutionEngine
                 ├─ allowlist target resolution
                 ├─ digest preconditions
                 ├─ backup manifest + atomic write
                 └─ receipt/history + guarded rollback
```

未来 Agent 只通过编译进应用的 adapter 接入，不支持用户通过配置或脚本定义 adapter。通用 command、store 和组件不解释 Claude JSON 或 Codex TOML 字段。

## 身份与能力

- `AgentDefinition`：内置 Agent 产品类型，例如 `claude-code`、`codex`。
- `AgentInstallation`：由 adapter 按有效配置 home 计算 canonical identity 的本机实例；同一实例被多个来源发现时只保留去重后的 canonical installation，不持久化 aliases。
- `AgentContext`：`installationId + 可选 projectPath`，是每次读取、预览、写入、history 和 rollback 的隔离边界。
- `AgentProfile`：以 `(agentId, profileId)` 为复合身份，common envelope 只保存 metadata、`payloadSchema` 和 adapter-owned payload。

Capability 由 `SettingsPort`、`SkillsPort`、`PluginsPort`、`ProcessPort` 和 `LaunchPort` 等真实可调用端口推导。descriptor 同时报告 scope、operations、availability 和结构化 limitations；端口不存在就不能宣称支持。

## 安全变更模型

所有 Agent 写操作遵循同一条路径：

1. adapter 从 allowlist resource 读取内容并生成带 digest 的 snapshot；
2. adapter 校验编辑内容或 Profile payload，生成 backend-owned `MutationPlan`；
3. UI 仅展示 plan view，apply 只提交 `planId`；
4. `ExecutionEngine` 重验 read/write set，先完成全部备份和 manifest，再使用 APFS rename 语义逐文件原子写；
5. 引擎持久化 `OperationReceipt` 和 history；多文件失败返回 `complete`、`compensated` 或 `partial_failure`；
6. rollback 重新经过 adapter allowlist，并检查 apply 后 digest，拒绝覆盖用户后续修改。

物理路径和 mutation content 不由前端提交。adapter 只负责 inspect 和 plan，`ExecutionEngine` 是 Agent v1 用户文件的唯一写入入口。

## Agent 适配边界

| 能力 | Claude Code | Codex |
|---|---|---|
| Settings | user/project JSON | user/project TOML，保留未知字段 |
| Profiles | `ad.profile/claude-code.v2` | `ad.profile/codex.v1` |
| Skills | Claude locations 与 symlink 语义 | `$HOME/.agents/skills`、项目 `.agents/skills` 与 TOML enable 状态 |
| Plugins | 列表和项目 override；无真实 install operation | 列表和 TOML toggle；Project install 进入 AD Managed Project Codex Home，User install 仍标记 degraded |
| Process | adapter matcher + 共享 macOS scanner | adapter matcher + 共享 macOS scanner |
| Terminal | adapter-owned launch recipe | adapter-owned launch recipe |

“能力对等”是用户任务和安全保证对等，不是文件字段一一对应，也不允许用不完整行为伪造成功。

## Claude Code → Codex 转换

转换是内置 artifact route：Claude source snapshots 始终只读；每个 artifact 标记 `exact`、`mapped`、`partial`、`requires_input`、`unsupported`、`conflict` 或 `unchanged`。每次转换只处理 User 或当前 canonical Project 一个作用域。Project Skills 写入原生项目载体；Project Settings/Plugin 则写入自动派生的 `~/.ad/codex-homes/<project-name>`，不会改写 `<project>/.codex/config.toml`。转换复制/转换 Project package，并在启用继承时镜像 Base 已启用 package，最后合成唯一的 runtime config。已确认等价的 `maxContextTokens` 自动映射为 `model_context_window`；Claude model 不猜测成 Codex model，模型和权限覆盖只能通过内置决策显式提供。只有安全可写目标进入 confirmation-required plan，目标写入复用共享执行引擎。转换不会删除或改写 Claude 配置，Codex 目标可通过 receipt 回滚。

Project Codex runtime 是 AD 提供的派生 installation，不是 Codex 官方 Project Plugin。每个 canonical Project 只拥有一个以项目目录名命名的 `~/.ad/codex-homes/<project-name>`，不会因 Base Codex installation 不同而创建多个 runtime；同名（含 macOS 不区分大小写碰撞）的不同项目会 fail closed，避免共享配置。切换 Base 时只复用这个 home，并要求新的 Preview → Apply 成功后才更新 applied Base。旧版单一哈希目录首次发现时迁移到项目名目录，并保留旧路径 symlink 兼容历史 receipt；同一项目存在多个旧 Base runtime 时不会猜测合并，而是保留旧 runtime 可发现、等待显式处理。runtime 只在 AD scoped launch 中通过 `CODEX_HOME=<project-home>` 生效。每个 Project 保存 desired `inheritBaseConfig`，runtime manifest 保存 applied 策略、Profile 与明确的 Project Plugin overlay；只有 Preview → Apply 才改变 applied 状态。开启时 Base `config.toml` 只读结构化合成并镜像启用 Plugin，关闭时从空 TOML 合成且不引用 Base 配置或新增 Base 镜像。两种模式都独立复用文件型 ChatGPT 登录的 `auth.json` symlink，AD 不读取凭据内容；Keychain-only Base 会阻塞隔离 runtime。Codex 原生 `<project>/.codex/config.toml` 保持只读并继续拥有更高优先级。

## 双窗口与前端状态

AD 运行两个独立 Tauri WebView：

- 主窗口：Agent/installation selector、项目工作台、Settings、Skills、Plugins、Profiles、转换、history 和终端入口。
- Settings 窗口：通用偏好、终端 backend、Skill sources、快捷键和 legacy 设置。

跨窗口偏好通过 `localStorage` 和 `storage` 事件同步。Agent 选择使用 `ad.agent-context.v2` 持久化，并在 installation 失效时回退到可发现实例。所有 Tauri Agent 返回值在前端经过严格 Zod schema 校验。

## 数据安全边界

adapter 必须显式 allowlist 可管理资源。auth、token、credentials、session、chat/prompt history、logs、telemetry、非 Plugin cache、SQLite 和索引永不进入 snapshot、diff、Profile、backup 或应用日志。Project runtime 只允许管理 `auth.json` 的 symlink 路径元数据，绝不读取、复制或备份认证内容。错误和 history 不记录配置正文。

## 运行时数据位置

| 数据 | 路径 |
|---|---|
| canonical Agent Profiles | `~/.ad/profiles/<agentId>/<profileId>.json` |
| legacy Claude Profiles（兼容读取） | `~/.ad/profiles/<profileId>.json` |
| operation backups / manifest | `~/.ad/backups/operations/<receiptId>/` |
| operation history / receipt | `~/.ad/history/operations/<receiptId>.json` |
| legacy backups / history | `~/.ad/backups/`、`~/.ad/history/` |
| 项目注册 / 扫描根 | `~/.ad/state/projects.json`、`~/.ad/state/scan_roots.json` |
| Skill library / sources | `~/.ad/skill-library/`、`~/.ad/state/skill-sources.json` |
| Project Codex homes | `~/.ad/codex-homes/<project-name>/` |
| Project Codex applied manifest | `~/.ad/codex-homes/<project-name>/.ad/runtime-manifest.json` |
| Project Codex runtime registry | `~/.ad/state/codex-project-runtimes/<project-name>.json` |
| Codex Plugin conversion stages | `~/.ad/staging/codex-plugin-conversion/<digest>/` |
| 测试 home 覆盖 | `AD_HOME=<path>` |

Agent 自有配置仍位于 adapter 解析出的 home（默认 `~/.claude/`、`~/.codex/` 或派生 `CODEX_HOME`）及项目目录。除上述受控登录引用外，AD 不管理认证和运行时状态。

## 代码结构

```text
src/
├─ components/               # 通用 Agent UI、编辑器、plan/rollback 对话框
├─ store/agents.ts           # canonical installations、AgentContext、capabilities
├─ lib/agentTypes.ts         # 严格 IPC schema
├─ lib/profileSchema.ts      # AgentProfile envelope schema
├─ lib/profileEditorRegistry.ts
├─ lib/tauri.ts              # invoke 边界
└─ i18n/locales/{zh,en}.json

src-tauri/src/
├─ agents/
│  ├─ adapter.rs / registry.rs / capabilities.rs
│  ├─ operations.rs / execution.rs
│  ├─ profiles.rs / conversion.rs
│  ├─ claude*.rs
│  ├─ codex*.rs / plugin_conversion.rs
│  └─ project_codex_runtime.rs / project_codex_config.rs / project_codex_manifest.rs
├─ commands/agents.rs        # inspect/plan/apply/rollback/conversion IPC
├─ commands/profile_envelopes.rs
├─ fs/atomic.rs / paths.rs
└─ terminal/                 # Ghostty、cmux、Terminal.app、Custom backend
```

旧 Claude façade 暂时保留给 legacy template/import/shortcut 流程；新多 Agent UI 和写操作使用 v1 contract。兼容层不得被 Codex 复用或扩展。

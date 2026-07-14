# Multi-Agent 支持 ExecPlan

本 ExecPlan 是一个活文档。当前处于 Stage 2 Plan，尚未获用户批准；批准前不得执行实现。

## 目的 / 全局视角

将 AD 从 Claude Code 专用配置管理器演进为 Agent-neutral 的多 Agent 管理器。第一阶段支持 Claude Code 与 Codex 的能力对等：配置、Skills、Plugins、进程探测、终端启动、profile、备份、历史和恢复。

跨 Agent 转换必须是非破坏性的：Claude Code → Codex 只生成 Codex 目标配置，源配置保持不动。目标已存在时，先预览差异和无法转换字段，用户确认后备份目标并写入，失败可回滚。

## 确认状态

- [x] **用户已确认** — 评审 HTML 路径：docs/exec-plans/active/multi-agent-support.html
- [x] 用户已确认，开始执行（2026-07-14 Asia/Shanghai）

## 假设

- macOS only；继续使用 APFS rename 和现有 AD_HOME 测试隔离。
- Agent 只通过 AD 内置 Rust adapter 接入，不支持用户通过配置声明 adapter。
- Agent discovery 可能从多个来源返回同一个实际安装；统一层只保留 canonical installation，不保留 aliases。
- profile 的持久化身份为 agentId + profileId；旧 Claude profile 文件需要兼容读取。
- Codex 的具体路径、格式和可管理能力以官方资料和本机实际安装发现结果为准，不在通用层预设 Claude 字段。
- 目标配置存在时，转换是显式操作，不自动覆盖。

## 影响范围

预计会触及以下区域，实际以任务执行中的最小 diff 为准：

- src-tauri/src/agents/：adapter trait、registry、capability、discovery、converter
- src-tauri/src/commands/：Agent discovery/state、apply/convert IPC 命令
- src-tauri/src/models.rs：agent-neutral persisted models
- src-tauri/src/fs/：Agent path、backup、atomic write 的复用或扩展
- src/lib/：Agent 类型、schema、IPC 封装、转换预览模型
- src/store/：Agent 选择、项目状态、profile、转换状态
- src/components/：Agent selector、能力驱动 UI、转换预览/确认
- src/i18n/locales/{zh,en}.json：同步 UI 文案
- docs/design-docs/architecture.md：批准后的架构更新
- tests/ 与 src-tauri/tests/：适配器、转换、回滚和行为测试

## 关键决策

1. Adapter 是唯一 Agent-specific 边界。通用层只消费 AgentMetadata、Capability、Installation、ProjectState、Profile 和 OperationResult。
2. Capability 驱动 UI 和命令可用性。Claude 与 Codex 第一阶段必须声明并实现同一组 P0 能力；实现细节可以不同。
3. canonical identity 去重。多个 discovery 来源解析到同一真实安装时，只保留一个 installation。
4. 复合身份。所有 profile、project state、history、last applied 记录都必须包含 agentId。
5. 转换不等于覆盖。转换器产生独立目标文档和不可转换报告；源文档只读，目标覆盖必须经过确认和备份。
6. 适配器不共享 Agent-specific schema。未知字段保留在源 Agent 文档中，不能未经规则映射到另一 Agent。

## 进展

- [x] (2026-07-14) 评审多 Agent 产品规格和本 ExecPlan
- [x] (2026-07-14) 建立 agent-neutral model、registry、capability、内置 Claude/Codex discovery 和 IPC 基础；旧 profile migration 仍在后续 Claude adapter slice 完成
- [ ] (进行中) 将现有 Claude Code 行为包入 Claude adapter，保持回归兼容；当前已完成 Agent-scoped profile identity、Agent-specific profile storage 与 built-in discovery
- [ ] (待开始) 实现 Codex adapter 的 discovery、配置、Skills、Plugins、进程探测和终端启动
- [ ] (待开始) 实现 Claude Code → Codex 转换预览、冲突、备份和回滚
- [ ] (进行中) 接入 Agent-aware store、UI、i18n 和 IPC；当前已完成 Agent store、selector、双语文案、discovery IPC 与按 Agent profile 加载/保存
- [ ] (待开始) 完成单元、集成、行为测试及架构文档更新
- [ ] (待开始) 完成发布前 build 和人工验收

## 意外发现

- 当前 Claude Code 逻辑分布在 models.rs、commands/settings.rs、commands/skills.rs、commands/activate.rs、fs/paths.rs、多个 store 和 UI 组件中，不是单一模块。
- 本机同时存在 ~/.claude 和 ~/.codex 配置根；Codex 目录包含 config.toml、AGENTS.md、hooks.json 等文件。实现前必须逐项确认哪些文件属于可管理配置，避免误读 state、auth 或日志文件。
- 当前产品规格目录要求 HTML 规格；因此本计划同时提供 MD 活文档和 HTML 评审基线。

## 决策日志

- 决策：只保留 canonical installation，不保留 aliases。
  理由：用户明确要求发现结果去重后只保留 canonical installation。
  日期/作者：2026-07-14 / Codex

- 决策：Claude Code → Codex 保持源配置不动。
  理由：转换是生成目标配置的操作，不是跨 Agent 覆盖。
  日期/作者：2026-07-14 / Codex

- 决策：未来 Agent 只支持内置代码 adapter。
  理由：用户明确不支持用户配置式扩展。
  日期/作者：2026-07-14 / Codex

## 上下文和方向

当前 profile 模型以 ClaudeSettings、ProfileLayers 和 Claude-specific activation 语义为中心。实施时先抽出稳定的 Agent-neutral contract，再把 Claude 现有行为迁入 Claude adapter；不要直接把 Codex 分支塞进现有 Claude commands。

当前 Skill 管理通过 ~/.claude/skills 和项目 .claude/skills symlink 工作。Codex 的 Skill/Plugin 语义必须由 Codex adapter 实现，不能假设路径或格式与 Claude 相同。

## 工作计划

### Phase 1 — Contract and compatibility foundation

1. 定义 AgentId、Capability、AgentMetadata、AgentInstallation、AgentProjectState、AgentProfileRef、ConversionPreview、ConversionIssue、Apply/Backup result。
2. 建立 adapter registry 和 capability negotiation。
3. 设计旧 ProfileFile 的迁移：旧 Claude profile 映射到 agentId=claude-code，旧 IPC 行为保持兼容。
4. 增加 canonical identity 的归一化与 discovery 去重测试。

验证：Rust schema round-trip、旧 fixture 读取、registry 可注册 stub adapter、重复 discovery 只返回一个 canonical installation。

### Phase 2 — Claude adapter vertical slice

1. 将 Claude settings/project state/profile apply/backup/history/legacy activation 迁移到 Claude adapter façade。
2. 将 Claude skills/plugins/process detection/terminal launcher 接到 capability contract。
3. 保持现有命令和 UI 的行为兼容，必要时通过兼容 IPC 层过渡。

验证：现有 Rust/TypeScript 测试通过；旧项目应用、skill toggle、plugin toggle、legacy activation 均无回归。

### Phase 3 — Codex adapter vertical slice

1. 在不读取敏感 auth、日志和数据库内容的前提下实现 Codex installation discovery，并返回 canonical installation。
2. 根据本机安装版本和官方资料确认可管理配置文件、用户级/项目级作用域及格式。
3. 实现配置读写、Skills、Plugins、进程探测和终端启动五类能力。
4. 每类能力都使用独立 adapter operation，未支持或发现异常时返回结构化英文错误。

验证：临时 AD_HOME fixtures 覆盖存在/不存在/权限错误/未知字段/外部修改；Codex 五类能力端到端可用。

### Phase 4 — Non-destructive conversion

1. 定义 Claude source snapshot、Codex target document、conversion issue 和 diff preview。
2. 实现 Claude Code → Codex 转换器：源只读，目标独立生成。
3. 目标存在时先返回 preview，不写盘；用户确认后备份目标、原子写入并记录 history。
4. 写入任一步骤失败时恢复目标备份；源配置始终不变。
5. 对无法转换字段分级：lossless、mapped、unsupported、requires-confirmation。

验证：源不变、目标正确生成、无法转换字段完整展示、目标覆盖可回滚、模拟写入失败恢复成功。

### Phase 5 — Agent-aware UI and state

1. 增加 Agent selector 和项目 Agent 状态视图。
2. profile 列表、创建、编辑、应用、历史按复合身份隔离。
3. capability gating 统一控制 Skills、Plugins、进程和终端入口。
4. 增加转换入口、差异预览、无法转换提示、确认/取消/回滚状态。
5. zh/en 文案同步，错误字符串保持英文。

验证：真实临时项目完成 Agent 选择 → 读取 → 编辑 → 转换预览 → 确认 → 应用 → 恢复完整流程。

### Phase 6 — Documentation and release verification

1. 更新 architecture.md、product-specs index 和必要的迁移说明。
2. 运行 typecheck、lint、Vitest、Cargo tests、build。
3. 执行 macOS 手工验收：双窗口同步、终端启动、进程提示、备份和恢复。
4. 归档 ExecPlan MD + HTML 到 completed/。

## 验证和验收

最低命令集：

- pnpm typecheck
- pnpm lint
- pnpm test
- cargo test --manifest-path src-tauri/Cargo.toml
- pnpm build

核心行为验收：

- Claude Code 与 Codex 同时存在时，二者配置、profile、history 不串写。
- 同一 Agent 被多个 discovery 来源发现时只出现一个 canonical installation。
- Claude Code → Codex 不修改任何 Claude 文件。
- Codex 目标不存在时生成目标配置；目标存在时必须先显示 diff 和无法转换字段。
- 用户确认后才备份并写入 Codex；写入失败可恢复目标原状。
- Claude/Codex 的五类能力均可通过同一套 UI 操作，但由各自 adapter 实现。

## 幂等性和恢复

- Discovery、读取和预览是幂等的。
- apply 使用现有 atomic write；每个目标文件写入前生成带 agentId、project、timestamp 的 backup。
- conversion preview 不写盘，可重复执行。
- conversion apply 使用 operation ID 防止重复确认造成二次覆盖。
- 目标写入失败时按 backup manifest 恢复；源文件不参与恢复写入。
- 若 adapter 版本或配置格式变化，拒绝静默转换，返回 requires-confirmation。

## 接口和依赖

- 复用现有 fs::atomic::write_atomic、fs::paths、backup/history 机制。
- 优先使用现有 serde、serde_json、zod、Zustand 和 Tauri invoke，不新增数据库。
- 新增 adapter registry 前先确认是否可用静态 Rust registry，避免动态插件系统。
- 关键接口必须在 Phase 1 完成后存在：AgentAdapter、AdapterRegistry、ConversionPreview、ConversionIssue、CanonicalInstallation。

## 待确认依赖

- Codex 配置使用 TOML；Claude Code → Codex 的可靠转换需要 TOML parser/serializer。项目边界要求新增依赖先确认，当前转换实现暂停在此处，未采用字符串拼接。

## 里程碑规划

- M1 Contract：模型、registry、migration、去重测试。
- M2 Claude parity：Claude adapter façade 完成，旧行为无回归。
- M3 Codex parity：Codex 五类能力通过 fixtures 和真实 macOS 检查。
- M4 Conversion safety：转换预览、目标备份、回滚和审计完成。
- M5 UI integration：Agent-aware UI 和跨窗口状态完成。
- M6 Release gate：全量测试、build、手工验收和文档归档。

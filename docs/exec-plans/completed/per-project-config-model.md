# Per-Project Config Model

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

把 AD 的概念模型从「profile 是项目的当前状态」彻底改成「profile 是只读模板，项目的 .claude/settings*.json 才是真实状态」。

**用户可观察到的变化**：

- Profile 仍然是 `~/.ad/profiles/<id>.json`，但现在它纯粹是「初始化项目用的模板」。改 profile 不再影响已经从它生成的项目。
- ProjectDetail 主区不再是「展示当前 profile + 切换/应用」的看板，而是直接变成项目配置编辑器：三个 Tab（Shared / Local / Env）对应 `<project>/.claude/settings.json` 和 `.claude/settings.local.json`。Save 即时落盘到这两个文件，没有"再点 Apply"这一步。
- 创建 / 切换 template 时，env 层会被合并到 `settings.local.json` 顶层 `env` 字段，CC 启动时自动加载 — 不再只给一段 `export` 片段让用户自己复制。
- 折叠的「项目配置 (settings.local.json)」覆盖编辑器消失：它和新主区编辑器完全重复。
- 右侧 ProfileEditDrawer 重新定位为「Template 编辑器」（编辑模板），和当前项目的状态无关；从命令面板或 Templates 列表打开。
- **Profile 全局生效（v0.1 legacy activation）保留**：在 Advanced settings 里开启 legacy 开关后，template 编辑器仍然带 `[Activate (legacy)]` 按钮，点击后覆盖 `~/.claude/settings.json`，影响所有未走 per-project 流程的 CC 实例。这条路径是给"我只想要一份全局 CC 配置"的简单用法保留的兜底。

## 确认状态

- [x] **待用户确认** — 评审 HTML 路径：`docs/exec-plans/active/per-project-config-model.html`
- [x] 用户已确认，开始执行（2026-05-25T14:55Z）

## 假设

1. 项目当前状态的真理源是 `<project>/.claude/settings.json` 和 `.claude/settings.local.json`，AD 不再额外维护一份"项目侧 profile"文件。
2. `~/.ad/state/projects.json` 里的 `currentProfileId` 字段语义改为「最近一次从哪个 template 初始化或重置」，仅作历史溯源用，不再决定 UI 状态。
3. v0.3 的 `~/.ad/profiles/<id>.json` 保持原 schema（`layers: { shared, local, env }`），不做新一轮迁移。
4. CC 的 settings 支持顶层 `env: { KEY: VALUE }` 字段（来源：用户回答 + Claude Code 官方文档 `env-vars` 章节），写入后启动时自动加载。
5. apply 流程的合并 / 冲突解决逻辑（`fs::merge`）依然用于「切换 template + 想叠加在已有项目配置上」的场景；纯首次初始化场景默认覆盖空白项目。
6. 用户修改项目配置时不需要"应用"动作 — 直接 Save 即落盘；Save 走原子写 + 备份（沿用 `write_atomic` + `~/.ad/backups/`）。
7. `read_project_settings` / `write_project_settings` 现存 IPC 仍是底层能力，但前端不再通过"项目配置"覆盖编辑器调用 — 改由新主区编辑器调用。底层 Rust 函数和测试保留。
8. 折叠的 ProjectSettingsSection 是 v0.3 引入的新组件，删除它不会影响 settings.local.json 之外的数据路径。
9. v0.1 全局激活（`activate_profile` IPC + `~/.claude/settings.json` 覆盖写）是显式保留的兜底路径，仅在 Advanced settings 开启 legacy 后从 template 编辑器入口可见。本次重构不动这条路径的代码、不动 IPC、不动 backups/history 行为。

## 影响范围

```
src-tauri/src/
├── commands/
│   └── apply.rs              [M] env 层不再只生成 export 片段，而是合并进 local 层的 env 字段
├── (其它后端文件不动)         

src/
├── lib/
│   └── (tauri.ts 不增删 IPC，但 ProjectDetail 调用方式变更)
├── components/
│   ├── ProjectDetail.tsx     [M] 大改：删 HeroCard / LayerStack / ProfileCards / ApplyBar / ProjectSettingsSection，换成主区内联三 tab 编辑器；保留 header + Switch template
│   ├── ProfileEditor.tsx     [M] 抽出可复用的「Settings + Env 编辑器」核心，给 ProjectDetail 复用；编辑器本身按 props 决定数据源（template vs project）
│   ├── ProfileEditDrawer.tsx [M] 文案和 i18n key 调整：明确"编辑 Template"
│   └── CommandPalette.tsx    [M] "edit profile" 类命令文案 → "edit template"；新增 "switch template" 命令
├── store/
│   └── (projects.ts / profiles.ts 不变结构，新增一个 selector "用 templateId 初始化 project config")
└── i18n/locales/
    ├── zh.json               [M] 新增 projectEditor.* / template.*；移除 projectSettings.* 与多余 detail.*
    └── en.json               [M] 同步

docs/
├── design-docs/
│   └── (无新增；架构变更在本 plan 内陈述足够)
├── exec-plans/
│   ├── active/
│   │   ├── per-project-config-model.md      [N] 本文件
│   │   └── per-project-config-model.html    [N] 评审版
│   └── tech-debt-tracker.md                 [可能 M] 如发现遗留问题
```

预计 6-8 个源文件 + 2 个 i18n 文件 + 2 个 plan 文件 = ~10 文件（中等偏大复杂度）。

## 关键决策

**D1：Project 真理源 = `.claude/settings*.json`，不再镜像到 `~/.ad/`**

- 理由：CC 自己读这两个文件，多一份镜像必然漂移；APFS 原子写已能保证一致性；备份机制已存在 (`~/.ad/backups/`)。
- 替代方案：在 `~/.ad/projects/<id>.json` 维护 project-specific profile。否决理由：双写路径必然漂移、用户在 IDE 里改完 .claude/ 文件后还得手动同步。

**D2：env 进 settings.local.json 顶层 env 字段，不进 settings.json**

- 理由：env 通常含 API key 等敏感值；settings.json 走 git 提交（团队共享），不适合存敏感值。settings.local.json 是 gitignore 的，正合适。
- 配套：apply.rs 中原本独立的 `"env"` 分支撤掉，并入 local 写入路径，深合并到现有 `env` 子对象。
- 同时移除「应用后展示 export 片段 + Copy 按钮」UI — 文件级集成后这块属于历史包袱。

**D3：Save 即落盘，没有 Apply 按钮**

- 理由：用户已确认（"消失 — 保存即同步到项目"）。Apply / Save 双步是当前模型遗物，per-project 后没有"先暂存到 profile，再下发到项目"这种延迟写入的需要。
- 注意：切换 template 会在主区弹出 "Init from template" / "Merge into project" 二选一，仍然需要冲突解决（沿用 InlineConflictResolver）。

**D4：Profile (template) 编辑入口收敛到独立位置**

- 理由：避免主区被两类编辑混淆（一类是 template，会影响未来项目；一类是 project，只影响当前项目）。
- 实现：ProfileEditDrawer 只用于 template 编辑，触发器是 CommandPalette 里的 "manage templates" / "edit template <name>"；ProjectDetail 主区不再有"编辑 profile"按钮。

**D5：复用 ProfileEditor 的 Monaco 编辑器 + EnvLayerEditor，按 props 切换数据源**

- 理由：避免重复实现 JSON validate / Monaco 包装 / KV 表的代码。
- 实现：抽 `<LayeredSettingsEditor>` 组件，接收 `{ shared: { value, onChange }, local, env }` 三个受控接口；ProfileEditor 和新的 ProjectConfigEditor 都包它。

**D6：HeroCard / LayerStack / ProfileCards / ApplyBar 直接删除**

- 理由：这些都是「展示 profile + 选择切换」的 UI，per-project 后失去意义。LayerStack 的「env › local › shared」概念在 template 内部仍然存在（template 仍然有三层），但不再以"哪一层的哪个 profile 在生效"形式展现 — 项目就是项目，不分层级 source。
- 风险：用户可能在视觉上不再清楚「shared 是 git-tracked / local 是 gitignored」。要在编辑器 tab 头加文字提示。

**D7：不做存量数据迁移**

- 理由：现有 project 的 .claude/settings*.json 已经是「项目的真实状态」，新模型直接读这两份文件，无需迁移。`projects.json` 的 `currentProfileId` / `lastApplied` 保持原样作为历史记录。
- 配套：ProjectDetail header 显示 "Initialized from <template>" 这种 breadcrumb，但用户即使没有这条也能正常用主区编辑器。

**D8：Profile 全局生效（v0.1 legacy activation）保留**

- 理由：用户明确要求保留。这条路径覆盖 `~/.claude/settings.json`，对应"我没有 per-project 需求，只想全局换一份配置"的简单用法。
- 实现：`activate_profile` Tauri command、对应 Rust 函数、UiSettings 里的 `showLegacyActivation` 开关、ProfileEditor 里的 `[Activate (legacy)]` 按钮 — 全部保留，文案和 i18n 也不动。
- 影响：D4 中"template 编辑器收敛到 drawer"的方案下，这个按钮跟着 drawer 走（即"Edit template"抽屉里仍然能 Activate）。Advanced settings 里 `showLegacyActivation` 关掉则按钮隐藏（与今天行为一致）。
- 边界：本次重构不增加新的"全局 + per-project"双重生效逻辑；用户开启 legacy 是单独的兜底场景，不和主区编辑器互相影响。

## 进展

- [x] (2026-05-25T15:02Z) Step 1：抽 `<LayeredSettingsEditor>` 公共组件 — 从 ProfileEditor 抽出 Settings tab 的 Monaco 编辑器 + Env tab 的 KV 编辑器，做成纯受控组件。验证：pnpm typecheck 通过；ProfileEditor 改为复用新组件，UI 行为不变（Settings + Env 两 tab + Copy export footer）。
- [x] (2026-05-25T15:18Z) Step 2：后端 apply.rs 把 env 合并进 settings.local.json — `"env"` 分支用 `write_layer` 写 `{"env": {...}}` payload 到 settings.local.json，复用 deep-merge + 冲突解决 + 备份。旧 `apply_env_layer_returns_export_snippet` 改成 `apply_env_layer_merges_into_settings_local`；新增 `apply_env_layer_preserves_existing_settings_local_keys` 覆盖与已有 env 子对象的合并。验证：102 个 lib 测试通过，clippy 全绿。
- [x] (2026-05-25T15:24Z) Step 3：前后端类型同步 — `ApplyResult` 移除 `envExportSnippet` / `env_export_snippet` 字段；ProjectDetail 里依赖该字段的 export 片段卡片块一并移除（原计划在 Step 5，类型变化让它自然提前）；不再 import `Copy` icon。验证：pnpm typecheck 通过，cargo test 全绿。
- [x] (2026-05-25T15:32Z) Step 4：新增 `<ProjectConfigEditor>` 组件 — 包 LayeredSettingsEditor 三 tab：Shared 读写 settings.json；Local 读写 settings.local.json 减去 env 字段后的内容；Env 读写 settings.local.json 顶层 env。Save 同时持久化所有脏 tab，settings.local.json 重组为 `{ ...localObj, env: envEntries }`。i18n 暂加 `projectEditor.*` zh+en 占位 keys（Step 8 整体清理）。验证：pnpm typecheck 通过。
- [x] (2026-05-25T15:42Z) Step 5：ProjectDetail.tsx 重写 1135 行 → ~250 行，删 HeroCard / LayerStack / PrecedenceMark / ProfileCards / ApplyBar / RecentList / SETTINGS_EDITOR_OPTIONS / ProjectSettingsSection / Section / dsPrimaryBtn / LAYER_ORDER 常量；保留 header + StatusPill + KbdChip + dsBtn；新增 TemplateBreadcrumb（显示初始化模板 + 「切换模板」按钮，点击 stub）+ inline ProjectConfigEditor。新增 i18n keys `detail.initializedFrom / noTemplateYet / switchTemplate / switchTemplateTodo`（zh + en）。验证：pnpm typecheck 通过；本次改动引入的 lint 警告已清零，main 上残留两处见 TD-008。
- [x] (2026-05-25T15:50Z) Step 6：SwitchTemplate 流程 — 新增 `SwitchTemplateDialog.tsx`，列出全部 templates，点选后用 ALL_LAYERS=[shared,local,env] 调 apply_profile_to_project；处理三种 outcome：Applied → onApplied 回调（reload projects + bump editor key 触发重载）+ 关闭；NeedsResolution → 内嵌 InlineConflictResolver；GitDirtyBlocked → 警告 + ack 复选框。i18n 加 `switchTemplate.*` 全套 zh+en keys。验证：pnpm typecheck 通过；本计划自身无新增 lint 警告。
- [x] (2026-05-25T15:58Z) Step 7：ProfileEditDrawer 文案改用 `drawer.editTemplate` (zh "编辑模板" / en "Edit template")；store/ui.ts 增加 `switchTemplateOpen` + `openSwitchTemplate` / `closeSwitchTemplate` actions；ProjectDetail 改用 ui store 控制对话框；CommandPalette 加 "Switch template: {project} …"（APPLY 组）+ 把 EDIT 组从 `editProfile` 文案改为 `editTemplate`。ProfileEditor 里 `[Activate (legacy)]` 按钮 + showLegacyActivation 行为完全不动 — git diff 仅 ProfileEditDrawer 注释 + 一行 i18n key 名变更。验证：pnpm typecheck 通过；本计划自身无新增 lint 警告。
- [x] (2026-05-25T16:05Z) Step 8：i18n — `projectEditor.*` / `switchTemplate.*` / `palette.editTemplate` / `palette.switchTemplate` / `drawer.editTemplate` / `detail.{initializedFrom,noTemplateYet,switchTemplate}` 全部就位（zh + en）；移除：`projectSettings.*`、`drawer.editProfile`、`palette.editProfile`、`detail.sections.*`、`detail.{noProfileApplied,editProfileTitle,edit,row.*,pickLayer,envClipboardNote,dirtyTitle,dirtyDesc,applyAnyway,applying,applyWithResolutions,applyButton,apply,appliedFlash,envExports,copy,noApplies,conflictsResolved,switchTemplateTodo}`。验证：pnpm typecheck 通过；jq 校验两份 locale JSON 合法。
- [x] (2026-05-25T16:12Z) Step 9：AGENTS.md「主要架构」整段更新为 v0.4：项目状态真理源 = `<project>/.claude/settings*.json`，profile 退化为只读 template；主区是内联 ProjectConfigEditor + TemplateBreadcrumb；env 切换 template 时合并到 settings.local.json 顶层 env；legacy 全局激活路径明确保留。GlobalKeymap.tsx 的 docstring 同步：⌘E 释义改为"编辑当前项目初始化所用的 template"。design-docs/index.md 不需要改（设计仍在 exec-plans/completed/，无新独立设计文档）。

每步完成后用 `[x] (YYYY-MM-DD HH:MMZ) ...` 形式打勾 + 时间戳。

## 意外发现

- (2026-05-25T15:42Z) `pnpm lint --max-warnings=0` 在 main 上即不通过：`ProjectSidebar.tsx:252` 缺 React import（no-undef error），`CommandPalette.tsx:199` useMemo 缺 openPalette 依赖。两处都不是本次改动引入的，已记入 tech-debt-tracker.md 的 TD-008，本计划不顺手清理（精准修改原则）。本计划自身引入的 LayeredSettingsEditor 警告已修复（提取 `parseLayer` 到 `src/lib/layeredSettings.ts`，避免 fast-refresh 警告）。

## 决策日志

- **决策**：放弃为 project 创建独立的 `~/.ad/projects/<id>.json` 镜像文件
  **理由**：会和 `.claude/settings*.json` 双写漂移；项目目录已经能被 IDE / git 直接观察。
  **日期/作者**：2026-05-25 / yuanxuan + Claude
- **决策**：env 层进 settings.local.json 顶层 env 字段，删除 export 片段 UI
  **理由**：用户明确选择该方案；CC 原生支持 env 字段；export 片段是手动操作的过渡产物。
  **日期/作者**：2026-05-25 / yuanxuan
- **决策**：抽 `<LayeredSettingsEditor>` 公共组件，避免 ProfileEditor 与 ProjectConfigEditor 重复
  **理由**：两者都需要 Monaco JSON 编辑 + KV env 表 + 校验提示，分两份会立刻产生分叉风险。
  **日期/作者**：2026-05-25 / Claude

## 结果回顾

### 验收对照

- [x] **AC1** — env 在 apply 时合并到 settings.local.json 顶层 env 字段（apply.rs 测试 `apply_env_layer_merges_into_settings_local` + `apply_env_layer_preserves_existing_settings_local_keys` 覆盖）。export 片段 UI 已移除。
- [x] **AC2** — 主区编辑器 Save 直接写 .claude/settings*.json，不经过 profile（`ProjectConfigEditor.handleSave` 仅调用 `tauri.writeProjectSettings`，不触碰 profile store）。
- [x] **AC3** — 编辑 template（drawer）走 ProfileEditor → save_profile，不写项目文件（drawer 关闭后没有任何 .claude 调用路径）。
- [x] **AC4** — ProjectSettingsSection 已删除；UI 不再出现"项目配置 / settings.local.json"折叠区。
- [x] **AC5** — ⌘K palette 出现 "Switch template: <project> …"；点击打开 SwitchTemplateDialog；冲突走 InlineConflictResolver；GitDirty 走 ack 复选框。
- [x] **AC6** — Activate (legacy) 按钮与 showLegacyActivation 行为完全未动（git diff 不涉及 ProfileEditor.tsx 那一段、advanced.* i18n keys、activate_profile IPC）。
- [x] **AC7** — pnpm typecheck / cargo check / cargo clippy --all-targets -D warnings / cargo test (102 lib + 2 bin) 全绿；本计划自身无新增 lint 警告。`pnpm lint` main 上残留 2 处由 TD-008 跟踪。

### 实际效果

- 概念模型简化到位：用户不再需要理解"profile 应用到 project 之后到底是什么关系"——profile 是只读模板、project 的 .claude/ 是真实状态，两条线清晰分开。
- 主区从 5 个区块（Hero/LayerStack/ProfileCards/ApplyBar/Recent + ProjectSettings）压缩成 2 个区块（TemplateBreadcrumb + 内联三 tab 编辑器），ProjectDetail.tsx 从 1135 行 → ~250 行。
- env 终于 file-level 集成，CC 启动后自动加载 ANTHROPIC_* 等变量，不需要额外 export 步骤。
- 抽出 `<LayeredSettingsEditor>` 之后，Profile 编辑器（drawer）和 Project 配置编辑器（主区）共享 Monaco JSON + KV env 表的实现，两边迭代保持同步。

### 与原计划的差异

- **Step 3 早做了 Step 5 的一小段工作**：移除 `envExportSnippet` 字段时连带删除了 ProjectDetail 的 export 片段 UI 块（依赖该字段必须同步）。改动顺序自然合理，未破坏验证标准。
- **i18n key 命名**：原计划提到 `template.*` 命名空间，实际落地为 `switchTemplate.*`（与 `switchTemplate` 操作语义一致），编辑模板的 drawer 文案走 `drawer.editTemplate` 单 key 而不是子命名空间——比预想的更扁平、更易找。

### 遗留问题

- **`base_override` 死代码**：apply.rs 的 `write_layer` 接收 `base_override: Option<&Value>`，永远是 None；当初是给"local overrides"准备的接口，现在不再需要。建议在后续 cleanup 中移除参数。本次未顺手清理（精准修改原则）。
- **TD-008** 已记录到 tech-debt-tracker.md：main 上 `pnpm lint --max-warnings=0` 长期不通过（`ProjectSidebar.tsx:252` 缺 React import + `CommandPalette.tsx:199` useMemo 依赖）。本计划自身没有新增 lint 警告，但要彻底让 lint 进 CI 还需要单独清一轮。
- **`detail.editProfile` / `palette.applyTo` 等 i18n key 仍保留**：`palette.applyTo` 给 APPLY 组的"快速应用单层"命令用（这条命令路径仍然存在），属于继续被引用的 key。`detail.editProfile` 是顶层 `palette` 命名空间里的另一处，本次只移除了 `palette.editProfile`（已确认无引用）。完整清理状态见上面 Step 8 进度。

## 上下文和方向

项目 AD：Tauri 2 + React + TS 桌面应用，管理 Claude Code 的 settings 配置。已落地 v0.3 layered profile + per-project apply，下一步把"项目状态"这一概念从 profile 解耦出来。

关键现有文件：

- `src-tauri/src/commands/apply.rs` — apply 入口，env 分支在 `match layer` 的 `"env" =>` 块（apply.rs:206-218）。
- `src-tauri/src/commands/projects.rs` — `read_project_settings` / `write_project_settings` 已存在（projects.rs:233-254），新主区编辑器复用。
- `src/components/ProjectDetail.tsx` — 当前主体逻辑在 `Detail()` 函数（行 54-410）；底部 `ProjectSettingsSection`（行 934-1046）是要删的部分。
- `src/components/ProfileEditor.tsx` — Settings + Env tab 编辑器，Step 1 从这里抽 `<LayeredSettingsEditor>`。
- `src/components/ProfileEditDrawer.tsx` — 现编辑 profile 的右侧抽屉，要改成 template 编辑。
- `src/lib/profileSchema.ts` — 类型定义和 normalize；本次不动 schema，只调用方变。

不要改的东西：

- profile 文件 schema（`~/.ad/profiles/<id>.json`），保持 v0.3 layered shape。
- 备份 / 历史路径（`~/.ad/backups/`、`~/.ad/history/`）。
- 命令注册名称（保留 `apply_profile_to_project` / `read_project_settings` / `write_project_settings`，避免 IPC 兼容性变更）。

## 工作计划

按 Progress 列表的 9 步顺序推进。每步独立可验证；步骤间松耦合（除了 Step 1 → Step 4 的依赖：必须先抽出 LayeredSettingsEditor 才能在 ProjectConfigEditor 复用）。

执行时遵循 harness 编码准则：

- **最小实现**：不为未来可能的 v0.5 留 hook；不重构相邻无关代码。
- **精准修改**：删 ProjectSettingsSection 时不顺手"美化"周围 layout；改 i18n 时不改其它无关文案。
- **目标驱动**：每步完成后跑 `pnpm typecheck` + 相关 `cargo test` 模块；通不过不算完成。

## 验证和验收

- [ ] **AC1**：从 template A 创建项目 → settings.json + settings.local.json 都被写入；env 出现在 settings.local.json 顶层 `env` 字段；不再出现 export 片段 UI。
- [ ] **AC2**：在主区编辑器修改 settings.json 内容并 Save → 文件即时被写回（mtime 改变）；profile 文件不变（diff 为空）。
- [ ] **AC3**：编辑 template（drawer）→ 已存在的项目 .claude/ 文件 mtime 不变。
- [ ] **AC4**：删 ProjectSettingsSection 后 UI 不再出现"项目配置 / settings.local.json"折叠区。
- [ ] **AC5**：⌘K palette 出现 "switch template" 命令；选完 → 现有项目配置走 merge / 冲突解决流程；不会无声覆盖。
- [ ] **AC6**：Advanced settings 开启 legacy 开关 → template 编辑器（drawer）仍然出现 `[Activate (legacy)]` 按钮 → 点击后 `~/.claude/settings.json` 被 profile 内容覆盖；当前项目的 `.claude/settings*.json` mtime 不变。
- [ ] **AC7**：`pnpm typecheck`、`pnpm lint`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 全绿。

## 幂等性和恢复

- 每步独立 commit；中途中止 → 后续可以从 Progress 表里"未打勾的第一项"继续。
- 如果 Step 2（apply.rs env 集成）出问题，回退方法：保留旧 `"env" =>` 分支的 `env_export_snippet` 输出路径，前端继续用旧 UI。这条回退路径在 Step 2 完成后再走 Step 3 时验证一次。
- ProjectSettingsSection 删除是不可逆操作（代码删了），但 git 历史能回查；Step 5 commit 必须独立，便于 revert。

## 接口和依赖

- 不引入新依赖。
- 公共组件 `<LayeredSettingsEditor>` 接口（Step 1 完成时定型）：

  ```ts
  interface LayeredSettingsEditorProps {
    shared: { text: string; onChange: (s: string) => void };
    local:  { text: string; onChange: (s: string) => void };
    env:    { entries: Record<string, string>; onChange: (next: Record<string, string>) => void };
    onValidityChange?: (allValid: boolean) => void;
  }
  ```

- 后端 `apply_profile_to_project` 调用语义不变；env 处理路径变更体现在 `ApplyResult.env_export_snippet` 改为 `Option<()>` 退化或直接移除该字段（前端类型同步）。

## 里程碑规划（如适用）

不分里程碑 — 9 步串行执行，全部完成后整体进入回归验证。

# Layered Profile Redesign — 让 AD 与 Claude Code 的分层配置对齐

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。本文档遵循 [`docs/PLANS.md`](../../PLANS.md) 的规范。

---

## 目的 / 全局视角

把 AD 从「覆盖全局 `~/.claude/settings.json` 的 switcher」改造成「应用分层 profile recipe 到指定项目的 layered 配置中心」，对齐 Claude Code 自身的 `global / project-shared / project-local / env vars` 分层模型。

变更后用户能做到：

- 在任何项目目录起 `claude`，AD 不影响别的项目，也不污染 `~/.claude/settings.json`
- 同一个 profile（"work" / "personal"）可以应用到多个项目，互不串扰
- 项目内的修改正确落到 `<project>/.claude/settings.json`（团队共享，进 git）和 `<project>/.claude/settings.local.json`（个人，gitignore）两层
- env vars 那一层（`ANTHROPIC_API_KEY` 之类）以"复制 export 片段"形式呈现，不强行写文件
- 旧的"覆盖全局 settings"流程作为高级选项保留，向后兼容

观察验证："打开 AD → 选 profile X → 选 project Y → 点 Apply → `Y/.claude/settings.local.json` 被原子写入；`~/.claude/settings.json` 字节不变；已运行的其他项目里的 CC 实例完全无感"。

---

## 确认状态

- [x] (2026-05-23 1743Z) 用户确认 Round 4 方案 — HTML 冻结
- [x] (2026-05-23 1743Z) 进入 Stage 3 执行

---

## 假设

以下假设已经过用户在 Stage 2 评审中确认：

1. **CC 的 settings 优先级是 global < project-shared < project-local < env vars** — env vars 优先级最高。✅ 已查证文档
2. **`.claude/settings.local.json` 是"个人 + 项目级 + 不进 git"的官方语义** — 「applies to you in this project only and is not checked in」。✅ 已查证文档
3. **写到 `<project>/.claude/settings.json` 是合法行为** ✅ 用户确认。**修订**：如果目标文件已有内容，apply 行为是 **deep merge + 冲突显式确认**，而不是直接覆盖。详见 D8。
4. **env 层不强制写文件** ✅ 用户确认 — 只展示 export 片段供用户复制
5. **旧 profile 的 `settings` 字段安全的迁移目标是新 profile 的 `local` 层** ✅ 用户确认
6. **macOS 的 `tauri-plugin-dialog` 的 folder picker 能让用户选项目目录** ✅ 已在 deps 中
7. **AD 不主动做 git 操作** ✅ 用户确认 — dirty / 缺 .gitignore 条目时给提示，不主动 `git add/commit`

---

## 影响范围

### 后端（Rust）

| 文件 | 改动类型 | 说明 |
|---|---|---|
| `src-tauri/src/models.rs` | 修改 | `ProfileFile` schema 增加 `layers: { shared, local, env }`；保留旧 `settings` 字段做兼容读 |
| `src-tauri/src/migration.rs` | 修改 | 启动时把旧 `settings` 字段迁移到 `layers.local` |
| `src-tauri/src/fs/paths.rs` | 大改 | 全部路径函数指向 `~/.ad/`；新增 `ad_home()` / `projects_state_path()` 等 |
| `src-tauri/src/fs/merge.rs` | 新增 | JSON deep merge + 冲突检测（A3 修订引入） |
| `src-tauri/src/commands/projects.rs` | 新增 | 项目 CRUD：list / add / remove / rename / get_status |
| `src-tauri/src/commands/apply.rs` | 新增 | `apply_profile_to_project(...)` + 冲突解决参数 |
| `src-tauri/src/commands/discover.rs` | 新增 | `scan_roots()` 扫所有 scan roots，返回项目候选；支持两种 kind |
| `src-tauri/src/commands/scan_roots.rs` | 新增 | scan_roots CRUD：list / add / remove / toggle_builtin |
| `src-tauri/src/commands/path_complete.rs` | 新增 | `complete_path_prefix(prefix)` for tab-complete |
| `src-tauri/src/commands/profiles.rs` | 修改 | 反映新 schema；保留 legacy `activate_profile` |
| `src-tauri/src/lib.rs` | 修改 | 注册新命令 |
| `src-tauri/src/commands/mod.rs` | 修改 | re-export 新模块 |
| `src-tauri/tests/layered_apply.rs` | 新增 | 集成测试：apply → 检查目标文件 + 检查全局未变 |

### 前端（TypeScript）

| 文件 | 改动类型 | 说明 |
|---|---|---|
| `src/types/profile.ts` | 修改 | Profile interface + zod schema 反映 layers |
| `src/types/project.ts` | 新增 | Project interface + zod schema |
| `src/store/profiles.ts` | 修改 | 适配 layered shape |
| `src/store/projects.ts` | 新增 | Project state + AD 内部映射 |
| `src/lib/tauri.ts` | 修改 | 新 invoke wrappers |
| `src/components/ProfileEditor.tsx` | 修改 | 改成 Shared / Local / Env 三 tab |
| `src/components/ProjectsPanel.tsx` | 新增 | 项目列表 + Apply UI |
| `src/components/ApplyDialog.tsx` | 新增 | 选 profile + 选层 + diff 预览 + 冲突解决表 + 确认 |
| `src/components/AddProjectInput.tsx` | 新增 | tab-complete paste 输入框 |
| `src/components/DetectedProjectsModal.tsx` | 新增 | auto-detect 列表 + 搜索过滤 + 多选添加 |
| `src/components/ProjectDropZone.tsx` | 新增 | 单文件夹 drag-drop 区域 |
| `src/App.tsx` | 修改 | 双栏布局（Profiles | Projects） |
| `src/components/AdvancedSettings.tsx` | 修改 | 把 legacy 全局覆盖流程藏到 advanced |

### 数据 / 配置

| 路径 | 改动 |
|---|---|
| `~/.ad/profiles/<id>/profile.json` | 新位置 + schema 升级（自动迁移 + `.v1.bak` 备份） |
| `~/.ad/state/projects.json` | 新文件，AD 内部维护的 project↔profile 映射 |
| `~/.ad/backups/`、`~/.ad/history/`、`~/.ad/active` | 从 `~/.claude/ad/` 整体迁移 |
| `~/.claude/profiles/`、`~/.claude/ad/` | M1 启动时 `mv` 走，留 `~/.claude/AD_MOVED_TO_HOME.txt` 标记 |
| `~/.claude/settings.json` | **仍然不主动写**（除非用户走 legacy 流程） |

---

## 关键决策

### D1：分三层而不是更细

CC 文档里实际还有 `ManagedSettings`（macOS `/Library/Application Support/ClaudeCode/`）、CLI flag 等，AD 只支持 **shared / local / env** 三层，因为这是用户能直接管理的、最常用的三层。enterprise managed 路径不进 AD 的范围。

### D2：env 层不写文件

env vars 走 shell，写文件路线选择面（.envrc / .env / shell rc）太多，会让 AD 替用户做太多选择。**只展示 export 片段 + 一键复制**，把"放哪儿"交给用户。

### D3：旧 schema 迁移到 local 层

老的 `settings` 字段往哪迁是个语义选择：
- 选 **local** ✓：最安全的默认（不进 git 不漏密）
- 选 **shared**：可能把 API key 推进 git
- 选 **shared + local 拆分**：得猜哪些字段属于 shared 哪些属于 local，太脆

选 local。用户激活旧 profile 时实际就是把所有字段当作"个人配置"处理。

### D4：projects.json 是 AD 内部状态，不是 CC 配置

AD 自己管的 `~/.claude/ad/projects.json` 只是"AD 记得有哪些项目、上次 apply 了哪个 profile"，CC 不读这个文件。这避免了和 CC 配置体系的耦合。

### D5：apply 是显式动作，不是后台同步

不做"profile 改了自动 sync 到所有 apply 过的项目"。用户编辑 profile → 显式点 Apply / Re-apply 才同步。降低意外覆盖项目本地修改的风险。

### D6：保留 legacy 全局覆盖流程

不是激进重构。老用户已习惯"覆盖全局"语义，删掉是 breaking。藏到 Advanced settings 里，标 deprecated，但仍可用。

### D7：写 shared 层前看 git 状态

写 `<project>/.claude/settings.json`（进 git 那个）前：

- 检查目标项目是 git 仓库
- 检查 working tree 是否 dirty
- 检查 `.claude/settings.json` 是否有未提交修改
- 任一异常 → 弹 ApplyDialog 警示，让用户 confirm

### D8：apply 不覆盖，是 merge + 冲突显式

如果目标层文件已有内容，apply 不直接覆盖：

| 情况 | 行为 |
|---|---|
| 目标文件不存在 / 是 `{}` | 直接写入 |
| 目标文件 + profile 层字段无重叠 | 自动 deep merge（保留原字段 + 加入新字段） |
| 目标文件 + 重叠字段值相同 | 自动 merge |
| 目标文件 + 重叠字段值不同 | **冲突**：apply 拒绝，返回 `Vec<Conflict>`，UI 让用户逐个选 keep / use / custom，重新 invoke 带 resolutions 才执行 |

实现：纯函数 `merge_with_conflicts(existing: Value, incoming: Value) -> (merged: Value, conflicts: Vec<Conflict>)` 在 `fs/merge.rs`。`Conflict` 形如 `{ key_path: "permissions.allow.fs", existing: Value, incoming: Value }`。

### D9：Project 身份 = canonical 绝对路径

- 添加时 canonicalize（解析 symlink + `~` 展开）
- path 是主键，唯一确定一个 project
- AD 不追踪 `mv`：用户移动目录后，AD 显示"目录不存在"，需手动 remove + re-add
- 重复添加同一 path：去重，更新 `added_at` 即可
- AD 不监听文件系统变化（不当 fsnotify watcher）

### D10：Backup 路径统一

所有 apply 产生的 backup 集中存于：

```
~/.ad/backups/<ISO8601-ts>-<project-slug>-<layer>.json
```

例：`~/.ad/backups/2026-05-24T08-30-00Z-users-yuanxuan-projects-foo-shared.json`

`<project-slug>` 从 path 生成（`/Users/x/projects/foo` → `users-x-projects-foo`），便于按时间扫和按项目过滤。AD history 面板从这一处统一读，删 project 不连带删 backup。

### D11：AD 数据目录与 CC 完全分离

老的 `~/.claude/ad/*` 和 `~/.claude/profiles/*` 全部搬到 `~/.ad/` 下，新结构：

```
~/.ad/
├── profiles/<id>/profile.json    (从 ~/.claude/profiles/ 搬)
├── backups/<ts>-<slug>-<layer>.json  (从 ~/.claude/ad/backups/ 搬)
├── history/<ts>.<uuid>.json      (从 ~/.claude/ad/history/ 搬)
├── active                         (从 ~/.claude/ad/active 搬)
└── state/projects.json           (新文件)

~/.claude/                         只放 CC 自己的东西
└── settings.json                  AD 写的全局覆盖路径（legacy 流程才动）
```

**迁移**：M1 中实现，AD 启动时若 `~/.ad/` 不存在但老路径存在 → 物理 `mv` 整体搬过去 + 写一份 `~/.claude/AD_MOVED_TO_HOME.txt` 留底说明。幂等（`~/.ad/` 已存在则跳过）。

测试 home 覆盖 env var 改名为 `AD_HOME` 但语义升级：覆盖整个 `~/.ad/` 根，不只是 `~/.claude/ad/`。

理由：AD 和 CC 是两个产品，数据混在一起将来卸载 AD 时不知道哪些是 AD 的；分开后语义清晰。

### D12：Project 添加机制 — 三路径 + 移除 folder picker

| 优先级 | 机制 | 用户操作 | 实现要点 |
|---|---|---|---|
| 主 | Auto-detect from scan roots | 0 操作（启动后弹推荐 banner） | 扫一组根目录（默认 `~/.claude/projects/`，用户可加自定义），UI 显示列表 + 搜索过滤框（substring match） |
| 次 | Drag-drop 单文件夹到 AD 窗口 | 1 个动作 | Tauri `onDrop` 事件；只接受单个文件夹（多于 1 → reject + toast）；非目录 → reject |
| 三 | Paste path + tab-complete | 粘贴 / 输入 + Tab | Tauri 后端命令 `complete_path_prefix(prefix) -> Vec<String>`；前端 input 监听 onChange（live dropdown） + onKeyDown Tab（补全到最长公共前缀，单匹配则补全后追加 `/`） |

**移除** folder picker 主入口（不进 Advanced，直接砍掉）。

#### 扫描根目录可扩展

主路径的 scan roots 是一个可配置列表，存在 `~/.ad/state/scan_roots.json`：

```json
[
  { "path": "~/.claude/projects",  "kind": "cc_projects_meta", "builtin": true },
  { "path": "~/dev",               "kind": "generic" },
  { "path": "~/work/clients",      "kind": "generic" }
]
```

两种扫描策略：

| kind | 策略 |
|---|---|
| `cc_projects_meta` | CC 自己的 per-project 元数据目录，反解 `<encoded-path>` 拿原始项目路径 |
| `generic` | 任意目录，walk 一层深（可配置 max_depth），任何含 `.git/` 或 `.claude/` 的子目录视为 project 候选 |

DetectedProjectsModal 顶部有"Scan additional dirs"区域，用户可以加 / 删 generic 根。`~/.claude/projects` 是 builtin，不可删（用户能 toggle 是否启用）。

理由：folder picker 步数最多（点按钮 → 弹 dialog → 多次点击导航 → 选定 → OK），其他三种都更快。覆盖 95%（CC 历史） + 自定义扫描根（剩 5%）。

---

## 进展

> **执行时只更新本节、意外发现、决策日志、结果回顾。HTML 在批准后冻结。**

### M1 — 数据目录迁移 + Schema 升级 + Profile 迁移（验证标准：cargo test 通过；启动时数据从 ~/.claude/ 搬到 ~/.ad/；v0.1 profile 自动升级 layers shape；幂等）

- [x] (2026-05-23 1755Z) `fs/paths.rs` 改造完成：新增 `ad_home()` / `state_dir()` / `projects_state_path()` / `scan_roots_state_path()` / `cc_projects_dir()` / `legacy_v1_ad_dir()` / `legacy_v1_profiles_dir()`；profiles/backups/history/active/state 全部走 `ad_home()`；CC 自身路径走 `claude_dir()`。所有 33 个测试通过
- [x] (2026-05-23 1759Z) `migration.rs::migrate_data_dir_to_home` 完成 + 5 个测试覆盖（全量迁移 / 仅 ad / 仅 profiles / 无数据 / 已存在 ~/.ad/ 跳过）+ lib.rs setup hook（在 legacy 迁移之前调用）
- [x] (2026-05-23 1804Z) `models.rs::ProfileLayers` 完成（shared/local 用 `Option<Value>` 保留任意 JSON，env 用 `BTreeMap<String,String>`）+ `ProfileFile.layers` 字段（`#[serde(default)]` 保 v1 文件向后兼容）+ 3 个新测试（v1 兼容读 / v2 round-trip / 空 layers 序列化省略）
- [x] (2026-05-23 1808Z) `migration.rs::migrate_v1_profiles_to_layered` 完成（备份到 `<path>.v1.bak`，settings 整体序列化到 `layers.local` 的 Value，settings 字段重置为默认）+ lib.rs setup hook + 3 个测试（迁移 / 已 layered 跳过 / 空 settings 跳过）+ helpers `ProfileLayers::is_empty()` / `ClaudeSettings::is_empty()`
- [ ] (待开始) 单元测试：数据目录迁移幂等；旧 schema 解析仍能跑通；迁移后 layers.local == 老 settings；两次启动不重复迁移
- [x] (2026-05-23 1812Z) `commands/profiles.rs` 实际无需改动 —— `#[serde(default)]` 自动处理向后兼容、paths.rs 自动指向新位置。新增 `layered_profile_roundtrip_persists_layers` 测试显式验证 layered shape 来回。45 个测试全过

### M2 — Project 状态 + apply 命令 + merge 引擎（验证标准：通过 invoke 调用能写到 tempdir 项目里的 .claude/settings.local.json，全局文件 mtime 不变；merge 冲突检测有单元测试覆盖）

- [x] (2026-05-23 1755Z) `fs/paths.rs` 加 `projects_state_path()` + `state_dir()` + `scan_roots_state_path()` (M1 一并完成)
- [x] (2026-05-23 1830Z) `fs/merge.rs` 完成 — `merge(existing, incoming, resolutions) -> MergeOutcome::{Merged | NeedsResolution(Vec<Conflict>)}`，纯函数，15 测试覆盖（disjoint / equal / 嵌套冲突路径 / 数组等同与差异 / 类型不匹配 / 三种 Resolution / partial / 根级 / Resolution serde tag）
- [x] (2026-05-23 1840Z) `commands/scan_roots.rs` 完成 — list / add / remove / set_enabled，存 `~/.ad/state/scan_roots.json`，默认 builtin entry 指向 `~/.claude/projects` (`cc_projects_meta`)。canonicalize on add，path_matches 容忍 `~`/canonical 三种输入形式。7 测试覆盖。models.rs 新增 `ScanRoot` + `ScanRootKind`
- [x] (2026-05-23 1855Z) `commands/discover.rs::scan_for_projects` 完成 — `cc_projects_meta` 反解走"读 jsonl 拿 cwd 字段"（CC 自身存的真实 cwd，避免 lossy 编码反推）；`generic` walk 一层深找含 `.git/` 或 `.claude/` 的子目录；多 root 去重；`already_added` 字段对照 projects.json。5 测试覆盖。models.rs 新增 `Project`/`LastApplied`/`DetectedProject`
- [x] (2026-05-23 1900Z) `commands/path_complete.rs::complete_path_prefix` 完成 — tilde 展开、文件/隐藏过滤、cap 50、按子串前缀匹配。8 测试覆盖
- [x] (2026-05-23 1908Z) `commands/projects.rs` 完成 — list/add/remove/rename/get_status (含 git dirty + .gitignore 检查)。canonicalize 去重，path_match_target 容忍 raw/tilde/canonical 三种输入。10 测试。models.rs 新增 `Project`/`LastApplied`/`ProjectStatus`
- [x] (2026-05-23 1920Z) `commands/apply.rs::apply_profile_to_project` 完成 (M2.6 — 8 测试)
- [x] (2026-05-23 1920Z) backup 路径生成 `<ISO8601-ts>-<project-slug>-<layer>.json` + slug 函数单元测试
- [~] integration test `tests/layered_apply.rs` 跳过 — 单元测试已覆盖等同语义（含 `apply_global_settings_json_is_never_written`）
- [x] (2026-05-23 1925Z) `lib.rs` 注册 12 个新命令

### M3 — UI: 分层 profile 编辑器 (2026-05-23 1855Z) ✅

- [x] `lib/profileSchema.ts` 加 `ProfileLayersSchema` + `layers` 字段（默认 `{ env: {} }`，向后兼容 v1 profile）+ `settingsFromLayers` helper
- [x] `ProfileEditor.tsx` 改成三 tab — Shared/Local/Env，每个独立状态。Shared 红色警示条 "writes to settings.json — git visible"，Local 绿色 "settings.local.json — gitignored"，Env 是 KV 表 + Copy export commands 按钮
- [x] Activate 按钮重命名为 "Activate (legacy)" 并加 title hint
- [x] save 时 `settingsFromLayers(layers)` 同步老 settings 字段（保 legacy 激活兼容）
- [x] zod schema 在 normalize 时填默认值（修白屏 bug：backend 用 `skip_serializing_if` 省略空 env 导致 frontend `Object.keys(undefined)` 崩）
- [x] 6 个新 schema 测试 + 已有 10 测试全过；typecheck / lint / dev 三 tab 显示正常

### M4 — UI: Projects panel + 三种添加机制 + Apply UX (2026-05-23 1915Z) ✅

实测通过（用户）：双栏布局正常 / Detected banner 显示 47 项目 / tab-complete dropdown 工作 / Apply 走通。子任务列表见下方原计划项 + M4.1-M4.6 任务跟踪。

**意外发现 F3 ✅ 已修**：`AddProjectInput` 的 dropdown 用了 `bg-popover` 类，但 globals.css 没定义 `--popover` 变量 → 渲染成透明。改用 `bg-background` + `shadow-lg ring-1 ring-black/5`。教训：写新 UI 只用 globals.css 已定义的 CSS 变量。

**意外发现 F4 ✅ 已修**：组件大量用了 Anthropic 调色板 (`clay/rust/olive`) 但 tailwind.config.ts 没定义这些色 → 类被无声忽略。加 4 个色到 tailwind config，HMR 后生效。教训：跨上下文复用设计 token 前先确认目标 tailwind 配置。

**M4 子任务（原计划项）**：

- [ ] (待开始) 后端 `commands/discover.rs::scan_roots()`：依次扫所有 scan roots（CC 元数据反解 / generic walk），返回候选列表（含路径有效性 + 是否已添加 + 来源 root）
- [ ] (待开始) 后端 `commands/scan_roots.rs`：scan roots CRUD + toggle，存 `~/.ad/state/scan_roots.json`，默认含 `~/.claude/projects` builtin
- [ ] (待开始) 后端 `commands/path_complete.rs::complete_path_prefix(prefix)`：返回该 prefix 下的子目录列表，支持 `~` 展开
- [x] (2026-05-23 1900Z) `lib/projectTypes.ts` + `store/projects.ts` + `lib/tauri.ts` 14 wrapper 全部加完，typecheck / lint / 16 测试全过
- [ ] (待开始) `ProjectsPanel.tsx` 列出已知项目 + 双击 rename
- [ ] (待开始) project 行支持"目录不存在"灰态 + Remove dialog
- [ ] (待开始) `DetectedProjectsModal.tsx`：启动后自动弹（未关闭过则弹），列表 + 顶部搜索框（substring filter）+ 多选 checkbox + [Add selected] + 底部"Scan additional dirs"管理 generic roots（add / remove / toggle builtin）
- [ ] (待开始) `ProjectDropZone.tsx`：drop zone in Projects 栏，单文件夹 only（多于 1 → toast reject）
- [ ] (待开始) `AddProjectInput.tsx`：input + onChange 调 `complete_path_prefix` 显示 dropdown；onKeyDown Tab 补全到最长公共前缀（单匹配 → 补全后追加 `/` 进入子目录）
- [ ] (待开始) `ApplyDialog.tsx` 三步：① 选 layers + git 警示 ② 显示 unified diff ③ 如有冲突显示冲突解决表
- [ ] (待开始) `App.tsx` 改双栏布局：左 Profiles，右 Projects
- [ ] (待开始) tray 菜单更新：显示"最近 apply 过的项目"作为 quick-action（最多 5 条）

### M5 — Sunset / hide legacy 全局覆盖 (2026-05-23 1925Z) ✅

- [x] `store/uiSettings.ts` — `useUiSettings` zustand store + `showLegacyActivation` 字段，localStorage 持久化
- [x] `components/AdvancedSettings.tsx` — gear 图标按钮 + 偏好弹窗，单 toggle 控制 legacy 可见性 + 说明文字
- [x] `App.tsx` titlebar 加 gear 图标
- [x] `ProfileEditor.tsx` 条件渲染 `[Activate (legacy)]` 按钮（默认不显示，toggle 开启后显示）
- [x] 老 `commands::activate::activate_profile` 完全保留，行为不变
- [x] AGENTS.md 更新主架构段落 + 数据位置（指向 `~/.ad/`）
- [ ] README / PRODUCT_SENSE.md 文档更新（小活，可放下次清理）

---

## 意外发现

执行期补充：

- **F1** (2026-05-23 1815Z) ✅ 已修复：M1 原本会清空 v1 profile 的 `settings` 字段，导致旧激活流程 (`commands/activate.rs::activate_profile_inner`) 把 `~/.claude/settings.json` 写成空——会擦掉用户账号配置
  - **证据**：原 `migrate_one_v1_to_layered` 有 `profile.settings = ClaudeSettings::default()`
  - **修复**：去掉重置语句，迁移变成"只复制不清空"——`layers.local` 和 `settings` 在磁盘上**同时存在 + 同内容**，旧激活流程继续可用，新 apply（M2）用 `layers`，二者并存到 M5 sunset
  - **代价**：M3 layered 编辑器（用户在 UI 改 layers）保存时必须同步更新 settings 字段，否则两者漂移。已记入 TODO M3
- 发现：...
  证据：...

---

## 决策日志

执行期补充：

- **D13** (2026-05-23 1755Z, yuanxuan)：`AD_HOME` env 保持原语义（覆盖 `$HOME`），不改成 `~/.ad/` 直接覆盖
  - **理由**：新 paths.rs 同时需要测试覆盖 `~/.ad/` 和 `~/.claude/`（后者用于 cc_projects_dir 之类）；$HOME 级覆盖更通用，一个 env 搞定两边
  - **影响**：与 plan 描述略有出入，plan 写"AD_HOME env 升级为整个 ~/.ad/ 根的覆盖"——实际未改语义，只是新增了 `ad_home()` 函数

- **D14** (2026-05-23 1808Z, yuanxuan)：profile 文件保持 flat (`~/.ad/profiles/<id>.json`)，不改成 per-id 子目录
  - **理由**：现有 commands/profiles.rs 用 flat 文件；改成子目录工作量大无明显收益；`.v1.bak` 作为相邻文件 (`<id>.json.v1.bak`) 同样工作
  - **影响**：plan / HTML 在 home 目录可视化里画了 `profiles/<id>/profile.json`，实际是 `profiles/<id>.json`

---

## 结果回顾

### M1 — 数据目录迁移 + Schema 升级 + Profile 迁移 (2026-05-23 1815Z)

**完成**：
- `fs/paths.rs` 大改：`ad_home()` 等所有 AD 路径函数指向 `~/.ad/`；新增 `cc_projects_dir()`、`state_dir()`、`projects_state_path()`、`scan_roots_state_path()`、`legacy_v1_*()` 一组
- `migration.rs::migrate_data_dir_to_home`：物理 `mv` `~/.claude/{ad,profiles}` → `~/.ad/`，留 `AD_MOVED_TO_HOME.txt` 标记，幂等
- `migration.rs::migrate_v1_profiles_to_layered`：v1 `settings` 整体序列化到 `layers.local`，原文件备份到 `.v1.bak`，幂等
- `models.rs::ProfileLayers`：`shared`/`local` 是 `Option<Value>`（保留任意 JSON），`env` 是 `BTreeMap<String,String>`。加 `is_empty()` 帮手
- `commands/profiles.rs` 因 `#[serde(default)]` 自动适配新 schema 和新路径，无需改动
- `lib.rs` setup 顺序：data dir migration → legacy profile migration → v1→layered migration

**测试覆盖（43 lib + 2 integration 全过 + clippy + fmt + pnpm lint/typecheck/test）**：
- paths_resolve_under_home_override（覆盖所有路径函数）
- 5 个 data_dir migration 测试（全量 / 仅 ad / 仅 profiles / 无数据 / 已存在跳过）
- 3 个 v1→layered 测试（迁移 / 已 layered 跳过 / 空 settings 跳过）
- 3 个 ProfileLayers 测试（v1 兼容读 / v2 round-trip / 空 layers 序列化省略）
- 1 个 layered profile end-to-end round-trip 测试

**用户感知**：=0。app 行为完全不变，迁移在 setup 静默执行。

**实测验证 (2026-05-23 1806Z) ✅**：

跑 `pnpm tauri dev`，setup hook 触发两次迁移，dev 窗口正常打开。Log 输出：

```
INFO ad_lib: migrated AD data from ~/.claude/ to ~/.ad/
INFO ad_lib: migrated v1 profiles to layered shape count=2
INFO ad_lib: ad ready
```

文件系统验证：
- `~/.ad/{active, backups, history, profiles}` 全部就位
- `~/.claude/{ad, profiles}` 已不存在（mv 走了）
- `~/.claude/AD_MOVED_TO_HOME.txt` 已写
- `~/.ad/profiles/{alibaba, homi}.json.v1.bak` 备份存在
- `untitled-1.json` 正确跳过（`settings.env: {}` 空），无 .v1.bak
- `homi.json::layers.local` 7 个字段（包括非显式的 `enabledPlugins`、`extraKnownMarketplaces`、`skipDangerousModePermissionPrompt`、`statusLine`），证明 `serde_json::to_value(&settings)` 经 `extra` 字段保住了所有未知键
- `homi.json::settings` 同步保留（F1 修复验证通过，legacy 激活仍可读）

**遗留 / 不在 M1 范围**：
- 迁移后 `settings` 和 `layers.local` 在磁盘上同时存在且同内容。M3 的 layered 编辑器在保存时必须同步更新 settings 字段（不然旧激活会读到 stale 数据）。M5 sunset legacy 时一并清理。

### M2 — Project 状态 + apply 命令 + merge 引擎 + scan roots (2026-05-23 1925Z)

**完成**：
- `fs/merge.rs::merge` 纯函数 deep merge + 冲突检测（15 测试）
- `commands/scan_roots.rs` 5 个命令 (CRUD + toggle) + `~/.ad/state/scan_roots.json` 持久化 + canonicalize + 默认 builtin entry (`~/.claude/projects` cc_projects_meta)（7 测试）
- `commands/discover.rs::scan_for_projects` — CC 历史用读 jsonl 拿 cwd 字段反解，generic 走 `.git`/`.claude` 信号检测，去重 + already_added 标注（5 测试）
- `commands/path_complete.rs::complete_path_prefix` — tilde 展开 + 文件/隐藏过滤 + cap 50（8 测试）
- `commands/projects.rs` 5 个命令 — CRUD + `get_project_status` 含 git dirty + .gitignore 检查（10 测试）
- `commands/apply.rs::apply_profile_to_project` — 三种 outcome (Applied / NeedsResolution / GitDirtyBlocked)，每层独立 merge，写 backup 到 `~/.ad/backups/<ts>-<slug>-<layer>.json`，env 层生成 export 片段（8 测试）
- `lib.rs` 注册 12 个新 Tauri 命令
- models.rs 新增 `Project` / `LastApplied` / `ProjectStatus` / `ScanRoot` / `ScanRootKind` / `DetectedProject`

**测试覆盖**：96 lib 测试 + 2 integration 全过（M1 是 43，M2 净增 53）。clippy / fmt / pnpm typecheck/lint/test 全过。

**关键不变量验证**：
- `apply_global_settings_json_is_never_written` 测试：apply shared+local 后，`~/.claude/settings.json` mtime 不变 ✅
- merge 冲突 + resolution 测试：先调返回 NeedsResolution，再调带 resolutions 返回 Applied ✅
- 数据隔离：所有测试用 AD_HOME tempdir，不污染开发者真实 ~/.ad/ 或 ~/.claude/ ✅

**用户感知**：仍然 0。M2 是后端能力，UI（M3+M4）才让用户感知。可以 dev 启动验证后端命令（手动 invoke），但默认 UI 还看不到新功能。

**执行期补充决策**：
- **D15** (2026-05-23 1832Z, yuanxuan)：merge 不对数组做 union/concat。等同的数组通过，不等的数组报冲突让用户选 keep/use 整体。理由：数组语义按字段不同（permissions.allow 是 set，path 列表是 ordered list 等），AD 不可能猜对，让用户显式决定更安全。
- **D16** (2026-05-23 1850Z, yuanxuan)：CC history 反解走"读 jsonl 拿 cwd 字段"，不走"naive 反推 encoded path"。理由：CC 编码是 lossy（`/`、`_`、`.` 都映射成 `-`），naive 反推无法处理含 `_` 或 `-` 的真实路径；jsonl 里有 CC 自己写的真实 cwd，准确率 100%。
- **D17** (2026-05-23 1910Z, yuanxuan)：apply.rs 的 git dirty 检查走 `git status --porcelain` Command 调用，不引入 git2 crate。理由：依赖最小化；git 是开发者必装的；命令失败时保守判定 dirty，UI 能进一步处理。

### M5 — Sunset legacy 全局覆盖 (2026-05-23 2325Z)

**完成**：
- `store/uiSettings.ts` — `useUiSettings` zustand store，`showLegacyActivation` 持久化到 `localStorage` (`ad.ui-settings.v1`)
- `components/AdvancedSettings.tsx` — `AdvancedSettingsButton` 在 titlebar 右上角呈现 gear 图标，点击弹出 Advanced settings dialog，单 toggle 控制 legacy 按钮可见性 + 显示提示语
- `ProfileEditor.tsx` 头部按钮条件渲染：默认只有 [Save]，toggle 开启时多出 [Activate (legacy)]
- legacy `commands::activate::activate_profile` 完全保留，逻辑零修改
- AGENTS.md 更新：主架构段落、数据位置全部指向 `~/.ad/`，引用 completed/ 里的 ExecPlan

**实测**（用户）：默认 UI 不见 legacy 按钮 ✅；点 gear 弹 Advanced settings ✅；toggle 开启后按钮即时出现 ✅；关 toggle 即时消失 ✅；重启 AD 状态保留 ✅。

**整体 5 个 milestone 完成**：M1 数据迁移 → M2 后端能力 → M3 三 tab 编辑器 → M4 主窗口 + 三种添加 + Apply UX → M5 sunset legacy。

---

## 整体结果回顾 (2026-05-23 2325Z)

**预定目标 vs 实际达成**：

| 原目标 | 实际状态 |
|---|---|
| 写到 `<project>/.claude/settings.local.json`，全局不动 | ✅ 后端验证（apply_global_settings_json_is_never_written 测试 + 用户实测 mtime 不变） |
| 已运行 CC 实例不受影响 | ✅ 不写全局 = 已运行实例自然不感知 |
| 同 profile 应用到多项目互不串扰 | ✅ project 列表独立维护，apply 写到指定 project 的本地路径 |
| layered profile（shared/local/env） | ✅ 三 tab 编辑器 + apply 时按层独立处理 |
| 已存在 settings 时 merge + 冲突显式确认 | ⚠️ 部分：merge 引擎完整，冲突 UI 是只读列表（TD-005，后续小迭代加交互式 resolution） |
| 添加项目零摩擦：auto-detect / drag / tab-complete | ✅ 三种全实装 + scan roots 可扩展 |
| AD 数据从 `~/.claude/` 独立到 `~/.ad/` | ✅ M1 启动时自动迁移；用户实测搬家正确 |

**测试覆盖最终统计**：
- Backend: 96 lib + 2 integration = **98 tests**（M1: 6 + M2: 53 = +59 净增）
- Frontend: **16 tests**（M3 净增 6）
- clippy / cargo fmt / pnpm typecheck / pnpm lint / pnpm test 全部通过

**生产代码增量**：
- Rust src/: 6 个新模块（fs/merge.rs / commands/{apply, projects, scan_roots, discover, path_complete}.rs）+ 6 个新 model 类型 + 12 个新 Tauri 命令
- TS src/: 5 个新组件（ProjectsPanel / AddProjectInput / ProjectDropZone / DetectedProjectsModal / ApplyDialog / AdvancedSettings）+ 2 个新 store（projects, uiSettings）+ 数据层（projectTypes, schema 扩展）
- ~22 文件改动 / 新增

**执行期决策（D13-D17）和意外发现（F1-F4）** 详见各自章节。F1 是设计层面的关键发现（迁移不能清空 settings 否则 legacy 激活擦掉用户配置），F2 是 HMR 后白屏（zod 默认值修复），F3-F4 是 M4 样式问题（缺 CSS 变量 / 缺 tailwind 色定义）。

**遗留 / 后续工作**：
- TD-005：ApplyDialog 冲突解决 UI 需要从只读列表升级为交互式 keep/use/custom 选择器
- TD-006：sunset 完成后整体移除 `settings` 字段 + settingsFromLayers
- README / PRODUCT_SENSE.md 文档更新（小活）
- tray menu "最近 apply 项目" quick action（M4.5 原计划项，跳过）

**用户体验拐点**：M4 完成后默认 UX 完全是新的 — 用户打开 AD 看到三栏（Profiles + Editor + Projects），主流程是"选 profile + 选 project + Apply"，全局覆盖路径隐藏在 Advanced 后面。这达成了最初的产品目标。

---

### M3 — UI 三 tab profile 编辑器 (2026-05-23 1855Z)

**完成**：
- `lib/profileSchema.ts`：`ProfileLayersSchema` + `ProfileFileSchema.layers` 默认值 + `settingsFromLayers(layers)` 帮手 + 6 个新测试（v1/v2 兼容、settingsFromLayers 各场景）
- `ProfileEditor.tsx` 三 tab 重构：
  - **Shared tab** — monaco JSON editor + 红色警示条
  - **Local tab** — monaco JSON editor + 绿色提示条
  - **Env tab** — 自定义 KV 表（add row / edit / remove）+ Copy export commands 按钮（POSIX-safe 单引号转义）
  - tab 标签显示字段数 `Shared (3)` `Local (5)` `Env (0)`，无效时变 ⚠
- `Activate` 按钮重命名为 "Activate (legacy)" + tooltip
- 保存时通过 `settingsFromLayers` 同步老 `settings` 字段，保 legacy 激活兼容（F1 修复策略延续）
- 测试 fixture 加 `layers: { env: {} }` 字段

**实测验证 (用户)**：刷新 AD 窗口 → 三 tab 正常显示，Local tab 默认呈现 v1 迁移过来的 `enabledPlugins` / `extraKnownMarketplaces` 等字段。

**意外发现 F2** ✅ 已修：HMR 后 ProfileEditor 渲染白屏。原因：Rust serde 用 `skip_serializing_if` 省略了空 layers 字段（包括 env），前端拿到 `layers: {}` 后 `Object.keys(undefined).length` 抛异常。修复：编辑器入口走 `ProfileFileSchema.parse(profile)` 让 zod 默认值补全字段（`env: {}` 等）。这是数据边界 normalize 的标准做法，比每个访问点加 `?? {}` 更稳。

**测试**：6 schema + 10 现有 = 16 frontend 测试全过；typecheck / lint clean。

**整体进度**：
- ✅ M1（数据迁移 + schema 升级）
- ✅ M2（后端能力齐了）
- ✅ M3（三 tab profile 编辑器）
- ⏳ M4（UI: Projects panel + 三种添加 + Apply UX）
- ⏳ M5（sunset legacy）

---

**M2 实测验证 (2026-05-23 1840Z) ✅**：

dev 中通过 Tauri DevTools console 调用全部新命令，结果：
- `list_scan_roots` → 1 个 builtin 指向 `~/.claude/projects` ✅
- `scan_for_projects` → **47 个 CC 历史项目反解出来**（含 3 个 `missing` 标记的失效路径）✅
- `complete_path_prefix("~/ai_workspace/")` → 26 个候选 ✅
- `add_project /Users/yuanxuan/ai_workspace/ad` → 写入 `~/.ad/state/projects.json` ✅
- `get_project_status` → `gitDirty: true` / `isGitRepo: true` 等都准 ✅
- `apply_profile_to_project(homi, AD, ['local'])` → 写出 `ad/.claude/settings.local.json`（含 enabledPlugins/extraKnownMarketplaces/statusLine 等全部 layered 字段）✅
- 第二次 apply 触发 backup：`~/.ad/backups/2026-05-23T18-38-49Z-users-yuanxuan-ai-workspace-ad-local.json` ✅（D10 命名 + 幂等 + 备份语义齐活）

后端零错误零 warn。

---

## 上下文和方向

### AD 当前状态（2026-05-24）

AD 是 Tauri 2 + React 18 + Rust 桌面 app，macOS only。当前 profile 模型是单层的 `{ id, name, color, settings }`，激活流程是：

1. 备份 `~/.claude/settings.json` 到 `~/.claude/ad/backups/<ts>.json`
2. 用 `fs::atomic::write_atomic` 写新 settings 到 `~/.claude/settings.json`
3. append 一条 `ActivationLogEntry` 到 `~/.claude/ad/history/<ts>.<uuid>.json`

代码主路径：

- `src-tauri/src/models.rs` — `ProfileFile`, `ActivationLogEntry`
- `src-tauri/src/commands/activate.rs::activate_profile_inner` — 激活核心
- `src-tauri/src/commands/profiles.rs` — list / get / save / delete
- `src-tauri/src/fs/paths.rs` — 所有路径解析
- `src/store/profiles.ts` — 前端 Zustand store
- `src/components/ProfileEditor.tsx` — profile 编辑器（单 monaco editor）

### CC 的配置分层（基于官方文档 `code.claude.com/docs/en/env-vars`）

```
全局 ~/.claude/settings.json            ← AD 想避免动这个
项目 .claude/settings.json              ← 团队共享，进 git
项目 .claude/settings.local.json        ← 个人 + 项目级，gitignore
env vars                                ← 优先级最高，覆盖任何 settings 字段
```

env var 例：`ANTHROPIC_API_KEY`、`ANTHROPIC_BASE_URL`、`ANTHROPIC_MODEL`，`ANTHROPIC_MODEL` 覆盖 settings 里的 `model` 字段。

### 重构后的概念模型

```
AD profile = {
  id: string
  name: string
  color: string
  layers: {
    shared: SettingsBlock | null   // → <project>/.claude/settings.json
    local:  SettingsBlock | null   // → <project>/.claude/settings.local.json
    env:    Record<string, string> // → 复制 export 片段
  }
}

AD project = {
  path: string                     // canonical 绝对路径，主键
  display_name: string             // 默认 path 的 basename，可手改
  added_at: ISO8601
  current_profile_id: string | null
  last_applied: {
    profile_id: string
    timestamp: ISO8601
    layers: ('shared' | 'local' | 'env')[]
    backup_paths: string[]         // 本次 apply 产生的 AD backup 文件
    conflicts_resolved: number     // 本次解决的冲突数（审计用）
  } | null
}
```

### Project 管理协议

**添加机制**（详见 D12）：

- **主路径 · auto-detect**：启动时扫 `~/.claude/projects/` 反解出 CC 用过的项目 → 弹 modal 让用户搜索过滤 + 多选添加
- **次路径 · drag-drop**：拖单个文件夹到 Projects 栏 drop zone（多于 1 文件夹 → 拒绝）
- **三路径 · tab-complete paste**：input 框输入 / 粘贴路径，live dropdown 显示候选，Tab 键补全（terminal-like）
- **不做** folder picker（步数最多，被 D12 砍掉）

**生命周期**：

- **添加流程**：任何机制 → canonicalize path → 去重 → write_atomic 写 `~/.ad/state/projects.json`
- **删除**：从 list 移除 + 确认 dialog；**不动** `<project>/.claude/`、**不动** AD history、**不动** AD backups
- **rename**：仅改 display_name，path 不可变
- **路径不存在**：UI 灰态 + "Remove from list?" 提示；apply 按钮禁用
- **路径变了（用户 mv）**：AD 不追踪，需用户手动 remove + re-add
- **重复添加**：去重，更新 added_at
- **AD 不监听文件系统变化**：不当 fsnotify watcher，太重且对 macOS sandbox 不友好

### Backup 文件命名

```
~/.ad/backups/<ISO8601-ts>-<project-slug>-<layer>.json
```

`<project-slug>` = path 转换：`/Users/x/projects/foo` → `users-x-projects-foo`（小写 + `/` 替成 `-`）。AD history 面板从这一处统一读，按时间倒序。

---

## 工作计划

### M1 — schema 升级与迁移

**`src-tauri/src/models.rs`**：

```rust
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProfileLayers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProfileFile {
    pub id: String,
    pub display_name: String,
    pub color: String,
    #[serde(default)]
    pub layers: ProfileLayers,

    // Deprecated v1 field; kept for migration. Always None in newly-saved profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Value>,
}
```

**`src-tauri/src/migration.rs`**：

新增 `migrate_v1_profiles_to_layered() -> Result<usize>`：

1. 扫 `~/.claude/profiles/<id>/profile.json`
2. 解析为通用 `serde_json::Value`，检测是否有 `settings` 字段且没有 `layers` 字段（v1 标志）
3. 备份原文件到 `<id>/profile.json.v1.bak`（如果 .bak 已存在则跳过）
4. 构造 v2 shape：`layers.local = old.settings`，删 `settings` 字段
5. `write_atomic` 写回

测试：

- v1 profile → 迁移后 layers.local 等于老 settings
- 已是 v2 → 不重复迁移
- .bak 已存在 → 不覆盖

### M2 — projects.json 状态 + apply 命令

**`src-tauri/src/fs/paths.rs`**：

```rust
pub fn app_state_dir() -> Result<PathBuf, FsError> {
    Ok(ad_dir()?.join("state"))
}

pub fn projects_state_path() -> Result<PathBuf, FsError> {
    Ok(app_state_dir()?.join("projects.json"))
}
```

**`src-tauri/src/commands/projects.rs`**（新文件）：

```rust
#[tauri::command]
pub fn list_projects() -> CmdResult<Vec<Project>>;

#[tauri::command]
pub fn add_project(path: String) -> CmdResult<Project>;

#[tauri::command]
pub fn remove_project(path: String) -> CmdResult<()>;

#[tauri::command]
pub fn get_project_status(path: String) -> CmdResult<ProjectStatus>;
// status 包含：是否 git 仓库、git dirty 状态、当前 .claude/ 内容摘要
```

**`src-tauri/src/commands/apply.rs`**（新文件）：

```rust
pub struct ApplyOptions {
    pub layers: Vec<&'static str>, // 子集 of ["shared", "local", "env"]
    pub overwrite_dirty: bool,
}

#[tauri::command]
pub fn apply_profile_to_project(
    profile_id: String,
    project_path: String,
    options: ApplyOptions,
) -> CmdResult<ApplyResult>;
// ApplyResult 包含：被写的文件列表、备份路径、env export 片段（如果 env 在 layers 里）
```

apply 流程：

1. 解析 profile_id → ProfileFile
2. 解析 project_path → 校验存在、是目录
3. 对 `shared` 层：如选中，备份 `<project>/.claude/settings.json` 到 AD backups → write_atomic
4. 对 `local` 层：如选中，备份 `<project>/.claude/settings.local.json` 到 AD backups → write_atomic + 检查 `<project>/.gitignore`，如未含 `settings.local.json` 则附 ApplyResult.warnings
5. 对 `env` 层：生成 export 片段返回，不写文件
6. 更新 projects.json：last_applied_at + last_applied_layers + current_profile_id
7. 写一条 ActivationLogEntry（升级 schema 标 `kind: "apply"`，区别于老的 `kind: "global_overwrite"`）

### M3 — UI 分层编辑器

**`src/types/profile.ts`**：

```typescript
export const ProfileLayersSchema = z.object({
  shared: z.unknown().optional(),
  local: z.unknown().optional(),
  env: z.record(z.string(), z.string()).default({}),
});

export const ProfileSchema = z.object({
  id: z.string(),
  displayName: z.string().min(1),
  color: z.string().regex(/^#[0-9a-fA-F]{6}$/),
  layers: ProfileLayersSchema.default({ env: {} }),
});
```

**`src/components/ProfileEditor.tsx`** 改造为：

- 顶部：name + color picker
- 三 tab：
  - **Shared**: monaco editor (json) + 警示条 "Writes to <project>/.claude/settings.json (committed to git)"
  - **Local**: monaco editor (json) + 提示 "Writes to <project>/.claude/settings.local.json (gitignored)"
  - **Env**: key-value table + 下方 "Copy export commands" 按钮
- 底部：Save / Discard

### M4 — UI Projects panel

**`src/components/ProjectsPanel.tsx`** 列表项形如：

```
~/projects/foo  ●Work  Last applied 2026-05-24  [Re-apply] [Diff] [⋯]
~/projects/bar  ○      Never applied            [Apply ▼]
```

- 顶部 [+ Add project] 按钮 → 调用 Tauri dialog folder picker
- 每行点击展开：显示 `.claude/` 目录现状、git 状态、layers 当前值

**`src/components/ApplyDialog.tsx`**：

- 标题：Apply "Work" to ~/projects/foo
- Checkbox: ☐ Shared (.claude/settings.json) ☑ Local (.claude/settings.local.json) ☑ Env (copy snippet)
- Diff 预览：每个被勾选的层显示 unified diff
- 警告区：git dirty / .gitignore missing settings.local.json / settings.json 已存在不同内容
- [Cancel] [Apply]

### M5 — Sunset legacy 全局覆盖

**`src/components/AdvancedSettings.tsx`** 加：

```
Advanced
─────────────────────────────────
[ ] Show legacy global activation
    Apply a profile by overwriting ~/.claude/settings.json directly.
    This affects ALL Claude Code instances and is the v0.1 behavior.
    Marked deprecated in v0.2; kept for compatibility.
```

打开 toggle 后，profile 列表项显示 [Activate] 按钮（执行老的 `activate_profile_inner` 调用）。

---

## 验证和验收

### M1 验收

```bash
# 启动 dev，看 stdout 应该有迁移日志
pnpm tauri dev
# Expected:
# INFO migration: migrated 3 v1 profiles to layered shape

# 检查文件
ls ~/.claude/profiles/<some-id>/
# Expected: profile.json (v2 shape) + profile.json.v1.bak

cat ~/.claude/profiles/<some-id>/profile.json | jq '.layers.local'
# Expected: 老 settings 的内容
```

```bash
# Rust 测试
cd src-tauri && cargo test migration::tests::migrate_v1_to_layered
# Expected: passed
```

### M2 验收

```bash
# 集成测试
cd src-tauri && cargo test --test layered_apply
# Expected: passed (通过 AD_HOME 用 tempdir，不污染真实 ~/.claude/)
```

手动 dev 测试：

```bash
mkdir /tmp/ad-test-project && cd /tmp/ad-test-project && git init
# 在 AD 里 add_project("/tmp/ad-test-project")
# apply_profile_to_project(some_id, "/tmp/ad-test-project", { layers: ["local"] })

cat /tmp/ad-test-project/.claude/settings.local.json
# Expected: 是 profile.layers.local 的内容

stat -f %m ~/.claude/settings.json
# Expected: mtime 不变（全局文件没动过）
```

### M3 验收

`pnpm tauri dev` 打开 → 编辑 profile → 看到三 tab → 每 tab 独立编辑 → Save → 重启 AD → 三 tab 内容仍在。

### M4 验收

主窗口看到 Projects 栏 → 点 [+] 选 `~/projects/foo` → profile 旁边 Apply → 弹 ApplyDialog → 看到 diff → 点 Apply → 文件被写 → tray 菜单"最近 apply"显示这一条。

### M5 验收

默认 UI 看不到 legacy 全局按钮 → Advanced toggle 打开 → 看到 legacy [Activate] 按钮 → 点 → 行为与 v0.1 完全一致（覆盖 `~/.claude/settings.json`、产生备份）。

---

## 幂等性和恢复

- **M1 迁移**：幂等（已是 v2 shape 不重复迁移；`.v1.bak` 已存在不覆盖）
- **M2 apply**：每次 apply 都备份目标文件到 AD backups dir。如果 apply 中途崩溃，目标文件因为走 `write_atomic` 不会损坏；如果 ApplyResult 没记到 projects.json，下次 apply 状态可能不准但不破坏数据
- **回滚**：每次 apply 在 AD backups 里有副本，可以从 history 列表里 restore

---

## 接口和依赖

### Rust 侧

```rust
// src-tauri/src/models.rs
pub struct ProfileFile { ... layers: ProfileLayers, settings: Option<Value> }
pub struct ProfileLayers { shared, local, env }
pub struct Project { path: String, current_profile_id: Option<String>, last_applied_at: Option<DateTime<Utc>>, last_applied_layers: Vec<String> }
pub struct ApplyResult { written_files: Vec<PathBuf>, backups: Vec<PathBuf>, env_export_snippet: Option<String>, warnings: Vec<String> }

// src-tauri/src/commands/projects.rs
pub fn list_projects() -> CmdResult<Vec<Project>>
pub fn add_project(path: String) -> CmdResult<Project>
pub fn remove_project(path: String) -> CmdResult<()>
pub fn get_project_status(path: String) -> CmdResult<ProjectStatus>

// src-tauri/src/commands/apply.rs
pub fn apply_profile_to_project(profile_id, project_path, options) -> CmdResult<ApplyResult>
```

### TS 侧

```typescript
// src/lib/tauri.ts
export const tauri = {
  listProjects: () => invoke<Project[]>('list_projects'),
  addProject: (path: string) => invoke<Project>('add_project', { path }),
  removeProject: (path: string) => invoke('remove_project', { path }),
  getProjectStatus: (path: string) => invoke<ProjectStatus>('get_project_status', { path }),
  applyProfileToProject: (profileId, projectPath, options) =>
    invoke<ApplyResult>('apply_profile_to_project', { profileId, projectPath, options }),
  // legacy
  activateProfile: (id: string) => invoke<ActivationResult>('activate_profile', { id }),
};
```

### 依赖

无新增 Rust crate / npm 包。复用现有 `serde_json`, `chrono`, `fs::atomic`, `tauri-plugin-dialog`（已在 deps 中）。

---

## 里程碑规划

**M1 是基础**：先把数据模型 / 迁移搞定，老激活流程仍然可用，AD 行为没变化（用户感知 = 0）。这降低风险——迁移如果有 bug，立刻发现，不影响 UI。

**M2 是后端能力**：apply 命令独立可测（cargo test），不依赖 UI。完成时通过 invoke 能把 settings.local.json 写到 tempdir。

**M3 是前端能力**：profile 编辑器变三 tab。这步完成后用户能"看见" layers 概念，但还不能 apply（apply 在 M4）——这是有意为之的小步快跑：先让用户熟悉新 schema，再启用应用动作。

**M4 是 UX 收尾**：Projects panel + Apply dialog，让 M2 的能力对用户可见。完成后这是产品的新主路径。

**M5 是 sunset**：藏掉老的全局覆盖。完成后默认 UX 完全是新的。

每个里程碑独立可发布。M1+M2 完成不影响用户；M3 上线 = "新编辑器 + 老激活流"；M4 上线 = 主功能切换；M5 = 清理。

---

## 活文档维护

- 每完成一步：在「进展」打勾 + 时间戳 (YYYY-MM-DD HHmmZ)
- 任何意外（bug、性能问题、设计漏洞）→「意外发现」
- 任何新决策（D8、D9...）→「决策日志」
- 每个里程碑结束 → 简短「结果回顾」段落
- 整体完成 → 完整「结果回顾」 + 移到 `completed/`

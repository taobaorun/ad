# UI 改进 Pass 1 — 路径补齐 / 配色协调 / 默认中文

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

让 AD 主路径更顺手、视觉更协调、对中文用户更友好，分三块：

1. **添加项目无需打开 Finder**：在 ⌘K CommandPalette 内直接输路径，按 Tab/↑↓ 像 shell 一样补齐目录候选，Enter 添加。
2. **按钮配色不再突兀**：把近黑的 `--foreground` 换成温暖深色，让 ProjectSidebar `+⌘T` 按钮、ProfileChip / LayerToggle 选中态与 Anthropic 暖土调色板（clay/oat/olive）协调。
3. **默认中文界面**：接入 react-i18next，默认 zh，保留 en 作可切换 locale；AdvancedSettings 里加切换入口；CODE_STYLE.md 的"AD 是英文界面"段落同步更新。

用户能观察到：

- `⌘K` → 输入 `add ~/code/` → 实时弹出目录候选，Tab 补齐公共前缀，Enter 添加，路径错误时友好提示
- 选中的 profile chip / layer toggle 不再是刺眼的纯黑底，与左栏色环、detected 横幅的暖色基调一致
- 启动默认中文文案；AdvancedSettings 里切换到 English 后立即生效并重启保留

## 确认状态

- [x] **用户已确认** — HTML 基线冻结于 2026-05-24 05:36Z
- [x] 进入 Stage 3 执行（2026-05-24 05:36Z）

## 假设

任何一条不认同就告诉我，HTML+MD 一起改后再确认。

- **A1** 路径补齐入口在 **CommandPalette 内**（D1），不另起独立浮层 —— 复用现有 ⌘K 习惯，0 新窗口
- **A2** 补齐只列**目录**，不列文件；隐藏目录（以 `.` 开头）必须用户显式输入 `.` 前缀才显示（跟 shell 一致）
- **A3** `~` 自动展开为 `$HOME`；其他环境变量不支持（YAGNI）
- **A4** 后端新增一个 Tauri 命令 `list_directory(prefix: String)`，返回该前缀下的目录条目（含元信息：是否 git 仓库、是否已 added、是否在 detect 列表）
- **A5** 配色调整**最小化** —— 只改 `globals.css` 的 `--foreground` / `--primary` / `--ring` 三个 token，从冷调 `222.2 84% 4.9%` 改成温暖深 charcoal（接近 Anthropic palette 的 slate `#141413`，HSL 约 `60 5% 8%`）。组件 className 不动 —— 选中态仍走 `bg-foreground text-background`，但 foreground 已变暖
- **A6** i18n 默认 **zh**，但**保留 en 作可切换 locale** —— 开发者群体里中英分歧大，全删英文不可逆；可切换最小成本满足"默认中文"的真实需求
- **A7** i18n 框架选 **react-i18next**（生态成熟、TS 类型完善、bundle ~15KB），不引入 lingui / react-intl
- **A8** i18n 资源：`src/i18n/locales/{zh,en}.json`，按 namespace 组织（`common` / `palette` / `project` / `profile` / `apply` / `error`）
- **A9** 用户偏好的 locale 持久化到 `~/.ad/state/ui.json`（已有的 ui 状态文件）
- **A10** CODE_STYLE.md 第 68-71 行"AD 是英文界面"段落需更新为"默认中文，可切英文；所有文案必须走 i18n"
- **A11** `list_directory` 命令对路径越权零控制 —— 因为 add_project 本来就接受任意路径（用户自己输的），补齐不引入新攻击面；但 `..` 要按字面路径解析（不做特殊处理）

## 影响范围

### 新增（4 个文件）

```
src-tauri/src/commands/
└── fs_browse.rs              [NEW] list_directory 命令 + 单测
src/
├── i18n/
│   ├── index.ts              [NEW] i18next init + locale 持久化
│   └── locales/
│       ├── zh.json           [NEW] 默认中文资源
│       └── en.json           [NEW] 英文资源（保留旧硬编码文案）
└── lib/
    └── pathAutocomplete.ts   [NEW] 前端补齐 hook：状态、Tab/↑↓ 键、~ 展开
```

### 修改（~12 个文件）

```
src-tauri/src/
├── commands/mod.rs           registry export
└── lib.rs                    invoke_handler 注册 list_directory

src/
├── lib/tauri.ts              加 listDirectory wrapper
├── styles/globals.css        --foreground / --primary / --ring 改暖
├── App.tsx                   import './i18n' 触发 init
├── main.tsx                  i18n provider（若需 Suspense）
├── components/
│   ├── CommandPalette.tsx    识别 `add ` prefix → 切换路径补齐模式 + t()
│   ├── ProjectSidebar.tsx    t() 替换
│   ├── ProjectDetail.tsx     t() 替换；可能微调 chip 选中态
│   ├── ProfileEditDrawer.tsx t() 替换
│   ├── AdvancedSettings.tsx  t() 替换 + 加 Language switch
│   └── (其他含硬编码文案的组件) t() 替换

package.json                  + react-i18next + i18next
docs/CODE_STYLE.md            UI 文案章节更新
AGENTS.md                     可能补一行 i18n 入门提示
```

### 删除

- 无（i18n 是加法；旧 `window.prompt` 的 add path 行为被 CommandPalette 新流程取代，prompt 调用直接换掉）

## 关键决策

- **D1**：路径补齐在 CommandPalette 内完成（vs. 独立浮层 D2）
  - 理由：用户明确选 D1；复用 ⌘K 入口，0 新窗口，跟现有键盘流连续
- **D2**：i18n 用 `react-i18next`（vs. lingui / formatjs）
  - 理由：生态最成熟、TS 类型完善、bundle 小（~15KB）、API 简洁
- **D3**：默认 zh，保留 en 切换（vs. 仅 zh / 不可切换）
  - 理由：CODE_STYLE.md 既有约束"将来加中文必须走 i18n"——本任务正好兑现；可逆比锁死安全
- **D4**：配色调整**只改 3 个 CSS 变量**，不重做整套 token（vs. 全 token 重设计）
  - 理由：当前问题就是 `--foreground` 一颗"冷黑"嵌进了暖土系；改这一处即解；组件 className 不需要触动（最小修改原则）
- **D5**：`list_directory` 不缓存（vs. 加缓存）
  - 理由：用户输入节流即可；缓存复杂度（失效、TTL）超出 MVP 需要

## 进展

里程碑 M1 — 路径补齐
- [x] (2026-05-24 05:36Z) 后端命令已存在 — 复用 `complete_path_prefix`（见「意外发现」）。`cargo test --package ad-tauri path_complete` 既有测试。
- [x] (2026-05-24 05:42Z) 前端 `lib/pathAutocomplete.ts` hook 完成 — `usePathAutocomplete(input)` 返回 `{candidates, loading, completion}`；120ms debounce；带 seq 防过期请求
- [x] (2026-05-24 05:48Z) CommandPalette 识别 `add ` 前缀切换补齐模式 — `addMode = term.startsWith('add ') && term.slice(4).length > 0`；候选列表/无匹配提示/Tab 公共前缀补齐 / Enter add / footer 动态显示 tab+add 提示
- [x] (2026-05-24 05:48Z) 移除 `window.prompt` 旧入口 — `add-path` 命令改为 `openPalette('add ')` 触发补齐模式；grep 验证无 `window.prompt`
- [x] (2026-05-24 05:48Z) `pnpm typecheck` + `cargo test path_complete` 通过

里程碑 M2 — 配色协调
- [x] (2026-05-24 05:51Z) `globals.css` light 模式改 `--foreground` / `--primary` / `--ring` 为 `60 3% 8%`（暖深 charcoal ≈ Anthropic slate `#141413`）
- [x] (2026-05-24 05:51Z) dark 模式 token 同步：foreground/primary 改为 `42 29% 97%` (ivory)、ring 改为 `42 15% 80%`
- [ ] 手测端到端：dev 启动后肉眼对比 sidebar +⌘T、ProfileChip 选中、LayerToggle 选中、palette 高亮 row 不再撞色

里程碑 M3 — i18n 框架接入
- [x] (2026-05-24 05:58Z) `pnpm add i18next react-i18next` — i18next 26.2.0 + react-i18next 17.0.8
- [x] (2026-05-24 05:58Z) `src/i18n/{index.ts,locales/zh.json,locales/en.json}` — init + localStorage 持久化（`ad.lang.v1`）+ `setLanguage()` 导出
- [x] (2026-05-24 05:58Z) `App.tsx` 引入 `import './i18n'`，默认 zh
- [x] (2026-05-24 05:58Z) CommandPalette / ProjectSidebar / ProjectDetail 全部 `useTranslation()` + `t()`；`pnpm typecheck` 通过

里程碑 M4 — 剩余文案 + 语言切换
- [x] (2026-05-24 06:08Z) 替换：AdvancedSettings / App.tsx Titlebar / ActivateToast / ImportDialog / DetectedProjectsModal / HistoryDialog / ProfileEditDrawer / InlineConflictResolver / StatusRing。`EmptyState` 与 `DiffView` 为死代码（无 import）已跳过
- [x] (2026-05-24 06:08Z) AdvancedSettings 加 Language 切换（中文/English chip）。持久化使用 localStorage（key `ad.lang.v1`，与 sidebar 折叠态同一存储层）——见决策日志 D6
- [x] (2026-05-24 06:08Z) `pnpm typecheck` 通过
- **遗留**：`ProfileEditor`（profile 表单字段标签 ~40 keys）与 `HistoryPanel`（历史列表 ~15 keys）未 i18n，记入 tech-debt-tracker 留待下一 pass。原因：表单字段名涉及业务概念名，需独立设计中英对照；本次 M4 不强行做完以免风险扩散。

里程碑 M5 — 文档同步
- [x] (2026-05-24 06:11Z) `docs/CODE_STYLE.md` UI 文案段落重写：默认中文 + 必须走 i18n + 例外情形（console/Error/技术概念名）+ 正反例
- [x] (2026-05-24 06:11Z) `AGENTS.md` 项目结构树新增 `src/i18n/`；代码规范摘要 UI 文案行更新
- [x] (2026-05-24 06:11Z) `docs/exec-plans/tech-debt-tracker.md` 新增 TD-007（ProfileEditor + HistoryPanel 未 i18n）

完成时用 `[x] (YYYY-MM-DD HH:MMZ) 步骤描述` 形式打勾 + 时间戳。

## 意外发现

- **发现**：后端 `complete_path_prefix(prefix)` 命令、前端 `tauri.completePathPrefix()` wrapper、`lib.rs` 注册、8 个单测**全部已经存在**（`src-tauri/src/commands/path_complete.rs`）。原注释指它服务于已删除的 `AddProjectInput.tsx`（UI redesign 中拆除），现在是孤儿后端。
  - 证据：`commands/mod.rs:6` 有 `pub mod path_complete;`；`lib.rs:88` 已注册；`tauri.ts:74-75` 已暴露
  - 影响：M1.1（新建后端命令 + 单测）整个可跳过，节省大量工作。前端 hook 可直接消费现有命令。
  - 调整：A4「新增 `list_directory`」改为「复用 `complete_path_prefix`」；影响范围中的 `fs_browse.rs [NEW]` 取消。

## 决策日志

- **决策**：跳过 M1.1，复用既有 `complete_path_prefix` 命令而非新建 `list_directory`
  - 理由：行为完全匹配（~ 展开 / 隐藏目录处理 / 仅目录 / 上限 50 / 完整单测）
  - 日期/作者：2026-05-24 / Claude（执行期）

- **决策 D6**：locale 持久化用 localStorage（key `ad.lang.v1`），不写入 `~/.ad/state/ui.json`
  - 理由：现有 `useUiState` 的 sidebar 折叠态/active project 也用 localStorage（ui.ts 中既定模式，注释里明确写了 D7 选择 localStorage 以避免新 IPC）。保持同一层一致；若以后要做"跨设备同步偏好"再统一搬到后端
  - 日期/作者：2026-05-24 / Claude（执行期）

- **决策 D7**：ProfileEditor 与 HistoryPanel 不在 M4 范围
  - 理由：两者各自体量大（~40 + ~15 keys），且 ProfileEditor 的字段名涉及业务概念，需要做术语表对照。强行做完会让本 ExecPlan 风险扩散且 token 成本激增。挂到 tech-debt-tracker 留待 ui-polish-pass2
  - 日期/作者：2026-05-24 / Claude（执行期）

## 结果回顾

**编码完成状态**：M1–M5 五个里程碑的代码与文档全部落地。

| 里程碑 | 状态 | 关键产出 |
|---|---|---|
| M1 路径补齐 | ✅ | 复用现有 `complete_path_prefix`（节省后端工作）；新增 `lib/pathAutocomplete.ts` hook；CommandPalette 识别 `add ` 前缀切补齐模式；Tab/↑↓/Enter 三键；移除 `window.prompt` |
| M2 配色 | ✅ | `globals.css` 改 3 token + dark mode 同步；从 shadcn 默认 222.2°/4.9% 冷黑改为 60°/8% 暖深 charcoal（≈ Anthropic slate） |
| M3 i18n 框架 | ✅ | 装 i18next 26 + react-i18next 17；建 `src/i18n/{index.ts,locales/{zh,en}.json}`；默认 zh；3 个核心组件全 t() |
| M4 剩余文案 + 切换器 | ✅ | 替换 9 个组件（AdvancedSettings/App/ActivateToast/ImportDialog/DetectedProjectsModal/HistoryDialog/ProfileEditDrawer/InlineConflictResolver/StatusRing）；AdvancedSettings 加中英 chip 切换 |
| M5 文档 | ✅ | CODE_STYLE.md UI 文案章节重写；AGENTS.md 项目结构 + 规范摘要更新；tech-debt 新增 TD-007 |

**自动化验证全过**：
- `pnpm typecheck` ✅
- `cargo clippy --all-targets -- -D warnings` ✅
- `cargo test` ✅（98 passed, 1 ignored, 0 failed；含 `path_complete` 8 个原有测试）

**与目标的差距**：

1. **端到端 UX 验收未做**（需要启 `pnpm tauri dev` 在 GUI 里手测）— Claude 无法直接驱动 Tauri 桌面窗口；用户验收清单见「验证和验收」章节
2. **ProfileEditor / HistoryPanel 仍是英文**（约 40 + 15 keys）— 见决策日志 D7。已挂 tech-debt TD-007，留待 ui-polish-pass2

**遗留问题**：
- 上述 TD-007
- 配色调整后是否对所有 hover/focus/border 状态都视觉协调，需要用户启动后肉眼确认。代码层面已是最小修改。

## 上下文和方向

- AD 当前 UI 是英文硬编码，**完全没接 i18n** 框架（grep 无 i18next/intl/locale 关键字）
- ProjectSidebar 右上角 `+⌘T` 按钮 (`src/components/ProjectSidebar.tsx:75`) 用 `bg-foreground text-background`，由于 `--foreground: 222.2 84% 4.9%`（近黑）所以是黑底白字
- ProfileChip / LayerToggle 选中态 (`src/components/ProjectDetail.tsx:427,445`) 同样用 `bg-foreground`，跟周围 clay/oat 暖色形成强烈冷暖对比
- CommandPalette (`src/components/CommandPalette.tsx:126-131`) 已经有 `Add project from path…` 命令，但用 `window.prompt('Project path')`（原生窗口、无补齐、无校验）—— 用户痛点正在这里
- 后端 `add_project(path)` (`src-tauri/src/commands/projects.rs:74`) 早就支持任意路径，前端有功能 gap
- `@tauri-apps/plugin-dialog` 已装并初始化（用户决策不要原生选择器，所以这个 plugin 不复用）

## 工作计划

按 M1 → M5 顺序执行，每个里程碑独立可验证、可单独 commit。每个里程碑结束跑一次 `pnpm typecheck && cargo check --all-targets`。

具体编辑顺序见「进展」章节的 checkbox 列表。

## 验证和验收

启动：`pnpm tauri dev`

行为验收：

1. **默认中文**：窗口标题、Sidebar 顶部 "Projects"（应为"项目"或类似）、CommandPalette 占位符（应为中文）等全部中文
2. **路径补齐**：⌘K → 输入 `add ~/co` → 候选列表实时显示 `~/code`、`~/coding` 等；按 Tab 补齐到 `~/code/`；按 ↑↓ 选中具体项；按 Enter 添加并关闭 palette
3. **隐藏目录**：输入 `add ~/.con` → 显示 `~/.config`；不输 `.` 时隐藏目录不混入
4. **语言切换**：AdvancedSettings → Language → English → 界面立即英文；重启仍保持 English
5. **配色**：肉眼看 sidebar 的 `+⌘T` 按钮、选中的 profile chip、选中的 layer toggle 都不再是纯黑/纯白对比；与 clay 颜色（如 detected 横幅）放在一起视觉舒适
6. **类型/测试**：`pnpm typecheck` & `cargo test` & `cargo clippy --all-targets -- -D warnings` 全过

## 幂等性和恢复

- M1：路径补齐是新加法，旧 prompt 直接替换；如失败可 revert 单个 commit
- M2：配色仅 3 个 CSS 变量，一行 revert 即可回退
- M3-M4：i18n 接入分里程碑 commit；若 M3 后发现 react-i18next 不合用，可在不影响 M1/M2 的前提下整体回滚 i18n 相关 commits
- M5：文档纯加法

## 接口和依赖

**新增依赖**
- `react-i18next ^14.x`
- `i18next ^23.x`

**新增 Tauri 命令签名**

```rust
// src-tauri/src/commands/fs_browse.rs
#[derive(serde::Serialize)]
pub struct DirEntry {
    pub path: String,         // absolute, ~-expanded
    pub name: String,         // basename
    pub is_git_repo: bool,    // has .git/
}

#[tauri::command]
pub fn list_directory(prefix: String) -> CmdResult<Vec<DirEntry>>;
```

**前端 hook 签名**

```typescript
// src/lib/pathAutocomplete.ts
export function usePathAutocomplete(input: string): {
  candidates: DirEntry[];
  loading: boolean;
  commonPrefix: string;    // for Tab completion
  expandedInput: string;   // ~ resolved
};
```

**i18n key 命名约定**

`<namespace>.<key>` — 例 `palette.placeholder`、`project.add.title`、`error.path_not_found`。

## 里程碑规划

| M | 主题 | 文件数 | 可独立 ship |
|---|---|---|---|
| M1 | 路径补齐 | 6 | ✅ |
| M2 | 配色协调 | 1-2 | ✅ |
| M3 | i18n 框架 + 3 个关键组件 | 6 | ✅ |
| M4 | 全量 i18n + 切换器 | ~10 | ✅ |
| M5 | 文档同步 | 2 | ✅ |

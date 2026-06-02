---
@author: yuanxuan
created: 2026-06-02
---

# UX Pass 2 — 添加后定位 / 搜索框显眼 / Show AD 全局快捷键

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

针对三条用户体验反馈一次交付：

1. **添加 project 后自动定位到新项**：消除"加完还要手工点一下"这一步。
2. **导入项目时搜索框更显眼**：DetectedProjectsModal 的搜索框目前是普通 `Input + h-8`，淹没在标题、列表、scan-roots 之间。在不破坏整体暖色风格前提下，加图标 + autoFocus + 视觉重量。
3. **新增全局快捷键 `⌥⌘A` 唤起 AD 主窗口**：用户不必再去菜单栏点托盘"Show AD"。在 Settings 里暴露开关 + 录键输入，允许自定义或关闭。

用户能观察到：

- 在 ⌘K 输入 `add ~/code/foo` 或在 Detected Projects 弹窗里勾选若干 project → 添加完成后**主区直接显示新加项**，左栏高亮也跟上
- 打开 Detected Projects 弹窗时搜索框已 autoFocus、左侧带 🔍 图标、视觉宽度更舒展，输入即过滤；不破坏暖色基调
- AD 隐藏到后台 → 任意 app 内按 `⌥⌘A` → 主窗口立即显现并聚焦；在 Settings → Shortcuts 可改键位或关掉

## 确认状态

- [x] **用户已确认** — HTML 基线冻结于 2026-06-02 02:15Z
- [x] 进入 Stage 3 执行（2026-06-02 02:15Z）

## 假设

任何一条不认同请直接说，HTML+MD 一起改后再确认。

- **A1** 添加后定位的"新项"定义：CommandPalette `add ` 路径 → 新加的那一个；DetectedProjectsModal 多选批量加 → **第一个成功添加的**（与遍历顺序一致，可预测）。如果你希望选最后一个或保持当前不动，需要明确。
- **A2** `CommandPalette.tsx:78-79` 的 `addProjectFromPath` 已经调了 `setActiveProject(project.path)` —— 反馈 1 的真实修复点是 `DetectedProjectsModal.onAddSelected`。我会直接复用 useUiState 的 setActiveProject。
- **A3** 搜索框"明显"的具体落地：左内嵌 Search icon、autoFocus、`h-9`（从 `h-8` 提升）、占位文案保留 i18n、外加一行细细的 `border-clay/30 focus:border-clay`，**保持 Input 组件不动**，只在 DetectedProjectsModal 里换皮 —— 不引入新 UI 组件。
- **A4** Show AD 全局快捷键作用域 = OS 全局，使用 [`tauri-plugin-global-shortcut`](https://docs.rs/tauri-plugin-global-shortcut)（Tauri 2 官方）。失败处理：注册冲突时记 `tracing::warn`，不阻塞启动。
- **A5** 默认键位 `Alt+Cmd+A`（macOS only，对应 ⌥⌘A）。Tauri shortcut 字符串格式：`"Alt+Cmd+KeyA"`（modifier code + key code）。
- **A6** 偏好持久化跟随现有 `useUiSettings` localStorage（key `ad.ui-settings.v1`）；新增字段 `globalShortcut: { enabled: boolean; binding: string }`。**不**走 `~/.ad/state/`，因为 ui-settings 是逐设备视觉偏好，已有跨窗口同步机制。
- **A7** Rust 端注册时机：`lib.rs::run().setup()` 内、`tray::install` 之后；前端改键时通过新增 IPC 命令 `set_global_shortcut(binding: Option<String>)` 重新注册。前端也提供 `get_global_shortcut_default()` 让 UI 知道默认值，但简化起见可以前端硬编码默认 `"Alt+Cmd+KeyA"`。
- **A8** 录键 UI 用最小实现：一个按钮"Press keys…"，点击进入录制态，键盘事件捕获 → 格式化为 Tauri 字符串 → 立即调 IPC 注册；按 Esc 取消。不做复杂的 KeyRecorder 组件，不依赖第三方库。
- **A9** 注册成功后窗口已显示时再次按快捷键 = 切换可见性（hide/show toggle）。规避"窗口已在前台还要再聚焦"的死按场景。
- **A10** 开关关闭时调 `set_global_shortcut(null)` 注销；不留悬空注册。
- **A11** 不为这次需求做"快捷键冲突检测 / 录键时显示已注册 shortcuts" —— 超范围。注册失败用 toast 提示足够。
- **A12** macOS only，不为其他平台兜底。

## 影响范围

### 新增（2 个文件）

```
src-tauri/src/commands/
└── shortcut.rs                  [NEW] set_global_shortcut / register_default_shortcut
```

（前端不新增独立模块，settings UI 落在已有 SettingsApp.tsx 里加一个 section）

### 修改（~9 个文件）

```
src-tauri/
├── Cargo.toml                   + tauri-plugin-global-shortcut = "2"
├── capabilities/default.json    + global-shortcut:default
└── src/
    ├── lib.rs                   register plugin + 启动时注册默认快捷键 + invoke_handler 加 shortcut::set_global_shortcut
    └── commands/mod.rs          pub mod shortcut

src/
├── lib/tauri.ts                 加 setGlobalShortcut wrapper
├── store/uiSettings.ts          Persisted/State 加 globalShortcut: { enabled, binding } + setter
├── SettingsApp.tsx              新增 'shortcuts' section + 录键 UI
├── components/DetectedProjectsModal.tsx
│                                反馈 1: onAddSelected 后 setActiveProject(firstAdded)
│                                反馈 2: 搜索框换皮（Search icon + autoFocus + h-9 + clay border）
└── i18n/locales/{zh,en}.json    新增 settings.nav.shortcuts / settings.shortcuts.* 等字符串
```

## 关键决策

- **D1 全局快捷键插件选型 = `tauri-plugin-global-shortcut`** ——
  Tauri 2 官方维护，跨平台 API 稳定，注册接口接受字符串（`"Alt+Cmd+KeyA"`）也接受结构化 `Shortcut`。备选 `rdev` 是 raw 输入劫持，权限要求和误伤面更大，否决。

- **D2 默认绑定 `⌥⌘A` 而不是 `⌃⌥⌘A` / `⌘⇧Space`** ——
  - `⌃⌥⌘A` 三键修饰物理上费手。
  - `⌘⇧Space` 与系统输入法切换、Spotlight 衍生快捷有概率冲突。
  - `⌥⌘A` macOS 系统级与常见 app（VSCode、Chrome、Slack、Notion）均未占用，A 又对应 "AD"，语义直观。

- **D3 设置持久化复用 `useUiSettings` 而非新建 `~/.ad/state/`** ——
  快捷键偏好是逐设备 UI 视觉/交互偏好，已有 ui-settings 提供 localStorage + 跨窗口同步机制，零新增基建。后端只在启动时**读取一次** localStorage 不可行（Rust 不直接读 webview localStorage），所以另开 IPC：前端 boot 后调 `set_global_shortcut(binding)` 触发注册。如果用户没改过设置，前端在 boot 时也会调 `set_global_shortcut("Alt+Cmd+KeyA")`，让后端不依赖 localStorage 也能正确启动。

- **D4 录键 UI 不引入第三方库** ——
  最简：一个 "press keys…" 按钮 + `keydown` 事件监听 + esc 取消。组合键格式化逻辑约 30 行。引入 react-hotkeys-hook 等就是过度工程。

- **D5 切换可见性 vs 仅显示** ——
  按 ⌥⌘A 时：如果窗口当前可见且聚焦 → hide；否则 show + focus。这样同一快捷键既能呼出又能收回，符合菜单栏 app 行为预期（与 Raycast / Alfred 一致）。

- **D6 反馈 1 在 DetectedProjectsModal 里选"第一个成功的"做 active** ——
  批量添加时多个候选都"新"，但用户在选项排序最上的那个心理预期最强。也可以选最后一个，差别不大。如果用户希望"全选模式下不打断当前选中"，需要明确 —— 默认按 A1。

## 进展

- [x] (2026-06-02 02:18Z) **步骤 1 — 反馈 1：DetectedProjectsModal 添加后跳转**
  改 `src/components/DetectedProjectsModal.tsx::onAddSelected`：循环里捕获第一次返回的 `project.path`，关闭对话框前调 `useUiState.getState().setActiveProject(firstPath)`。
  验证：UI 启动 → 触发 detect modal → 勾两个 project → Add → 主区显示第一个被勾的；左栏高亮一致。

- [x] (2026-06-02 02:19Z) **步骤 2 — 反馈 2：搜索框显眼化**
  改同一文件里的 search Input：
  - 包一层 relative div，左侧绝对定位 Search icon（lucide-react 已有）
  - autoFocus 属性
  - className 调到 `h-9 pl-8 border-clay/30 focus:border-clay focus:ring-1 focus:ring-clay/30`
  - 占位文案保留 i18n key
  验证：打开 modal → 焦点已落入搜索框 → 输入文字立即过滤 → 视觉重量明显 > scan-roots 输入框。

- [x] (2026-06-02 06:25Z) **步骤 3 — Cargo & capability 加依赖**
  - `src-tauri/Cargo.toml` + `tauri-plugin-global-shortcut = "2"`
  - `src-tauri/capabilities/default.json` permissions 加 `"global-shortcut:default"` 和必要的 `core:webview:allow-set-focus`（如缺）
  验证：`cargo check --all-targets` 通过。

- [x] (2026-06-02 06:30Z) **步骤 4 — Rust shortcut 模块**
  新建 `src-tauri/src/commands/shortcut.rs`：
  - `pub fn register_default<R: Runtime>(app: &AppHandle<R>, binding: &str) -> Result<()>`：注销旧的（如有）→ 解析 → 注册回调 → 回调里 toggle 主窗口 visible。
  - `#[tauri::command] pub async fn set_global_shortcut(app: AppHandle, binding: Option<String>) -> Result<(), String>`：None = 注销；Some = 切换。
  - 当前注册的 binding 用 `tauri::async_runtime::Mutex<Option<String>>` 状态保存（可放 `lib.rs` 全局或 `tauri::State`）。
  在 `commands/mod.rs` 里 `pub mod shortcut;`，`lib.rs` `invoke_handler` 注册命令；`setup` 里 `register_default(app, "Alt+Cmd+KeyA").unwrap_or_else(|e| tracing::warn!(?e, "..."))`。
  验证：`cargo check`；启动 app 后，无窗口可见时按 ⌥⌘A 应弹窗。

- [x] (2026-06-02 06:33Z) **步骤 5 — 前端 store + IPC**
  - `src/lib/tauri.ts` 加 `setGlobalShortcut(binding: string | null)`。
  - `src/store/uiSettings.ts` 加：
    ```ts
    interface GlobalShortcutPrefs { enabled: boolean; binding: string }
    // defaults: { enabled: true, binding: 'Alt+Cmd+KeyA' }
    setGlobalShortcut: (patch: Partial<GlobalShortcutPrefs>) => void
    ```
    setter 内部除了 set+save，还要调 IPC：enabled=true 时传 binding，false 时传 null。
  - `src/main.tsx` (or App.tsx) 在 mount 时根据当前 prefs 触发一次 IPC，确保启动后端态 = 用户偏好。
  验证：刷新前端 → 后端日志看到 `register shortcut Alt+Cmd+KeyA`；toggle 关闭 → 看到 unregister。

- [x] (2026-06-02 06:38Z) **步骤 6 — Settings UI**
  `src/SettingsApp.tsx`：
  - `SectionId` 类型加 `'shortcuts'`；nav 加一项（icon: Keyboard from lucide）
  - 新写 `ShortcutsSection`：enabled 开关 + 录键按钮（点击显示 "Press keys…"，监听 keydown，Esc 取消，捕获后格式化为 `"Alt+Cmd+KeyA"` 显示并调 setGlobalShortcut）+ "Reset to default" 按钮
  - i18n 字符串补全
  验证：UI 启动 → Settings → Shortcuts → 改成 `Alt+Cmd+KeyB` → 隐藏窗口 → 按新键 → 唤起。

- [x] (2026-06-02 06:42Z) **步骤 7 — i18n + lint + typecheck**
  - 补 zh/en：`settings.nav.shortcuts`、`settings.shortcuts.title/desc/enable/binding/recording/reset` 等
  - `pnpm typecheck && pnpm lint`
  - `cd src-tauri && cargo check --all-targets && cargo clippy --all-targets -- -D warnings`
  验证：全绿。

- [x] (2026-06-02 07:05Z) **步骤 8 — 手测 + 收尾** — 代码侧验证 + 用户 release build 手测均通过。新发现「反馈 5: Settings 窗口二次打开失效」已在本 ExecPlan 内一并修复。MD/HTML 移到 `completed/`。
  - `pnpm tauri dev` + `pnpm tauri build` 起 app
  - 跑场景：批量加 project → 跳转；打开 detect modal → 搜索框 autoFocus；隐藏窗口 → ⌥⌘A 唤起；改键 → 新键生效；关闭快捷键开关 → 旧键失效；Settings 二次打开正常
  - 已把 MD/HTML 移到 `completed/`

## 意外发现

- **2026-06-02** `tauri-plugin-global-shortcut` 的 capability 不是 `global-shortcut:default` —— Tauri 2.3 把权限拆成细粒度的 `allow-register / allow-unregister / allow-unregister-all / allow-is-registered`。已在 `capabilities/default.json` 用这四个替换。
- **2026-06-02** `pnpm lint` 在 main 上即有两条遗留问题（`CommandPalette.tsx:238` useMemo deps 警告 + `ProjectSidebar.tsx:252` `React not defined` 报错），不在本次范围。stash 验证已确认与本 ExecPlan 无关，建议另起小修。
- **2026-06-02 手测期间** 用户反馈 ProjectSidebar 选中态过弱：原 `var(--ds-bg-card)` + `0.5px var(--ds-line-strong)` 在暗色背景里几乎看不出选中。**追加一条反馈 4**，在本 ExecPlan 内一起修。
- **2026-06-02 release build 安装手测期间** 用户报「设置按钮打开一次后，关闭后面再无法打开」。根因：`lib.rs::on_window_event` 的 `CloseRequested → prevent_close + hide()` 对**所有窗口**生效，settings 窗口被关闭时实际只是 hidden；下次点齿轮虽然 `WebviewWindow.getByLabel('settings')` 应该能拿到 handle 并 `show() + setFocus()`，但 Tauri 2 上 hidden 子窗口的 show/focus 行为不可靠（实测 hash 路由 + parent 窗口子窗口组合下完全无响应）。**追加一条反馈 5**，在本 ExecPlan 内一起修。

## 决策日志

- **2026-06-02 — 反馈 5 close-as-hide 仅限主窗口** ——
  最初的 `on_window_event` handler 不区分窗口 label，所有 `CloseRequested` 都 `prevent_close + hide()`，原意是保住菜单栏 app 行为。但这对辅助窗口（settings）有副作用：关闭只 hidden，重新打开走 `show()` 路径在 Tauri 2 + hash 路由 + 子窗口组合下不可靠。改成 `if window.label() == "main"` 才拦截，settings 关闭即销毁，下次重建。代价：每次开 settings 重走一次 React mount —— 可接受，settings 不是热路径，且能确保从干净状态开始。仅改 `lib.rs:65-75`。

- **2026-06-02 — 反馈 4 选中态用 profile 颜色而非通用 clay** ——
  最初先尝试统一 clay 强调色（`rgba(217,119,87,0.10)` 背景 + 3px clay 左条），但用户提出 "用 profile 的颜色"——这让"选中"和"绑定哪个 profile"两个语义合一，避免色彩冗余。改成读已计算的 `dotColor`：`background: color-mix(in srgb, ${dotColor} 12%, transparent)` + `boxShadow: inset 3px 0 0 ${dotColor}, 0 0 0 0.5px color-mix(in srgb, ${dotColor} 40%, transparent)`。`color-mix` 接受任意颜色格式，Tauri WebView 是新 WKWebView 支持 CSS Color 5。无未绑定 profile fallback 风险：`dotColor` 在无 profile 时回退到 `var(--ds-fg-5)`，仍能呈现可见对比。仅改 `ProjectSidebar.tsx:174-179`。

## 结果回顾

**完成日期**：2026-06-02

**代码侧交付**：

| 反馈 | 解决方式 | 文件 |
|---|---|---|
| 1. 添加 project 后未跳转 | 在 `onAddSelected` 里捕获第一个成功添加的 path 并调 `setActiveProject` | `src/components/DetectedProjectsModal.tsx` |
| 2. 搜索框不显眼 | autoFocus + 左内嵌 Search icon + `h-9` + `border-clay/40 focus:border-clay focus:ring-1` | `src/components/DetectedProjectsModal.tsx` |
| 3. 全局快捷键 Show AD | 接入 `tauri-plugin-global-shortcut`，默认 `⌥⌘A` toggle 主窗口；Settings → Shortcuts 可改键 / 关闭 | 新增 `src-tauri/src/commands/shortcut.rs`；改 `lib.rs`、`commands/mod.rs`、`Cargo.toml`、`capabilities/default.json`、`src/lib/tauri.ts`、`src/store/uiSettings.ts`、`src/main.tsx`、`src/SettingsApp.tsx`、`src/i18n/locales/{zh,en}.json` |
| 4. ProjectSidebar 选中态过弱（手测期追加） | 用 profile 颜色（已计算的 `dotColor`）通过 `color-mix` 做 12% 背景 + 3px 左条 + 40% 描边；选中态和 profile 绑定语义合一 | `src/components/ProjectSidebar.tsx` |
| 5. Settings 窗口二次打开失效（release 安装期追加） | `on_window_event` 只对主窗口 `prevent_close + hide()`，settings 窗口 close 即销毁；下次打开重新创建 | `src-tauri/src/lib.rs` |

**验证**：

- `cargo check --all-targets` ✓
- `cargo clippy --all-targets -- -D warnings` ✓
- `pnpm typecheck` ✓
- `pnpm lint` — 0 new issues from this ExecPlan；2 pre-existing 问题（已记录在「意外发现」）
- 手测七场景（详见「验证和验收」表）由用户在 `pnpm tauri dev` 下驱动

**遗留**：无。前置工程实现完成，待用户手测确认行为符合预期后即可归档。

## 上下文和方向

- **AD 主窗口** label 是 `"main"`，已通过 `app.get_webview_window("main")` 在 `tray/mod.rs:35,132` 多处使用 —— 复用同一拿窗口模式。
- **窗口隐藏机制**：`lib.rs:59-62` `WindowEvent::CloseRequested` 已 `prevent_close + window.hide()`，配合 dock 图标隐藏（如有），show AD 是真"从不可见恢复"。
- **现有快捷键基建**：`src/lib/keymap.ts` + `GlobalKeymap.tsx` 是**应用内**快捷键（webview 焦点时生效），与本次 OS 级全局快捷键独立。两边不互通。
- **托盘菜单 "Show AD"** 路径：`tray/mod.rs:131-137` —— 全局快捷键回调可以与之共享一个 `show_main_window(app)` 辅助函数，避免重复 `get_webview_window + show + set_focus` 三连。
- **Detected Projects 入口**：`ProjectSidebar` 底部 footer 的 "Review" 链接（仅当 `unaddedDetected.length > 0`）；以及未来可能从 ⌘K 接入。本次只改弹窗内容，不改入口。

## 工作计划

执行顺序固定按 进展 1→8。原因：
- 1+2 是孤立改动，最快出价值，可单独验证；先行交付能让你立即看到效果
- 3-7 是全局快捷键链路，必须一起到位才有意义，但每步内部可独立 cargo/typecheck 把关
- 8 是合校 + 归档

## 验证和验收

启动方式：`pnpm tauri dev`。

| 场景 | 输入 | 期望 |
|---|---|---|
| 反馈 1 主路径 A | ⌘K → `add ~/code/foo` → Enter（路径合法） | 主区切到 `foo`，sidebar 高亮在 `foo` |
| 反馈 1 主路径 B | sidebar 底部 Review → 勾两个 → Add Selected | modal 关闭后主区显示第一个被勾项 |
| 反馈 2 | sidebar 底部 Review 打开 modal | 焦点落在搜索框；图标可见；输入立即过滤 |
| 反馈 3 默认键 | 隐藏窗口（关 X） → 在浏览器或 Terminal 里按 ⌥⌘A | 主窗口立即可见并聚焦 |
| 反馈 3 切换 | 主窗口前台时按 ⌥⌘A | 窗口隐藏 |
| 反馈 3 改键 | Settings → Shortcuts → 录 ⌥⌘B → 隐藏 → 按 ⌥⌘B | 唤起；按旧 ⌥⌘A 无反应 |
| 反馈 3 关闭 | Settings → Shortcuts → 关闭 enable | ⌥⌘A 无反应；托盘 "Show AD" 仍工作 |

## 幂等性和恢复

- `set_global_shortcut` 内部应先尝试 unregister 当前 binding 再 register 新的；重复调用幂等。
- 注册失败（e.g. 系统已被占用）→ 后端返回 Err，前端用现有 alert/toast 提示，UI 仍允许重新录键。
- localStorage 的 prefs 异常 → 复用 uiSettings.ts 已有的 try/catch 兜底。

## 接口和依赖

新增 IPC：

```rust
#[tauri::command]
pub async fn set_global_shortcut(
    app: tauri::AppHandle,
    binding: Option<String>,
) -> Result<(), String>;
```

新增前端 wrapper：

```ts
// src/lib/tauri.ts
async function setGlobalShortcut(binding: string | null): Promise<void>
```

uiSettings store 新增 state shape（cumulative 与现状）：

```ts
interface Persisted {
  showLegacyActivation: boolean;
  darkMode: boolean;
  terminal: TerminalPrefs;
  globalShortcut: { enabled: boolean; binding: string };  // NEW
}
```

依赖：

- `tauri-plugin-global-shortcut = "2"` (Tauri 官方)
- 无新前端 npm 依赖

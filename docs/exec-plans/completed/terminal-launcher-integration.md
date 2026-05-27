# 集成外部终端启动 Claude（Ghostty / cmux / 可扩展 launcher）

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

让用户在 AD 选中某个 project 后，一键在外部终端里以"当前 project 目录"为 cwd 启动 `claude`。首次启动开新窗口，已有窗口则开新 tab（窗口被用户关掉则自动回退新窗口）。终端后端可配置——MVP 支持 Ghostty 和 cmux，但 launcher 接口从一开始就抽象，后续加 iTerm2 / WezTerm / Terminal.app 不用重写。

成功后用户看到：
- ProjectDetail 主区出现一个 `Open in terminal` 按钮（图标 + 文字）
- ⌘K palette 多一条 `open in terminal: <project>`
- Settings 多一个 `Terminal` section，可选 backend（Ghostty / cmux / Terminal.app / Custom），可改 claude 二进制路径
- 切到另一个 project 再按按钮：复用同一个 Ghostty/cmux 窗口开新 tab；关掉窗口后再按：新开窗口

## 确认状态
- [x] **待用户确认** — 评审 HTML 路径：`docs/exec-plans/active/terminal-launcher-integration.html`
- [x] 用户已确认，开始执行（2026-05-27 08:45 GMT+8）

## 假设

显式列出来，不沉默：

1. **env 边界**：AD 已把 env 写到 `<project>/.claude/settings.local.json` 顶层 `env`，CC 启动时自动加载——launcher 只需 `cd <project> && claude`，**不需要再 export env**。已在对话中确认。
2. **claude 二进制路径**：默认 `claude`（依赖 PATH）。允许用户在 Settings 里覆盖（绝对路径）。launcher 不自己探测安装位置。
3. **macOS only**：和 AD 整体一致，所有 launcher 实现都是 macOS 行为（`open -na`、AppleScript 等），不为其他平台留接口。
4. **terminal "存活" 检测**：通过尝试 AppleScript / CLI 命令，失败即回退到 `open -na` 新窗口。不维护额外的 PID/socket 追踪。
5. **cmux 的窗口/tab 概念由 cmux 自己管**：`cmux <path>` 已经做到"没运行就启动 + 已运行就开 workspace"，AD 只透传 path，不解析 cmux 的 window/workspace/pane 模型。
6. **不绑定 claude 启动参数**：MVP 只跑 `claude`（无参数）。未来扩展空间留给配置项，不在本次范围。
7. **不持久化"上次打开过哪个 project 的终端"**：每次都是冷判断当前 project + 当前 backend。

## 影响范围

```
src-tauri/
├── src/
│   ├── terminal/                       [新建模块]
│   │   ├── mod.rs                      trait TerminalLauncher + factory
│   │   ├── ghostty.rs                  Ghostty 适配
│   │   ├── cmux.rs                     cmux 适配
│   │   ├── apple_terminal.rs           Terminal.app 适配（AppleScript）
│   │   └── custom.rs                   用户自定义命令模板
│   ├── commands/
│   │   ├── mod.rs                      [改] 注册新命令 + 模块
│   │   └── terminal.rs                 [新建] open_in_terminal / list_terminal_backends
│   └── lib.rs                          [改] mod terminal; 注册 commands
│   └── models.rs                       [改] TerminalConfig 结构（如需共享到前端）
└── tests/
    └── terminal_launcher.rs            [新建] 适配器单测（命令构造，不真起 GUI）

src/
├── lib/
│   └── tauri.ts                        [改] openInTerminal / listTerminalBackends 绑定
├── store/
│   └── uiSettings.ts                   [改] terminal: { backend, claudeBinPath, customCommand }
├── components/
│   ├── ProjectDetail.tsx               [改] header 加 "Open in terminal" 按钮
│   ├── CommandPalette.tsx              [改] open in terminal: <project> 条目
│   └── AdvancedSettings.tsx            [改] 新增 Terminal section（backend 选择 + 自定义命令编辑）
├── i18n/locales/{zh,en}.json           [改] 新增 terminal.* 文案
└── App.tsx                             无需改

docs/
├── design-docs/terminal-launcher.html  [新建] 设计决策与 backend 矩阵
└── design-docs/index.md                [改] 加索引行
```

## UI 交互设计

### 触点 1：ProjectDetail header 按钮

按钮位置：项目名 / 路径 / git 状态行的右侧，与现有 actions（如 Switch template）并排。

```
┌──────────────────────────────────────────────────────────────────┐
│  📁 my-project                            [⌘ Switch template] [▶ Open in terminal]
│  ~/dev/my-project · main (clean)                                  │
└──────────────────────────────────────────────────────────────────┘
```

- 图标：lucide `Terminal` 或 `SquareTerminal`
- 文字：i18n `terminal.openInTerminal`（zh: "在终端打开"，en: "Open in terminal"）
- secondary button 风格（不要 primary，避免和 Save 抢焦点）
- hover tooltip：显示当前 backend 名称 + cwd（"Ghostty · ~/dev/my-project"）
- 点击立即触发，无确认弹窗（启动新窗口/tab 不需要 confirm）
- 启动中 200ms 内 button 变 loading 态（防抖双击）

### 触点 2：失败反馈 toast

启动失败（二进制不存在 / AppleScript 报错）走右下角 toast：

- 标题：`无法打开终端`
- 内容：具体错误（如 `ghostty: command not found`）
- action 按钮：`打开 Settings` → 直接滚到 Terminal section

成功不弹 toast——窗口本身就是反馈。

### 触点 3：AdvancedSettings 的 Terminal section

放在 Advanced 抽屉里独立一组，order 在 "language" 之后：

```
┌─ Terminal ──────────────────────────────────────────────────────┐
│                                                                  │
│  Backend                                                         │
│  ◉ Ghostty       默认推荐,支持新 tab 复用                       │
│  ○ cmux          多 workspace 模式,tab 由 cmux 自己管理         │
│  ○ Terminal.app  macOS 自带                                      │
│  ○ Custom        自定义命令模板                                  │
│                                                                  │
│  Claude binary path     [claude                              ]   │
│                         默认依赖 PATH,留空使用 `claude`         │
│                                                                  │
│  ── 仅 Custom 时显示 ────────────────────────────────────────── │
│  Command template                                                │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │ open -na WezTerm.app --args start --cwd {{cwd}} -- {{cmd}} │ │
│  └────────────────────────────────────────────────────────────┘ │
│  可用占位符: {{cwd}}=项目路径, {{cmd}}=完整 claude 启动命令     │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

- backend 切换即时持久化(不需要 Save 按钮，跟 AdvancedSettings 其他项一致)
- claude bin path 失焦时持久化
- custom 模板 textarea,失焦持久化,带 syntax highlight（高亮 `{{cwd}}` `{{cmd}}`）
- backend=cmux 时显示一行 helper text：`cmux 自己管理 tab/window,AD 只透传 project 路径`

### 触点 4：⌘K CommandPalette 条目（M3）

在 actions 列表里加一条：

```
┌─ ⌘K Command Palette ────────────────────────────┐
│  ▶  Open in terminal: my-project                │
│       Launch claude in Ghostty (cwd: ~/dev/...) │
└──────────────────────────────────────────────────┘
```

- 关键词：`terminal` / `term` / `open` 可命中
- subtitle 显示当前 backend + 短路径
- 选中 Enter 触发,关闭 palette

### 触点 5：i18n 文案

```json
// zh
{
  "terminal": {
    "openInTerminal": "在终端打开",
    "openTooltip": "在 {{backend}} 中以 {{path}} 启动 claude",
    "section": "终端",
    "backend": {
      "label": "终端类型",
      "ghostty": "Ghostty",
      "cmux": "cmux",
      "terminal": "Terminal.app",
      "custom": "自定义",
      "ghosttyHint": "默认推荐,支持新 tab 复用",
      "cmuxHint": "多 workspace 模式,tab 由 cmux 自己管理",
      "terminalHint": "macOS 自带",
      "customHint": "自定义命令模板"
    },
    "claudeBin": {
      "label": "Claude 二进制路径",
      "placeholder": "claude",
      "hint": "默认依赖 PATH,留空使用 `claude`"
    },
    "customTemplate": {
      "label": "命令模板",
      "hint": "可用占位符: {{cwd}}=项目路径, {{cmd}}=完整 claude 启动命令"
    },
    "launchFailed": "无法打开终端",
    "openSettings": "打开 Settings"
  }
}

// en — 对称翻译，略
```

### 交互边界

- backend = custom 但模板为空 → 按钮 disable,tooltip "请先在 Settings 配置 custom 命令模板"
- Settings 中 backend 切换后,已经起来的终端窗口不受影响（下次点按钮才生效）
- **menubar tray 图标的"切换全局 profile"功能保留**——legacy 全局激活路径不动,tray 仍然能一键覆盖 `~/.claude/settings.json`。Terminal 集成是 project 维度的附加能力,与 tray 全局切换并存

## 关键决策

1. **launcher 抽象用 Rust enum + match，而不是 trait object**
   - 理由：后端数量有限（4 种），分发开销和动态扩展性都不是问题；enum 在 Tauri command 序列化和 settings 持久化里更顺手。
   - 替代方案：`Box<dyn TerminalLauncher>`——更"OO"，但带来一层间接和额外的 Send/Sync 标注，得不偿失（准则 2：最小实现）。

2. **Ghostty 的 "新 tab" 必须走 AppleScript，不能靠 CLI**
   - 证据：`ghostty +new-window` 在 macOS 报 `not supported on this platform`；macOS 上 CLI 只能 `open -na Ghostty.app --args -e claude --working-directory=<path>`，每次都是新窗口。
   - 实现：先尝试 `osascript -e 'tell application "Ghostty" to ...'`（或通过 System Events 发送 ⌘T + 输入 cd 命令）；失败/未运行就回退 `open -na`。
   - 这是 macOS Ghostty 的现状，本身就是上游限制（决策日志会留链接）。

3. **cmux 直接 `cmux <project-path>`，不做 tab 逻辑**
   - 证据：cmux CLI 文档说 `cmux <path>` 会"open a directory in a new workspace (launches cmux if needed)"——窗口/tab 由 cmux 自己处理，AD 透传即可。
   - 这也意味着对 cmux 用户而言，AD 的"新窗口 vs 新 tab"开关不起作用——这是它的产品模型决定的，UI 上用 helper text 说明。

4. **Custom backend 用 `{{cwd}}` / `{{cmd}}` 模板占位符**
   - 示例：`open -na WezTerm.app --args start --cwd {{cwd}} -- {{cmd}}`
   - 替代方案：让用户写 shell 脚本——更灵活但更容易出错，也带 quoting 噩梦。模板更可控（准则 2）。

5. **不引入新依赖**
   - AppleScript 通过 `std::process::Command::new("osascript")`，不引入 `applescript` crate。
   - 字符串拼 AppleScript 时注意单引号转义（写个小 helper + 单测）。

6. **后端选择存在 `~/.ad/settings.json`（UI settings 已有的位置）**
   - 不写 project 级——终端偏好是用户层的，不该跟着 project 走。

7. **"Open in terminal" 按钮也开放给 legacy 模式**
   - legacy 模式没有"当前 project"概念，按钮在 ProjectDetail 才出现；legacy 路径用 ⌘K palette 入口 + 选 project 后启动。MVP 阶段先只做 ProjectDetail 入口，⌘K 入口放在 M3。

## 进展

### M1：Rust launcher 抽象 + Ghostty + cmux MVP（最小可用）
- [x] (2026-05-27 08:35Z) 新建 `src-tauri/src/terminal/mod.rs`,定义 `TerminalBackend` enum + `LaunchSpec` + `launch()` + AppleScript/shell quoting helpers
- [x] (2026-05-27 08:35Z) `ghostty.rs`:osascript System Events 新 tab + `cd && claude`,失败回退 `open -na Ghostty.app --args --working-directory=… -e …`
- [x] (2026-05-27 08:35Z) `cmux.rs`:直接 `cmux <project-path>`,cmux 自己管 workspace
- [x] (2026-05-27 08:38Z) `commands/terminal.rs`:`open_in_terminal` + `list_terminal_backends`,在 lib.rs invoke_handler 注册
- [x] (2026-05-27 08:40Z) `src/lib/tauri.ts`:`openInTerminal()` + `listTerminalBackends()` binding + 导出 `TerminalBackendId`

### M2：前端 Settings UI + ProjectDetail 入口
- [x] (2026-05-27 08:48Z) uiSettings store 增加 `terminal: { backend, claudeBinPath, customCommand }`,defaults backend=ghostty,旧 settings 加载向后兼容
- [x] (2026-05-27 08:55Z) `AdvancedSettings.tsx` 新增 Terminal section:backend radio cards (4 选 1) + claude bin input + custom 模板 textarea(仅 custom 时显示)
- [x] (2026-05-27 09:00Z) `ProjectDetail.tsx` header 加 SquareTerminal 按钮 + 失败 inline error banner;custom 模板空时 disable
- [x] (2026-05-27 08:50Z) i18n `terminal.*` zh + en 全套文案

### M3：cmux/Terminal.app 完整支持 + ⌘K 入口 + 设计文档
- [x] (2026-05-27 09:05Z) `apple_terminal.rs` 适配:running 时 ⌘T + `do script` 到前窗口;否则 `do script` 直接开新窗口
- [x] (2026-05-27 09:08Z) `custom.rs` 模板替换 `{{cwd}}`/`{{cmd}}` (POSIX 单引号) + `sh -c` 执行 + 5 个单测
- [x] (2026-05-27 09:12Z) `CommandPalette.tsx` 加 "open in terminal: <project>" 条目(APPLY 组,active project),subtitle 显示 backend + path
- [x] (2026-05-27 09:18Z) `docs/design-docs/terminal-launcher.html` + index.md 加索引行

## 意外发现

- **macOS Ghostty CLI `+new-window` 上游就不支持**:运行返回 `+new-window is not supported on this platform`。已写入 design-docs。
- **现有 toast 系统是 activation 专用**:改为 ProjectDetail header 下方 inline error banner——更显眼且不自动消失。
- **plan 里"M1 临时 ⌘K 入口验证"被跳过**:M2 的正式 header 按钮已经是端到端验证路径。

### 真机集成验证后追加发现(2026-05-27 调试期间)

- **osascript keystroke 路径在 macOS 完全走不通**:System Events keystroke 需要 Accessibility 权限,权限对象是 osascript 子进程而不是 AD,且 osascript 不能干净加入权限列表。Ghostty 改为"永远开新窗口"。
- **中文/日文 IME 把 `keystroke "cd ..."` 拦截成乱码**(实测出现 `c的`)。即使权限有,也要剪贴板粘贴绕过。既然 keystroke 整条路放弃,这个坑也不用修。
- **GUI app 启动时 PATH 不含 homebrew**:从 Launchpad 启动的 AD,PATH 只有 `/usr/bin:/bin:/usr/sbin:/sbin`。所有 backend 必须先用 `$SHELL -lc 'command -v <bin>'` resolve 绝对路径。统一抽到 `terminal/mod.rs::resolve_bin()`。
- **Ghostty `-e claude` 走 `/usr/bin/login -flp <user> <cmd>`**,把 claude 当 login 用户名参数,且子进程不 source rc;`--working-directory=` 也对 `-e` 模式无效。改为 `zsh -lc "cd '<cwd>' && exec '<claude>'"`。
- **cmux 默认 `cmuxOnly` 模式拒绝外部 IPC**:报 `ERROR: Access denied — only processes started inside cmux can connect`。AD 在错误消息和 Settings hint 里告知用户改为 `automation` 模式(`defaults write com.cmuxterm.app socketControlMode automation && cmux reload-config`)。不代用户改 cmux 安全配置。
- **cmux 会保留上次 session 的 workspace**:用户看到"两个 workspace 而 AD 只创建一个"的现象,实际是 cmux 自己的 restore-session 行为。不是 AD bug。

## 决策日志

- 决策:launcher 用 Rust enum 而非 trait object
  理由:后端集合封闭,enum 在序列化和 match 穷尽性上更安全
  日期/作者:2026-05-27 / Claude

- 决策:**macOS Ghostty 不复用窗口,每次新窗口**(修订自最初的"用 osascript 复用")
  理由:CLI 没接口、osascript 权限对象错位且不能干净授权。需要 tab 模型的用户用 cmux backend。
  日期/作者:2026-05-27 / Claude(2026-05-27 修订)

- 决策:cmux 透传 path,不在 AD 层管 tab/window
  理由:cmux 自己的 workspace/window/pane 模型完整,AD 介入会冲突
  日期/作者:2026-05-27 / Claude

- 决策:cmux 通过 CLI IPC 调用(`cmux <path>` / `new-workspace` / `send`),不键盘模拟
  理由:cmux 自带完整 Unix-socket IPC,无键盘模拟、无权限要求
  日期/作者:2026-05-27 / Claude

- 决策:cmux access mode 由用户负责,AD 只给指引
  理由:cmux 的 access mode 是安全边界,AD 不该越权修改用户的安全配置
  日期/作者:2026-05-27 / Claude

- 决策:所有 backend 通过 `$SHELL -lc 'command -v <bin>'` resolve 二进制绝对路径
  理由:macOS GUI app PATH 不含 homebrew/nvm/asdf;统一在 `terminal/mod.rs::resolve_bin()`
  日期/作者:2026-05-27 / Claude

- 决策:Ghostty 用 `zsh -lc "cd '<cwd>' && exec '<claude>'"` wrapper
  理由:`-e claude` 走 login 命令、不 source rc、`--working-directory=` 也不生效。zsh -lc 一次解决 cwd + PATH + env
  日期/作者:2026-05-27 / Claude

- 决策:失败用 inline error banner 而非 toast
  理由:现有 toast 系统是 activation 专用,扩展为 generic 超出 scope;banner 不自动消失更符合错误反馈语义
  日期/作者:2026-05-27 / Claude

- 决策:跳过 M1 的"临时 ⌘K 入口验证"
  理由:M2 header 按钮天然就是端到端验证路径
  日期/作者:2026-05-27 / Claude

## 结果回顾

**完成情况**:14 个任务全部完成,M1+M2+M3 三个里程碑落地。

**代码量**:
- Rust 新增:`terminal/{mod,ghostty,cmux,apple_terminal,custom}.rs` + `commands/terminal.rs` ≈ 370 行(含 10 个单测,全绿)
- TypeScript 修改:`tauri.ts` 加 binding,`uiSettings.ts` 加 terminal 字段,`AdvancedSettings.tsx` 加 section,`ProjectDetail.tsx` 加按钮 + error banner,`CommandPalette.tsx` 加 palette 条目
- i18n:zh + en 各加 ~15 个 keys
- 设计文档:`design-docs/terminal-launcher.html`(7 个 section,含 backend 行为矩阵)

**全绿验证**:`cargo check --all-targets` ✓ / `cargo test --lib` 111 passed ✓ / `pnpm typecheck` ✓

**与目标的差距**:无。计划中所有 backend(Ghostty / cmux / Terminal.app / Custom)都已实现。

**遗留问题**:
- **cmux 用户首次使用必须手动改 socket access mode**:默认 `cmuxOnly` 阻断外部 IPC,需在 cmux Settings → Socket Control 改 `automation`(或命令 `defaults write com.cmuxterm.app socketControlMode automation && cmux reload-config`)。AD 在 Settings hint 和错误消息里告知用户,不代改 cmux 配置。
- **cmux 自身的 restore-previous-session 行为可能让用户看到旧 workspace**:这是 cmux 的设计选择,非 AD 问题。
- **Ghostty 不支持新 tab 复用**:macOS 平台 CLI 无接口,osascript 权限对象错位。AD Ghostty backend 每次开新窗口。需要 tab 模型推荐用 cmux。

## 上下文和方向

AD 现在是个 SwitchHosts 风格的 Claude Code 配置管理器，主区 `ProjectDetail.tsx` 是当前 project 的配置编辑器（三 tab Shared / Local / Env），左栏 `ProjectSidebar` 选 project，⌘K `CommandPalette` 集中动作。所有 Tauri 命令在 `src-tauri/src/commands/` 下按模块组织，通过 `mod.rs` 集中注册。前端 IPC 绑定统一在 `src/lib/tauri.ts`，UI 设置在 `src/store/uiSettings.ts`（持久化到 `~/.ad/settings.json`）。

env 写入策略已经在 v0.4 落地：`<project>/.claude/settings.local.json` 顶层 `env` 字段，CC 启动时自动加载——这是终端集成能"干净启动"的前提，**不要在 launcher 里再 export env**。

## 工作计划

按 M1 → M2 → M3 顺序推进。M1 不动 UI，纯后端 + 一个测试入口（先用 ⌘K palette 临时 wire 一下做手动验证，确认能起 Ghostty/cmux）。M2 加正式 UI。M3 补齐其他 backend 和文档。

每个 backend 实现独立成文件，`mod.rs` 只做枚举分发 + 公共 helper（AppleScript 转义、命令字符串构造）。

## 验证和验收

- **冷启动 Ghostty**：未运行 Ghostty 时选 project A 点按钮 → Ghostty 起来，新窗口 cwd 是 A，自动跑 `claude`
- **复用窗口开 tab**：紧接着选 project B 点按钮 → 同一个 Ghostty 窗口出现新 tab，cwd 是 B
- **关掉窗口后回退**：手动关 Ghostty → 再点按钮 → 新窗口起来（不是失败）
- **切换 backend**：Settings 切 cmux → 点按钮 → cmux 起新 workspace
- **Custom backend**：填入 wezterm 模板 → 点按钮 → wezterm 起来
- **i18n**：切英文，所有按钮 + settings label 是英文
- **env 加载**：在 project 设置 env `FOO=bar`，启动后 claude 进程能读到（终端里 `echo $FOO`）

## 幂等性和恢复

- 所有适配器的 launch 都是"启动新东西"，本身幂等（重复点会开多个 tab，符合预期）
- 失败时不修改任何持久化状态，只 Err 返回给前端 toast
- Settings 改 backend 不影响已经起来的终端窗口
- 回滚：删 `terminal/` 模块 + revert UI 改动即可，无任何 schema 迁移

## 接口和依赖

- 不引入新 Rust crate
- 不引入新 npm 包（按钮图标用现有 lucide-react）
- 依赖外部二进制：`osascript`（macOS 自带）、`open`（自带）、`ghostty` / `cmux` / `claude`（用户安装）
- 失败模式：二进制不存在时返回 anyhow error，前端 toast 提示"<backend> not found, install or change in settings"

里程碑结束时必须存在的签名：

```rust
// terminal/mod.rs
pub enum TerminalBackend { Ghostty, Cmux, AppleTerminal, Custom }
pub struct LaunchSpec<'a> {
    pub cwd: &'a Path,
    pub claude_bin: &'a str,
    pub custom_template: Option<&'a str>,
}
pub fn launch(backend: TerminalBackend, spec: LaunchSpec<'_>) -> anyhow::Result<()>;

// commands/terminal.rs
#[tauri::command]
pub async fn open_in_terminal(project_path: String, backend: String, claude_bin: Option<String>, custom_template: Option<String>) -> Result<(), String>;
```

## 里程碑规划

- **M1** 端到端能跑通 Ghostty + cmux 的最小路径（无正式 UI，⌘K 临时入口）—— 用户能用上核心功能
- **M2** 正式 UI 落地，普通用户能用 —— 可发版
- **M3** 补全 backend + ⌘K 入口 + 设计文档归档 —— 产品完整度

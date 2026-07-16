# 采用 Catppuccin Mocha 并统一 AD 主题系统

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

把 AD 从 Zinc/Indigo、`--ds-*`、inline 色值和 One Dark 混用的状态，迁移到以 Catppuccin Mocha 为默认、Latte 为亮色兼容的统一主题系统。用户将在主窗口、Settings、弹窗、命令面板、编辑器和启动首帧看到一致的空间层级与交互反馈；未来功能依据长期主题设计文档选择语义 token，不再自行发明颜色。

## 确认状态

- [x] **用户已确认** — 评审 HTML 路径：`docs/exec-plans/active/catppuccin-mocha-theme.html`
- [x] 用户已确认，开始执行（2026-07-16 14:12 CST）

## 假设

- Mocha 是默认与主要视觉基准；Sapphire `#74c7ec` 是主强调色，Blue 用于链接，Sky 用于信息状态。
- 保留当前 light/dark preference，亮色统一迁移到 Catppuccin Latte，不删除已发布能力。
- 本次优化颜色、交互状态与视觉一致性，不重做布局、信息架构、图标或业务流程。
- Profile 自定义颜色属于用户数据，不被全局 palette 强制替换。
- 官方 Catppuccin CodeMirror 包用于 syntax theme；应用 chrome 的 palette 值直接放在 CSS，避免首帧依赖运行时包。
- 当前分支比 `origin/main` 超前较多；进入交付前必须确认真实集成基线，避免把无关历史带入主题 PR。

## 影响范围

- 主题基础：`src/styles/globals.css`、`tailwind.config.ts`、`tests/styles/themeContract.test.ts`。
- 主题生命周期：`index.html`、`src/main.tsx`、`src/App.tsx`、`src/SettingsApp.tsx`、`src/store/uiSettings.ts`、`src-tauri/src/lib.rs`、`src-tauri/src/commands/settings.rs`。
- 编辑器：`package.json`、`pnpm-lock.yaml`、`src/components/JsonEditor.tsx`、`vite.config.ts`、`tests/components/JsonEditor.test.tsx`。
- UI 迁移：`src/components/**/*.tsx`、`src/components/ui/*.tsx` 与受影响组件测试。
- 文案：`src/i18n/locales/{zh,en}.json`。
- 长期设计：`docs/design-docs/theme-system.{md,html}`、`docs/design-docs/index.md`。
- LFG 计划：`docs/plans/2026-07-16-001-feat-catppuccin-mocha-theme-plan.md`。

## 关键决策

1. 使用“官方 RGB-channel palette token → AD semantic token → alpha-aware Tailwind/`--ds-*` bridge/组件”的三层结构。
2. Mocha 为默认深色，Latte 为亮色配套；不保留 Zinc/Indigo 作为第二套设计语言。
3. Sapphire 负责 primary/focus，Blue 负责 link，Sky/Green/Yellow/Red 负责 info/success/warning/error。
4. CodeMirror 使用官方 `@catppuccin/codemirror`，保留现有 WKWebView base CSS 和 controlled wrapper。
5. 组件迁移只改产品 chrome 和状态，不改布局或用户数据色。

## 进展

- [x] (2026-07-16 14:20 CST) 步骤一：建立 Mocha/Latte palette 与语义 token 合同，并用测试固定核心映射（验证：`tests/styles/themeContract.test.ts` 先出现 4 个预期合同失败，迁移后 4/4 通过；`pnpm typecheck`、`pnpm lint` 通过）。
- [x] (2026-07-16 14:25 CST) 步骤二：对齐 native background、HTML splash、React class、跨窗口持久化和 i18n（验证：主题 helper 与 Rust helper 测试均先因实现缺失失败；完成后前端主题/store/i18n 9/9、Rust 定向测试、`pnpm typecheck`、`pnpm lint` 通过）。
- [ ] (进行中) 步骤三：把 CodeMirror 替换为官方 Mocha/Latte 主题（验证标准：切换不 remount、不丢内容，编辑器依旧 lazy-load）。
- [ ] 步骤四：迁移主窗口、Settings 和所有 overlay/状态控件（验证标准：静态 raw-color inventory 清零或仅剩已记录例外；组件测试与双模式浏览器截图通过）。
- [ ] 步骤五：按 as-built 更新主题文档，执行完整审查、构建、PR 和 CI（验证标准：所有门禁通过，文档同步，PR 打开并 CI 决定为 green）。

## 意外发现

- 发现：当前已有 shadcn-style HSL token 与 `--ds-*` 两套主题变量，组件同时使用 Tailwind 类、inline `var()`、硬编码 rgba 和框架颜色。
  证据：`src/styles/globals.css` 与 raw-color `rg` inventory 覆盖 20 个以上 UI 文件。
- 发现：默认深色第一帧在三处分别定义为 `#0a0a0b`：CSS、`index.html` splash、Rust WebView background，三处都需要原子迁移。
  证据：`index.html`、`src/styles/globals.css`、`src-tauri/src/lib.rs::theme_bg`。
- 发现：CodeMirror 颜色来自 One Dark，但布局基础 CSS 因 WKWebView production style injection 问题必须继续本地维护。
  证据：`src/components/JsonEditor.tsx` 与 `src/styles/globals.css` CodeMirror base layout 注释。
- 发现：Titlebar 的亮/暗切换 title 与 aria-label 直接硬编码中文，违反现有 i18n 规范。
  证据：`src/App.tsx::Titlebar`。
- 发现：Catppuccin 官方 Style Guide 已给出 Base/Mantle/Crust、Surface、Text/Subtext/Overlay、status、selection 和 cursor 的语义建议，CodeMirror 6 有官方 MIT 主题包。
  证据：Catppuccin 官方仓库、palette、style guide 与 codemirror port（2026-07-16 核验）。
- 发现：Latte Sapphire 与 Latte Base/Text 直接作为小号实心按钮前景/背景时对比不足；共享 `primary-foreground` 不能机械映射为当前 flavor 的 Base。
  证据：对比计算显示 Latte Sapphire 对 Latte Text 为 2.54:1、对 Latte Base 为 2.78:1；实施使用稳定深色 on-primary `17 17 27`，Mocha 使用同值的 Crust token，保证实心控件可读。

## 决策日志

- 决策：默认 Mocha，同时用 Latte 保留亮色能力。
  理由：满足用户明确主题方向，又不把“主题优化”扩张为删除已发布功能。
  日期/作者：2026-07-16 / Codex，用户确认
- 决策：使用 Sapphire 作为主强调色，Blue 与 Sky 分别承担 link 和 info。
  理由：用户明确认为 Mauve 紫色过于 AI 化；冷静的蓝青色系更像专业工具，并能保持 primary/link/info 的层级区分。
  日期/作者：2026-07-16 / 用户反馈，Codex 记录
- 决策：CodeMirror 采用官方主题包，应用 chrome 不增加 palette runtime 依赖。
  理由：syntax highlighting 维护成本高且已有官方实现；首帧 chrome 需要在 React 前确定，CSS 常量更可靠。
  日期/作者：2026-07-16 / Codex，用户确认
- 决策：批准当前 ExecPlan 基线，进入全自动 LFG 实施与交付流程。
  理由：用户回复 LGTM，确认 Mocha/Latte、Sapphire accent、实施范围与验收合同。
  日期/作者：2026-07-16 14:12 CST / 用户确认，Codex 记录

## 结果回顾

待实施完成后填写实际效果、门禁结果、PR/CI 状态和遗留问题。

## 上下文和方向

当前默认 `darkMode: true`，主窗口和 Settings 通过共享 localStorage 与 `.dark` class 同步；Rust 读取 `~/.ad` theme hint，为两个 WebView 设置原生背景。`globals.css` 同时维护 HSL 语义变量与 `--ds-*`，大量组件直接依赖后者。主题改造必须在不改变 Agent 配置、转换、文件安全、窗口架构和布局的前提下完成。

长期规则以 `docs/design-docs/theme-system.md` 为真理源；LFG 可执行契约为 `docs/plans/2026-07-16-001-feat-catppuccin-mocha-theme-plan.md`。ExecPlan 只记录本次推进与验收，用户批准后 HTML 冻结，执行进展只更新本文件。

## 工作计划

先以测试固定官方核心 palette 与语义映射，再改 CSS/Tailwind 基础，避免组件迁移期间出现第三套色表。随后对齐 Rust、splash、React 与跨窗口主题生命周期。第三步替换 CodeMirror theme extension，并验证 controlled wrapper。第四步按 raw-color inventory 分区迁移组件，使用真实浏览器检查主窗口、Settings、dialog、command palette 和 editor。最后更新 as-built 设计文档，执行 LFG 的简化、代码审查、browser QA、提交、推送、PR 与 CI 流程。

## 验证和验收

定向验证：

```bash
pnpm test -- tests/styles/themeContract.test.ts tests/components/JsonEditor.test.tsx tests/i18n/locales.test.ts
cargo test --manifest-path src-tauri/Cargo.toml theme_bg
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

1. 新安装默认显示 Mocha；持久化 light preference 后主窗口与 Settings 都显示 Latte。
2. 启动、打开 Settings 和切换模式时，不出现 Zinc 黑、纯白或其他错误首帧。
3. 背景层级、文本、边界、primary/focus、info/success/warning/error 在所有主要 surface 一致。
4. 键盘 focus 清晰可见，状态反馈不只依赖颜色。
5. CodeMirror 的 syntax、gutter、selection、cursor、active line 与 fold placeholder 随主题切换，内容和 history 不丢失。
6. 产品样式不再新增 raw Catppuccin/hex/framework 色；用户 profile 动态颜色保持不变。
7. 主题设计 MD/HTML 与代码 as-built 一致，后续 Agent 能据此设计新功能。

## 幂等性和恢复

主题 token 与组件迁移可按单元重复执行，测试与 raw-color inventory 提供确定性检查。若官方 CodeMirror 包导致 bundle 或运行时问题，可回滚该单元并恢复 One Dark，不影响其他 token。若 native/splash 与 React 不一致，先恢复三处旧背景常量再重新对齐。Git 不使用 destructive reset/checkout；提交按实施单元拆分，PR 前核对分支基线。

## 接口和依赖

新增 `@catppuccin/codemirror` 作为前端运行时依赖。主题接口以 CSS custom properties 为主，不新增 React Context 或 Zustand theme abstraction。`darkMode: boolean`、`write_theme_hint(dark)` 和双窗口 storage 同步接口保持兼容。新主题变量的最终命名以 `docs/design-docs/theme-system.md` 的语义角色为准；`--ds-*` 仅作为迁移桥，不是新功能 API。

## 里程碑规划

1. Token foundation：palette、semantic aliases、Tailwind、contract tests。
2. Theme lifecycle：native/splash/React/store/i18n。
3. Editor integration：official CodeMirror themes and behavior tests。
4. Surface migration：main/settings/overlays/states and browser QA。
5. Documentation and shipping：as-built docs, review, production build, PR and CI。

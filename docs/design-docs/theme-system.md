# AD 主题系统设计

> 状态：已实现（2026-07-16）
>
> 主主题：Catppuccin Mocha；亮色兼容：Catppuccin Latte
>
> 关联计划：`docs/exec-plans/active/catppuccin-mocha-theme.md`

## 结论

AD 采用 Catppuccin Mocha 作为默认深色主题，以 Sapphire `#74c7ec` 作为主要交互强调色，并保留现有亮色能力，以 Catppuccin Latte 提供同构语义映射。实现不允许组件直接选择 palette 色值，而是通过“原始 palette token → AD 语义 token → 组件状态”三层契约消费颜色。CSS 以 RGB channel 保存官方 palette，并通过 `rgb(var(--token) / <alpha-value>)` 暴露给 Tailwind，以保留透明度修饰符。

本设计避免常见的“深色底 + AI 紫”视觉套路，改用偏冷静、工具化的 Sapphire/Blue 色系建立交互辨识度。稳定的背景层级、文本层级、交互状态和代码编辑器主题覆盖主窗口、Settings 窗口、弹窗、命令面板、编辑器、启动 splash 与 Tauri 原生窗口底色。未来功能只选择语义角色，不重新发明颜色。

## 设计原则

1. **语义优先于色名。** 业务组件使用 `--color-bg-canvas`、`--color-text-muted`、`--color-action-primary` 等语义 token，不直接使用 `--ctp-mocha-sapphire` 或十六进制色值。
2. **层级来自中性色，强调色只表达状态。** Base、Mantle、Crust、Surface 构成空间层级；Sapphire、Blue、Sky、Green、Yellow、Red 只用于交互、链接和反馈。
3. **默认深色但不回退亮色能力。** Mocha 是产品默认与视觉基准；现有 light/dark preference、跨窗口同步和首帧 theme hint 保持兼容，亮色映射使用 Latte。
4. **文字可读性优先。** 正文使用 Text，辅助文本使用 Subtext，只有非关键细节使用 Overlay；低对比 accent 文字不得直接放在相近背景上。
5. **状态不能只靠颜色。** 成功、警告、错误、选中和禁用状态同时依靠图标、文案、边框、字重或形状表达。
6. **首帧就是主题的一部分。** `index.html` splash 与 Rust `WebviewWindowBuilder::background_color` 必须和 React canvas 一致，禁止启动时闪白或闪黑。

## Token 架构

```text
Catppuccin palette
  Mocha / Latte named colors
          │
          ▼
AD semantic tokens
  canvas / pane / surface / text / border / action / status / focus
          │
          ├──────────────┬──────────────────┬─────────────────┐
          ▼              ▼                  ▼                 ▼
Tailwind aliases     --ds-* bridge      CodeMirror       native splash
UI primitives       legacy inline UI    syntax theme     Tauri/WebKit
```

`--ds-*` 在迁移期作为兼容桥保留，但其值全部由语义 token 派生。新代码不得新增 `--ds-*`；完成现存 inline UI 的后续迁移后可在独立任务中移除兼容层。旧的 `clay`、`rust`、`olive`、`oat`、`slate2` Tailwind aliases 和 `--ds-clay` 已删除，不再是兼容面。

### As-built token 合同

基础实现位于 `src/styles/globals.css`，Tailwind 暴露位于 `tailwind.config.ts`。当前可供产品组件使用的核心角色如下：

| 类别 | CSS semantic token | Tailwind alias |
|---|---|---|
| Canvas / pane / chrome | `--color-bg-canvas` / `--color-bg-pane` / `--color-bg-chrome` | `background` / `pane` / `chrome` |
| Surface hierarchy | `--color-bg-surface` / `--color-bg-surface-hover` / `--color-bg-surface-active` | `surface` / `surface-hover` / `surface-active` |
| Text hierarchy | `--color-text-primary` / `--color-text-secondary` / `--color-text-muted` / `--color-text-disabled` | `foreground` / `muted-foreground`，其余通过基础组件消费 |
| Border / focus | `--color-border-subtle` / `--color-border-strong` / `--color-action-primary` | `border` / `input` / `ring` |
| Action / navigation | `--color-action-primary` / `--color-on-primary` / `--color-link` | `primary` / `primary-foreground` / `link` |
| Feedback | `--color-info` / `--color-success` / `--color-warning` / `--color-danger` | `info` / `success` / `warning` / `destructive` |
| Overlay / editor | `--color-overlay` / `--color-editor-cursor` | `overlay` / 由官方 CodeMirror theme 消费 |

## Palette 与语义映射

### 背景与表面

| AD 语义角色 | Mocha | Latte | 使用场景 |
|---|---|---|---|
| Canvas | Base `#1e1e2e` | Base `#eff1f5` | 主内容背景、编辑器背景 |
| Pane | Mantle `#181825` | Mantle `#e6e9ef` | 侧边栏、次级区域 |
| Chrome | Crust `#11111b` | Crust `#dce0e8` | 标题栏、最外层 chrome、首帧底色 |
| Surface 1 | Surface 0 `#313244` | Surface 0 `#ccd0da` | 卡片、输入框、弹窗 |
| Surface 2 | Surface 1 `#45475a` | Surface 1 `#bcc0cc` | hover、选中容器、较强分隔 |
| Surface 3 | Surface 2 `#585b70` | Surface 2 `#acb0be` | active、drag、强调边界 |

### 文本、边界与反馈

| AD 语义角色 | Catppuccin 色 | 规则 |
|---|---|---|
| Primary text | Text | 标题、正文、重要数值 |
| Secondary text | Subtext 1 | 标签、说明、次级正文 |
| Muted text | Subtext 0 / Overlay 2 | 元数据、占位符；关键操作不得低于此层 |
| Disabled text | Overlay 1 | 必须配合透明度或 disabled 状态 |
| Border subtle / strong | Surface 0 / Surface 1 | 通过层级而非白色透明度构造边界 |
| Primary action / focus | Sapphire | 主要按钮、选中、focus ring；Mocha on-primary 使用 Crust，Latte 使用稳定深色 `#11111b`，避免 Base/Text 在 Sapphire 上低于可读对比 |
| Link | Blue | 链接和可导航文本 |
| Information | Sky | 信息提示和非阻断说明 |
| Success | Green | 成功、干净状态、完成 |
| Warning | Yellow | 警告、待处理、风险提示 |
| Error / destructive | Red | 错误、删除、危险操作 |
| Editor cursor | Rosewater | CodeMirror caret；选区使用 Overlay 2 的 20%–30% |

## 组件状态契约

### 按钮

- Primary：Sapphire 背景 + `--color-on-primary` 文字；hover 使用同色透明度，不引入新 hex。Latte 不机械使用当前 flavor 的 Base/Text 作为前景。
- Secondary：Surface 0 背景 + Surface 1 边框 + Text。
- Ghost：透明背景；hover 进入 Surface 0。
- Destructive：Red 语义色；危险确认必须同时保留明确文案。
- Disabled：降低整体对比并禁用 pointer event；不能只把文字变成 Overlay。
- Focus：统一 2px Sapphire ring + canvas offset，键盘导航始终可见。

### 输入、选择器与编辑器

- 默认 Surface 0，边框 Surface 1；hover 提升边框，focus 使用 Sapphire ring。
- placeholder 使用 Overlay 2，错误状态使用 Red 边框并显示错误文本。
- CodeMirror 深色使用官方 `catppuccinMocha`，亮色使用 `catppuccinLatte`；AD 只追加布局和尺寸扩展，不复制 syntax 色表。
- 行号、gutter、active line、fold placeholder 和 scrollbar 必须使用主题 token，不能遗留 One Dark 或透明白灰。

### 容器、导航与弹层

- 主内容为 Base，侧边栏为 Mantle，标题栏与最外层 chrome 为 Crust。
- Card/Dialog/Popover 使用 Surface 0；hover/selected 通过 Surface 1 与 Sapphire 边界区分。
- Modal overlay 使用 Crust 的高透明度混色，不使用 `black/50` 之类无主题遮罩。
- 阴影只用于建立浮层高度，不承担边界；深色阴影采用 Crust 混色。

### 状态与数据色

- 成功 Green、警告 Yellow、错误 Red、信息 Sky；可导航链接使用 Blue。
- 多类别可视化可从 Teal、Sapphire、Lavender、Peach 中选择，但必须在文档中定义稳定类别映射。
- Profile 自定义颜色属于用户数据，不强制重映射；其周围的边界和文字仍遵循主题 token。

## 排版、间距与形状

- 字体继续使用 macOS system sans；路径、快捷键和配置内容使用 SF Mono fallback。主题任务不引入网络字体。
- 正文字号不低于 12px；重要控制与正文以 13–14px 为主；辅助 uppercase label 控制字距，避免全界面大写。
- 交互控件圆角以 6–8px 为主，dialog/card 以 8–12px 为主；圆角表达层级，不为每个组件新增尺寸。
- 动画保持 120–200ms，尊重 `prefers-reduced-motion`；颜色变化与 focus 不依赖长动画。

## 无障碍与验证

1. 主要正文、按钮标签和输入内容满足 WCAG AA 常规文本对比要求；较弱 Overlay 色只用于非关键辅助信息。
2. 所有可交互控件在键盘 focus 时具有可见 ring，hover 不是唯一反馈。
3. 错误、警告和成功同时包含文案或图标，不只依赖红黄绿。
4. 主窗口、Settings、所有 overlay、CodeMirror 和首帧 splash 在 Mocha/Latte 下均做浏览器截图巡检。
5. 主题契约测试固定官方 palette 值和核心语义映射，避免未来改动悄然引入第三套色表。

## 实现与自动化守卫

- `src/lib/theme.ts` 统一 React 根节点的 `.dark` class 与 pre-paint inline 根色；`index.html` 在 React 加载前使用同一组 Mocha/Latte 常量。
- `src-tauri/src/lib.rs::theme_bg_for` 固定 native WebView 背景，Rust 测试校验 Mocha Base `#1e1e2e` 与 Latte Base `#eff1f5`。
- `src/lib/editorTheme.ts` 只返回官方 `catppuccinMocha` / `catppuccinLatte` extension；`JsonEditor` 通过 Compartment 重配置，不重建编辑器。
- `tests/styles/themeContract.test.ts` 固定 palette、语义映射、首帧常量和旧基础色禁令。
- `tests/components/themeSurfaces.test.tsx` 递归扫描产品 TSX，拒绝 raw hex、数字 `rgb/rgba/hsl` 以及 framework/旧品牌色类。Profile schema/registry 的 `#7C3AED` 是用户数据默认值，不属于产品 chrome，明确保留。

## 取舍与否决方案

### 直接在组件中替换十六进制色值

否决。它能快速得到“像 Catppuccin”的截图，但无法约束未来功能，且会继续扩大当前 Tailwind token、`--ds-*` 与 inline style 三套系统的漂移。

### 只改深色，亮色保留 Zinc/Indigo

否决。保留两套不同视觉语义会让组件状态和文档规则分叉。使用 Latte 作为亮色同构映射能保留现有能力，同时让语义 token 只有一份。

### 删除亮色模式，只保留 Mocha

暂不采用。用户要求主题优化，没有要求移除已发布能力；删除 preference、跨窗口同步和原生 theme hint 会扩大产品变更范围。未来若要单主题化，应单独评估并迁移。

### 运行时依赖 palette 包生成 CSS

不采用。完整 palette 值稳定且体量很小，CSS 自包含更适合首帧与 Tauri splash；仅 CodeMirror 使用官方主题包，避免手工维护 syntax highlighting。

## 对未来功能的约束

1. 新组件必须先选择本文已有语义角色；缺少角色时先扩展语义层并更新本文件，不直接新增 palette/hex。
2. 禁止新增 `bg-black/*`、`text-emerald-*`、`rgba(255,255,255,...)` 等绕过主题的产品样式；调试和用户数据色除外。
3. Tailwind、CSS custom properties、CodeMirror 与原生首帧必须引用同一语义决策；任何新窗口都要加入 theme hint 验证。
4. 修改 palette 或主强调色属于设计系统变更，需要同步 MD + HTML 并进行双主题浏览器巡检。

## 依据

- Catppuccin palette 与设计哲学：<https://github.com/catppuccin/catppuccin>
- Catppuccin 官方 Style Guide：<https://github.com/catppuccin/catppuccin/blob/main/docs/style-guide.md>
- Catppuccin palette 包与 machine-readable 色值：<https://github.com/catppuccin/palette>
- Catppuccin CodeMirror 6 主题：<https://github.com/catppuccin/codemirror>

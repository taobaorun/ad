# 品牌化首次启动 Loading

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

AD 当前已有用于避免闪白的 pre-React splash，但主窗口 React 挂载两个 frame 后就会移除它；此时 Agent discovery、Profiles 和 Projects 仍可能在后台加载，首次启动会露出空白或未就绪主界面。本任务把现有 splash 改成品牌化 loading：展示现有 AD logo 与 “Be Water, My Friend” 探照灯文字效果，真实初始化尝试结束后再平滑露出主界面。

这不是启动性能优化，也不会人为延长等待。现有主窗口依然尽快显示，只是从第一帧到数据就绪之间始终有明确内容可看。

统一实现计划：`docs/plans/2026-07-17-001-feat-branded-startup-loading-plan.md`。

## 确认状态

- [x] **用户已评审** — HTML 路径：`docs/exec-plans/active/branded-startup-loading.html`
- [x] 用户已确认，开始执行（2026-07-17 09:37 CST）

## 假设

1. 用户所说的 logo 指现有 `src-tauri/icons/` 中的 AD 应用图标，不新增或重绘品牌资产。
2. “Be Water, My Friend” 是固定品牌文案，zh/en locale 都保留该英文原句，但仍通过 i18n key 输出。
3. 可见构图只保留 logo 和原句，不显示 “Initializing AD” 或其他第二行状态文案；加载状态通过唯一不可见 polite/atomic `role=status` 表达，logo 对读屏器隐藏。
4. loading 是等待体验，不是百分比进度条；不展示虚假进度和预计时间。
5. 单个初始化命令 rejection 或永久 pending 时不能卡住 loading；所有必要尝试 settled 或整体达到 12 秒 deadline 后露出当前可用主界面，晚到的幂等 read 仍可渐进更新 store。
6. 不修改 Rust 窗口架构、Tauri IPC、用户数据格式或 `~/.ad/` 内容；如果实测慢点主要发生在 WebView 创建前的 Rust migration，本任务停止扩 scope，另立架构任务。

## 影响范围

```text
index.html                              [M] 品牌 splash、logo、探照灯、reduced-motion、退出态
public/ad-logo.png                      [N] 从现有 128px app icon 复制的 WebView public asset
src/main.tsx                            [M] 主窗口初始化协调、i18n splash 文案、ready 后揭幕
src/App.tsx                             [M] 移除过早 splash/重复初始 load，保留 Agent 切换刷新
src/components/AgentSelector.tsx        [M] 移除只用于 mount 初始化的 hook
src/lib/startup.ts                      [N] 可测试的启动顺序与失败 settlement
src/lib/startupSurface.ts               [N] store-free 文案、文字探照灯与 crossfade helper
src/hooks/useReloadProfilesOnAgentChange.ts [N] Agent 切换后的 Profiles reload 与 rejection 处理
src/i18n/locales/{zh,en}.json           [M] slogan 与 accessibility 文案
tests/lib/startup.test.ts               [N] 顺序、并发、失败与单次执行测试
tests/main.test.tsx                     [N] main/Settings bootstrap route 合同
tests/hooks/useReloadProfilesOnAgentChange.test.ts [N] Agent reload effect 合同
tests/styles/themeContract.test.ts      [M] 首帧、logo、motion 与主题静态合同
docs/design-docs/theme-system.{md,html} [M] 完成后同步 as-built 启动状态规则
```

不修改 `docs/exec-plans/active/bundle-slim-codemirror.*`；其 bundle 优化和未完成的真实冷启动验证仍是独立工作。

## 关键决策

1. **复用现有 `index.html` splash，不做独立 splash window。** 这样 JS bundle 执行前也能看到主题正确的首帧，并保持 Rust `on_page_load(Finished)` 尽快 show/focus 的策略；ready 只移除 overlay，不再次抢焦点。
2. **以真实初始化 settlement/deadline 决定退出，不以 React mount 决定。** Projects 与 Agent discovery 同时开始，Profiles 等 Agent attempt；三项结束或整体达到 12 秒 deadline 后渲染/揭幕。
3. **失败或挂起时也揭幕。** 当前没有完整的首次启动 retry/error 产品流，永久 loading 比部分可用界面更糟；rejection/timeout 用英文 console 记录，晚到的幂等 read 可继续更新 store。
4. **固定使用现有 AD logo + 原句探照灯。** 这是用户明确指定的品牌方向，替代当前匿名 skeleton。
5. **不设最短展示时间。** 快机上 loading 可以很快结束；慢机上自然持续。主界面淡入与 splash 淡出使用同一条 260ms transition；文字探照灯按用户反馈加速为 750ms 循环，且不照亮背景。
6. **完整无障碍 fallback。** `prefers-reduced-motion` 下关闭探照灯和 logo 循环，原句保持静态可读；Mocha/Latte 均使用现有主题根色。
7. **不显示第二行状态文案。** 用户评审已明确去掉 “Initializing AD”；可见区域只有 logo 和原句。唯一不可见 live status 不占视觉空间；splash 覆盖期间 `#root` inert + aria-hidden，揭幕后恢复且不主动移动焦点。
8. **对比度是硬门禁。** 探照灯非高亮区在 Mocha/Latte 的整个循环中保持至少 4.5:1；不支持 background clipping 和 reduced-motion 时同样完整可读。

## 进展

- [x] (2026-07-17 09:46 CST) 步骤零：真实冷启动拆分 pre-WebView migration 与 WebView 后初始化耗时（验证：`/Applications/AD.app` 可冷启动；受 macOS 辅助功能查询限制未取得可靠 3-run 毫秒分段，但用户明确报告窗口出现后空白，且代码时序证明 Rust 只在 HTML page-load finished 后 show，随后 App 在未等待三组 store load 时移除 splash，因此结论为 CONTINUE；限制已记录）。
- [x] (2026-07-17 09:52 CST) 步骤一：先写启动协调器与首帧静态合同的失败测试（验证：startup 测试先因缺少 `@/lib/startup` 失败；theme/i18n 合同先出现 4 个缺失品牌 splash 合同的失败）。
- [x] (2026-07-17 10:02 CST) 步骤二：实现 `src/lib/startup.ts`，固定 Agent → Profiles/Projects 顺序与 failure settlement（验证：10 个 startup tests 通过，覆盖 12 秒 deadline、三类 rejection、晚到更新与单次执行）。
- [x] (2026-07-17 10:10 CST) 步骤三：把初始化接入 `src/main.tsx`，清理 `App`/`AgentSelector` 的重复 mount load（验证：focused 24 tests、typecheck、目标 ESLint 全部通过；Settings 无关格式化 diff 已清除）。
- [x] (2026-07-17 13:07 CST) 步骤四：实现 logo + slogan 文字探照灯 loading、260ms 交叉淡入淡出与同步 i18n（验证：logo 与 app icon SHA-256 一致；无可见 “Initializing AD”；真实浏览器 180ms 间隔三帧 background-position 均变化；交叉过渡中间帧 root/splash opacity 约 0.61/0.39；normal/reduced-motion 与 i18n 合同通过）。
- [x] (2026-07-17 14:02 CST) 步骤五：执行完整 frontend gates、production Tauri build 与真实冷启动 QA（验证：21 个测试文件 / 97 个测试、typecheck、lint、Vite production build 与 Tauri `.app`/DMG 全部通过；真实 Chrome 以 140ms 间隔采样四帧文字 background-position 均不同，splash 背景 `background-image: none`；260ms crossfade 中间帧 root/splash opacity 约 0.61/0.39；production bundle 已覆盖安装并启动，PID 48051）。
- [ ] 步骤六：更新 `theme-system` MD/HTML 为 as-built，完成 LFG 简化、review、browser QA、提交、PR 与 CI；归档本 ExecPlan MD/HTML。

## 意外发现

- 发现：现有 blank-screen 防护不是缺失，而是退出条件过早。
  证据：`index.html` 已有 `#ad-splash`，`src/App.tsx` 在 mount 后双 `requestAnimationFrame` 移除；三个 store load 未被等待。
- 发现：仓库已有独立的 bundle 启动优化 ExecPlan，且否决 deferred-show。
  证据：`docs/exec-plans/active/bundle-slim-codemirror.md` 假设 7 与决策 D5。
- 发现：首帧颜色是 native、HTML、React 三层主题合同，不能为 loading 引入第三套颜色。
  证据：`docs/design-docs/theme-system.md` 与 `tests/styles/themeContract.test.ts`。
- 发现：首次 migration 在 WebView 创建前同步运行，HTML loading 无法覆盖该阶段。
  证据：`src-tauri/src/lib.rs` 的 setup 顺序；执行前必须用真实冷启动确认用户感知的慢点位于 WebView 阶段。
- 发现：Projects 初始读取不依赖 active Agent，不应和 Profiles 一起等待 Agent discovery。
  证据：`src/store/projects.ts::loadAll` 只调用 projects/scan-roots IPC；adversarial plan review 建议与 Agent 并发。
- 发现：仅用 `allSettled` 不能覆盖永不 resolve 的 IPC。
  证据：adversarial plan review；增加 12 秒整体 deadline，超时 fail-open。
- 发现：macOS 辅助功能窗口查询在本机不可用，无法自动采集三次 visible/ready 毫秒分段。
  证据：release app 可由 `open -na` 启动且进程可见，但窗口查询无返回；U0 改用用户可见空白报告与 Rust→HTML→React 静态时序作为 continue 证据，未伪造 timing 数字。
- 发现：WKWebView 能绘制文字 gradient clipping，但仅依赖 CSS keyframes 时用户侧多次观察不到位置重绘；把 gradient 放在 inline span 上也无法与设计效果对齐。
  证据：多个已安装 production build 均只显示静态高光；改为对整行 `#ad-splash-quote` 逐帧更新 background-position 后，真实浏览器采样的三帧位置分别变化，视觉高光只位于字形内部。
- 发现：直接在 transition listener 上使用 `{ once: true }` 会被子元素冒泡的 `transitionend` 提前消耗。
  证据：新增子元素冒泡回归测试；listener 现在只在 `event.target === splash` 或 timeout 时完成并清理。

## 决策日志

- 决策：把本任务定位为 loading lifecycle + branded first-frame，而不是性能修复。
  理由：已有 bundle 计划负责性能；本任务解决的是数据仍加载时过早露出界面。
  日期/作者：2026-07-17 / Codex
- 决策：更新现有 `theme-system` 设计文档，不创建第二份 startup 设计源。
  理由：该文档已经定义 splash、native paint、motion 与主题约束，扩展它能避免设计记忆分叉。
  日期/作者：2026-07-17 / Codex
- 决策：Projects 与 Agent discovery 并行，Profiles 等 Agent；整体 12 秒 deadline。
  理由：缩短独立读取的关键路径，并覆盖永不 settle 的 Tauri 调用；晚到操作是幂等 read，可安全渐进更新。
  日期/作者：2026-07-17 / Codex
- 决策：正常模式以 requestAnimationFrame 驱动 750ms 文字 background-position，保留 CSS gradient/clip 作为视觉定义；背景不参与探照灯。
  理由：用户多次验证纯 CSS 与 inline span 方案在 WKWebView 中无可见动画；逐帧 helper 保证重绘，`prefers-reduced-motion` 时不启动循环。
  日期/作者：2026-07-17 / Codex
- 决策：揭幕改为 root 淡入与 splash 淡出同步进行的 260ms crossfade。
  理由：用户明确指出直接移除 loading、突然出现主界面不自然；同一时长的互补 opacity 让状态边界连续。
  日期/作者：2026-07-17 / Codex

## 结果回顾

主窗口现在先并行下载 App chunk、加载 Agents/Projects，Profiles 在 Agent attempt 后加载；三项 settle 或 12 秒 deadline 后才渲染和揭幕。第一可见帧只显示现有 AD logo 与 “Be Water, My Friend”，750ms 探照灯只在字形内循环，背景保持纯主题色；主界面与 loading 用 260ms 互补 opacity 自然交叉切换。rejection 会记录英文诊断后 fail-open，永不 resolve 的任务由 deadline 兜底，晚到 read 仍可更新 store；Settings 不执行主窗口 coordinator。

自动验证覆盖 21 个测试文件 / 97 个测试，typecheck、lint、Vite production build、Tauri `.app` 与 DMG 均通过。真实浏览器逐帧采样证明动画位置连续变化且背景无探照灯，交叉过渡存在中间透明度；production bundle 已重新安装到 `/Applications/AD.app` 并启动。全仓 `pnpm format:check` 仍会命中 72 个历史未格式化文件和一个既有 malformed completed ExecPlan HTML，本任务未扩大范围修改这些无关文件；本次 diff 自身通过 `git diff --check` 与目标 Prettier 检查。

## 上下文和方向

`index.html` 在 React/Tailwind 前读取 `ad.ui-settings.v1`，设置 Mocha/Latte 根色并绘制 `#ad-splash`。Rust 在 WebView page-load 完成后可靠地 show/focus main window；前端仍残留重复 show/focus。`src/main.tsx` 动态加载 `App` 或 `SettingsApp`。目前 `src/App.tsx` 通过独立 effects 调用 Agents、Profiles、Projects store，并在 mount 后双 frame 直接移除 splash，因此 splash 生命周期和数据生命周期没有连接。

实现保持 splash 在 `#root` 外，让它跨越 bundle import 和数据加载。主窗口 bootstrap 并行下载 App chunk 与执行初始化：Projects 与 Agent discovery 同时开始，Profiles 只等待 Agent attempt；三项 settle 或达到 12 秒 deadline 后渲染。React App paint 后给 splash 添加短退出态并移除。Settings route 不执行主窗口初始化，维持现有独立启动。

## 工作计划

先用 `tests/lib/startup.test.ts` 固定顺序、并发和 failure-settled 行为，再新增最小 `src/lib/startup.ts`。随后由 `src/main.tsx` 统一调用协调器，删除 `App` 和 `AgentSelector` 的 mount 初始化职责，并用首次 render guard 保留用户后续切换 Agent 时的 Profiles reload。

视觉部分把 `src-tauri/icons/128x128.png` 复制为 `public/ad-logo.png`，再改 `index.html` 的现有 splash：增加 i18n slogan 插槽、用 feature-gated gradient clipping 定义只作用于文字的探照灯，并由 startup helper 驱动 background-position，提供 reduced-motion 静态 fallback。`src/main.tsx` 在长初始化开始前从 i18n 注入文案和 accessibility label。最后用现有 theme contract 测试固定首帧颜色与 motion 合同，production build 验证图标被打包。

## 验证和验收

自动化门禁：

```bash
pnpm test -- tests/lib/startup.test.ts tests/styles/themeContract.test.ts tests/i18n/locales.test.ts
pnpm test
pnpm typecheck
pnpm lint
pnpm build
pnpm tauri build
```

行为验收：

1. 冷启动 main window，第一可见帧是当前主题的 AD logo + “Be Water, My Friend”，无空白/错误色帧。
2. 人为延迟 Tauri 初始化调用，loading 持续覆盖；全部必要尝试 settled 后才淡出显示已初始化主界面。
3. 人为让一个初始化调用失败，loading 不永久停留，console 有英文可操作上下文。
4. Mocha 与 Latte 均无主题跳闪；`prefers-reduced-motion` 下所有循环动效停止且文案清晰。
5. Settings window 后续打开正常，不受 main startup coordinator 影响。
6. 记录 WebView 创建前 migration 与 WebView 后 frontend 初始化的实际等待分布；若前者占主导，不把本 loading 宣称为完整覆盖。
7. splash 存在时读屏器只接收一个 loading status，不能访问被覆盖的主界面；揭幕后 root 恢复且不抢焦点。
8. 自动计算验证 Mocha/Latte 下探照灯非高亮文案对比度均不低于 4.5:1。

## 幂等性和恢复

启动协调器只读取已有 store/IPC，不写新持久化数据；现有 store inflight guard 仍防重复。失败后重新启动应用即可重试。若 production bundle 无法解析 logo，回退到原有 splash markup 不影响数据或 Rust 状态。Git 不使用 destructive reset/checkout；可按实现单元 revert。

## 接口和依赖

- 不新增 npm 或 Rust dependency。
- 新增前端内部 `initializeMainWindow`（最终命名以实现为准），返回可观察的 settled/failure 结果，不暴露公共 API。
- 复用 `useAgents.getState().loadAll`、`useProfiles.getState().loadAll`、`useProjects.getState().loadAll`。
- 以 `src-tauri/icons/128x128.png` 为品牌源，新增 `public/ad-logo.png` 供 WebView 使用；production Vite/Tauri build 必须验证资源路径。
- 不修改 Tauri IPC、Rust commands 或用户文件格式。

## 里程碑规划

- M1 启动生命周期合同与实现。
- M2 Bootstrap 集成与重复加载清理。
- M3 品牌 loading、i18n、motion/accessibility。
- M4 完整验证、设计记忆同步、LFG 交付。

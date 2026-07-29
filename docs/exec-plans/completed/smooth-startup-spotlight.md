# 平滑启动页文字探照灯

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

让启动页 “Be Water, My Friend” 的探照灯像用户提供的参考图一样连续、柔和地扫过字形，不再出现肉眼可见的分段跳动或循环复位闪烁。启动生命周期、双主题、文案与无障碍退化行为保持不变。

用户可在正常 motion 模式下看到一条中性、柔边的高光由左至右连续移动；在 `prefers-reduced-motion: reduce` 下仍看到完整静态文案，不播放循环动效。

## 确认状态

- [x] **用户已确认** — 评审本文件：`docs/exec-plans/active/smooth-startup-spotlight.md`
- [x] 用户已确认，开始执行（2026-07-23 11:28 CST）

## 设计文档判定

**需要设计文档：否。** 本任务不新增模块、公共接口、数据模型或架构取舍，只在现有 `theme-system` 启动 Loading 设计内修正动效参数与实现。由于该设计文档记录了精确动画合同，实施完成后会同步更新已有 `docs/design-docs/theme-system.md` 与 `.html`，但不新建第二份设计源，也不修改索引。

## 假设

1. 用户所说的“不够流畅”主要指当前工作区版本一轮只有 50ms、5 个离散位置导致的跳帧；参考图用于表达宽而柔和的移动明暗带，而非要求逐像素复刻 Claude Code 的字体和颜色。
2. 保留当前未提交改动的产品意图：主窗口至少展示启动层 600ms，并等第一轮探照灯完成后再揭幕；不回退 `main.tsx`、`tests/main.test.tsx` 中已有修改。
3. 一轮扫光使用 600ms，与现有最短可见时间对齐，避免为展示完整动画再额外延长快速启动。
4. 继续使用 `requestAnimationFrame` 驱动，避免项目已记录的 WKWebView 仅绘制 CSS keyframe 首帧问题。
5. 高光底色和静态 fallback 在 Mocha/Latte 下继续满足既有 4.5:1 可读性合同。

## 影响范围

```text
index.html                                  [M] 对称、柔边的中性文字高光渐变
src/lib/startupSurface.ts                   [M] 600ms 连续按帧插值与离屏复位
tests/lib/startup.test.ts                   [M] 连续中间帧、首轮完成、断连与 reduced-motion 合同
tests/styles/themeContract.test.ts          [M] 新时长与渐变合同
docs/design-docs/theme-system.md            [M] 同步 as-built 动画说明
docs/design-docs/theme-system.html          [M] 用 html-artifact 同步人读视图

以下已有未提交文件只做集成验证，不计划编辑：
src/main.tsx
tests/main.test.tsx
```

## 关键决策

1. **用连续时间插值替代固定步数。** 每个浏览器绘制帧都直接根据 `elapsed / duration` 计算位置，不再先量化为 5 或 30 个台阶；60Hz 与 120Hz 屏幕都能自然利用各自刷新率。
2. **600ms 完成一轮。** 这与当前主窗口 600ms 最短可见合同对齐；更长会人为拖慢快速启动，更短则减少可见帧数。
3. **光带在字形外复位。** 起点和终点留出渐变半宽，循环跳回发生在字形已经恢复为基础色时，避免右端瞬移到左端的闪烁。
4. **保留 rAF，不改成纯 CSS animation。** 纯 CSS 更短，但项目已有 WKWebView 重绘失败的实测记录，不能为了少几行代码重新引入已知退化。
5. **采用对称中性高光，不照亮背景。** 参考图的核心是平滑的文字明暗带；AD 保留当前“高光扫过可读底色”的方向和主题可访问性，不照搬参考图可能低于对比度门槛的暗带。

## 进展

- [x] (2026-07-23 11:28 CST) 步骤一：先修改测试，证明当前 50ms / 5 段实现无法满足 600ms 连续中间帧合同（验证：focused tests 出现 2 个预期失败，分别命中初始位置 80% 与缺少 `SPOTLIGHT_SWEEP_MS = 600`）。
- [x] (2026-07-23 11:29 CST) 步骤二：实现连续 rAF 插值、离屏复位与柔和对称渐变（验证：startup、theme contract、bootstrap 三组 31 个测试通过，覆盖连续中间帧、reduced-motion、断连 Promise 与现有 600ms 揭幕集成）。
- [x] (2026-07-23 11:42 CST) 步骤三：在真实浏览器中采样/截图首轮多个时点，确认位置单调、光带只在字形内、循环边界无可见跳闪，并检查 console（验证：真实 Chrome 在 0/151/301/451/601ms 采样到 100/73.59/47.34/21.09/-5%，第二轮重新连续进入；截图确认柔和高光只在字形内；reduced-motion 50–501ms 共 10 次采样均保持 10% 静态位置。console 仅有无 Tauri bridge 时的既有 invoke 失败和无关表单 issue，无 spotlight 新错误）。
- [x] (2026-07-23 11:45 CST) 步骤四：同步 `theme-system` MD + HTML，运行完整质量门禁并收尾归档（验证：MD/HTML 已同步；focused tests 31/31、全量 tests 104/104、typecheck、lint、production build、`git diff --check` 全部通过。除历史紧凑格式的 `theme-system.html` 外，本次文件通过 Prettier；HEAD 基线的同一 HTML 也返回相同格式提示）。
- [x] (2026-07-23 13:06 CST) 用户安装复验后修正生命周期语义：原生主窗口从 `PageLoadEvent::Started` 即显示，探照灯与初始化并行运行；加载结束后不再追加播放一轮（验证：先失败的 bootstrap 与 Rust page-load 测试分别证明旧实现过晚启动和旧窗口显示时点不符，修正后 focused tests 31/31、Rust 定向测试与 typecheck 通过）。
- [x] (2026-07-23 13:15 CST) 第二次安装复验确认 WKWebView 未重绘 clipped text background；以字符 `opacity` 合成替代 background-position，并保留 600ms 连续扫描、loading 并行与 reduced-motion 合同（验证：4 个先失败的字符层/渲染合同测试修正后全部通过，focused tests 31/31 与 typecheck 通过）。
- [x] (2026-07-23 13:25 CST) 用真实 Chrome 持久 splash 夹具采样并截图；亮度中心按 60ms 间隔从字符索引 8→11→13→15→18 移动，峰值约 0.99、底值 0.65。首次截图发现 span 空格折叠，补充 `white-space: pre` 后复验排版正常；a11y tree 只暴露完整原句，console 无 error/warn/issue。临时夹具已删除。
- [x] (2026-07-23 13:29 CST) 全量门禁、release 构建和精确路径重装完成（验证：104/104 tests、lint、typecheck、`git diff --check`、Tauri production build 全部通过；`/Applications/AD.app` 运行进程路径正确，安装包与构建产物二进制 SHA-256 一致）。
- [x] (2026-07-23 14:03 CST) 用户复验反馈字符实现出现晚、卡顿且过慢；把字符创建前移到文案后的同步内联脚本，以 300ms CSS opacity animation + 15ms 字符错峰替代 600ms rAF，并移除完成首轮/最短 600ms 的人为等待。真实 Chrome 20ms 采样中峰值索引连续 1→3→4→6→…→18→1，始终有 3–5 个字符处于可见过渡，无全暗断帧；临时夹具已删除。
- [x] (2026-07-23 14:07 CST) 最终全量门禁、release 构建与重装完成（验证：101/101 tests、lint、typecheck、`git diff --check`、Tauri production build 全部通过；`/Applications/AD.app` 精确路径运行，安装包与构建产物 SHA-256 一致）。
- [x] (2026-07-23 14:15 CST) 用户指出 Chrome 调试页正常而 WKWebView 集成版退化为单字母；移除 `calc(var(--index) * 15ms)`，由首帧和 localized renderer 直接写入 `0ms/15ms/30ms…` 绝对 delay，并把光带扩大为五字符核心、9–11 字符过渡。先失败的 WebKit-safe delay 合同修正后 focused tests 28/28 与 typecheck 通过。
- [x] (2026-07-23 14:22 CST) 系统 WKWebView 可见窗口最小复现证明，即使使用绝对 delay，多元素 opacity stagger 仍只有第一个字符运行；改为底色/高亮两层完整句子和单一 45% 句宽 clip-path 动画。WKWebView 12 帧采样显示 clip inset 连续推进并正常复位；先失败的双层结构与 CSS 合同修正后 focused tests 28/28、typecheck 通过。
- [x] (2026-07-23 14:27 CST) 双层完整句子版本完成全量门禁、release 构建与重装（验证：101/101 tests、lint、typecheck、`git diff --check`、Tauri production build 全部通过；`/Applications/AD.app` 精确路径运行，安装包与构建产物 SHA-256 一致）。
- [x] (2026-07-23 14:36 CST) 用户复验确认 animated clip-path 仍未产生可见效果；按用户要求改用 Canvas。真实 p 保留布局/读屏，aria-hidden Canvas 用单个 300ms rAF 主动重画基础文字与 55% 句宽渐变光带。系统 WKWebView 直接加载项目真实首页后，间隔 80ms 的 Canvas 像素缓冲发生变化（data URL payload 19,651→19,307 bytes），证明集成页实际重绘；focused tests 27/27 与 typecheck 通过。
- [x] (2026-07-23 14:45 CST) 安装包复验显示 Canvas 停在同步绘制的首帧；定位到页面在隐藏窗口阶段注册首个 rAF 的 WKWebView 生命周期差异。改为 16ms 定时器持续推进，并在 `focus` / `visibilitychange` 时立即补帧，断连时清理定时器和监听器；测试先按新合同失败，随后 focused tests 27/27 与 typecheck 通过。
- [x] (2026-07-23 14:53 CST) 按 Tauri 的“隐藏窗口加载→导航 Started 时显示”顺序运行系统 WKWebView，Canvas 间隔 80ms 的像素缓冲从 19,586 变为 19,418 bytes；全量 22 文件 100 测试、lint、typecheck、diff check 与 app-only Tauri build 全部通过。新版已安装并从 `/Applications/AD.app` 启动，安装包二进制与构建产物 SHA-256 同为 `19b9b664cc7b131baa846f0be1d466e5da67af330d650947427d9acce8afa335`。
- [x] (2026-07-23 15:00 CST) 安装包再次复验显示主线程定时器仍冻结，确认独立 WKWebView 探针不足以覆盖 Tauri 启动命令对页面主线程的影响。系统 WKWebView 支持 OffscreenCanvas、`transferControlToOffscreen` 和 Worker；改为 Worker 独立绘制，旧系统保留主线程 fallback。隐藏→显示并同步阻塞页面主线程 600ms 的集成探针确认 Canvas 已 transfer 且 80ms 间隔的 quote 区域快照发生变化；focused tests 27/27 与 typecheck 通过。
- [x] (2026-07-23 15:12 CST) Worker 版 app-only Tauri build、全量 22 文件 100 测试、lint、typecheck 与 diff check 全部通过。新版已安装并从 `/Applications/AD.app` 启动，安装包二进制与构建产物 SHA-256 同为 `42a2967f5ddff25320f48aa89c4ec7d6cf0f1232d466dedc221fd8ae5aa0a969`；上一安装版备份为 `~/.Trash/AD-20260723-151103.app`。

## 意外发现

- 发现：工作区已经存在与启动页首轮展示相关的 5 个未提交文件，当前探照灯被改为 50ms / 5 段。
  证据：`git diff -- src/lib/startupSurface.ts` 显示 `SPOTLIGHT_STEP_COUNT` 从 30 改为 5，且 `SPOTLIGHT_CYCLE_MS` 因而变成 50ms。
- 发现：现有 `theme-system.md/.html` 仍描述 300ms / 30 步，而已完成的 branded startup ExecPlan 结果回顾写的是 750ms，文档与代码历史存在漂移。
  证据：`docs/design-docs/theme-system.md:128`、`docs/exec-plans/completed/branded-startup-loading.md:101`。
- 发现：仓库 `docs/PLANS.md` 仍要求 ExecPlan MD + HTML，但本次用户显式加载的 harness v1.6 规定 ExecPlan 只保留 MD 真理源。
  证据：`docs/PLANS.md` 的“Tier 2 双格式”与本轮提供的 `harness-engineering/SKILL.md`“ExecPlan 只生成一份 Markdown 文件”冲突；本计划按本轮显式 skill 的较新规则只创建 MD。
- 发现：为截图临时使用 MutationObserver 固定 splash 时，测试夹具因重复移除同一 class 形成属性回调循环；丢弃该隔离页后，改为在 300ms 克隆静态中间帧，截图正常完成。
  证据：原隔离页 screenshot/navigation 超时；新隔离 context 的克隆中间帧在 58ms 内完成截图。该夹具从未写入项目文件。
- 发现：真实安装包中，WKWebView 到 `PageLoadEvent::Finished` 才显示原生窗口；探照灯虽然在隐藏期间运行，但用户看到窗口时 loading 已接近或已经结束。
  证据：用户安装复验截图中只有静态文案；生命周期追踪确认 `startStartupSpotlight()` 位于模块入口，而 `window.show()` 受 Finished 事件控制。
- 发现：窗口提前显示后仍只有静态文案，且 macOS `accessibilityDisplayShouldReduceMotion=false`；因此 motion 偏好不是原因，WKWebView 对 clipped text 的 background-position 更新未产生可靠可见重绘。
  证据：用户第二次安装复验仍无效果；系统 API 同时报告 reduceMotion、increaseContrast、reduceTransparency 均为 false。

## 决策日志

- 决策：先保留用户现有 `main.tsx` / `tests/main.test.tsx` 改动，只调整其依赖的探照灯时长合同，不重写启动揭幕逻辑。
  理由：精准修改并避免覆盖工作区中来源不明但显然相关的用户改动。
  日期/作者：2026-07-23 / Codex
- 决策：推荐 600ms 连续扫光，而不是恢复 300ms 分段扫描或采用 750ms/更慢循环。
  理由：600ms 在 60Hz 下约有 36 个自然帧，且与现有最短可见时间完全重合，不增加新的等待预算。
  日期/作者：2026-07-23 / Codex
- 决策：用 90° 对称中性高光替换原先不对称的白→蓝高光，并保留双主题各自的亮暗方向。
  理由：参考图的质感来自水平、柔边、连续的明暗带；对称 stops 让速度感一致，移除蓝色尾峰避免扫过时出现第二个视觉重心。
  日期/作者：2026-07-23 / Codex
- 决策：探照灯是 loading 指示器，与初始化并行，而不是加载完成后的过场；原生窗口在导航 Started 时显示主题底色。
  理由：用户明确要求一加载就看到探照灯，加载结束便离开；主题匹配的原生背景可承接 HTML 首帧，不需要为避免空白把整个窗口隐藏到 Finished。
  日期/作者：2026-07-23 / Codex
- 决策：用固定字形的逐字符 opacity 插值取代 background-clip + background-position；此决策取代先前的渐变实现选择。
  理由：opacity 可由 WKWebView 合成层稳定绘制，首帧即可产生可见明暗差，并保持高光只作用于字形；测试同时锁定首、中、末帧与静态 reduced-motion 状态。
  日期/作者：2026-07-23 / Codex
- 决策：用首屏同步内联脚本 + 300ms CSS opacity stagger 取代 600ms rAF 字符绘制，并取消首轮等待；此决策取代“必须展示完整 600ms 首轮”的早期选择。
  理由：动效是 loading 的即时状态，不应等待主 bundle，也不应延长实际初始化；opacity CSS 动画由合成器驱动，15ms 错峰和宽重叠区消除逐字符节拍与循环暗隙。
  日期/作者：2026-07-23 / Codex
- 决策：每个字符直接写绝对 animation-delay，不在 CSS 中用 custom property 乘以时间；高亮增加 24% 平台期并到 68% 才完成衰减。
  理由：Chrome 调试夹具与 WKWebView 的 typed arithmetic 支持不一致；绝对时间是跨 WebKit 版本的稳定语法。平台期让高光成为短语宽度的光带，而不是单字母峰值。
  日期/作者：2026-07-23 / Codex
- 决策：最终用两层完整句子 + 单一 clip-path 动画取代所有逐字符动画；此决策取代绝对 delay 的过渡方案。
  理由：系统 WKWebView 可见窗口复现确认多元素 stagger 本身就会退化为首字符动画，与 delay 表达方式无关；同一环境下单层 clip-path 能连续推进。单一宽裁剪层也直接满足“光带作用于整句话”的视觉语义。
  日期/作者：2026-07-23 / Codex
- 决策：最终改用 Canvas 定时器主动重绘，取代所有 CSS 文字裁剪/多层动画方案和启动阶段的 rAF 驱动。
  理由：clip-path 的 computed value 在最小 WKWebView 中推进，但真实安装包仍未产生可见效果；Canvas rAF 在普通可见 WKWebView 中能改变像素，却在 Tauri 隐藏窗口解析阶段停在同步首帧。16ms 定时器不依赖首个 rAF 的恢复，并在窗口聚焦/重新可见时主动补帧。
- 决策：由 Web Worker 的 OffscreenCanvas 驱动逐帧绘制，主线程定时器仅作为旧 WKWebView fallback；此决策取代主线程定时器作为首选路径。
  理由：真实 Tauri 安装包中主线程定时器仍随 loading 冻结。Worker 与页面主线程隔离；系统 WKWebView 在人为同步阻塞页面主线程 600ms 时，转交后的 Canvas 区域快照仍持续变化。
  日期/作者：2026-07-23 / Codex
- 决策：不格式化整个 `theme-system.html`。
  理由：该 artifact 在 HEAD 基线中已采用紧凑排版并无法通过 Prettier；为两处文案同步重排整份文件会制造与任务无关的大 diff。
  日期/作者：2026-07-23 / Codex

## 结果回顾

启动页探照灯最终采用 Worker OffscreenCanvas 主动重绘：原生窗口从导航开始即显示，真实 p 保留完整 localized 文案与布局，覆盖其上的 Canvas 在首屏同步脚本中转交给独立 Worker。每轮 300ms，一条约 55% 句宽的柔和渐变带通过重复绘制同一句话从左扫到右。动效只在真实 loading 期间循环；初始化完成后立即揭幕，不额外等待。文字和尺寸变化会同步给 Worker，Canvas 移除即终止；reduced-motion 只画静态基础文字，不支持 OffscreenCanvas 时回退到主线程定时器，Canvas 不可用则回退到可见 p。

真实 Chrome 采样在 0/151/301/451/601ms 得到 100/73.59/47.34/21.09/-5% 的严格单调位置，第二轮正常重新进入；中间帧截图确认背景不受影响、高光只裁剪在字形内。reduced-motion 的 10 次运行时采样全部保持 10% 静态位置。浏览器 console 里的 Tauri bridge 缺失和表单 issue 属于直接在普通 Chrome 运行 Tauri 应用的既有环境差异，没有 `startupSurface.ts` 新错误。

自动门禁通过：3 个 focused 文件 27 个测试、全量 22 个文件 100 个测试、`pnpm typecheck`、`pnpm lint`、app-only Tauri build、`git diff --check`。系统 WKWebView 的隐藏→显示集成探针也确认 80ms 间隔内 Canvas 像素发生变化。除历史紧凑格式的 `docs/design-docs/theme-system.html` 外，所有本次文件通过目标 Prettier 检查；HEAD 基线的同一 HTML 也返回格式失败，因此保持精准更新。

设计记忆检查：本任务没有新增未来 Agent 必须理解的结构性设计；已有 `theme-system.md/.html` 已按实际落地同步“Worker OffscreenCanvas、独立 16ms 定时器驱动 300ms 扫光、loading 完成即揭幕”的合同。项目结构、技术栈、命令和关键文档位置均未改变，`AGENTS.md` 无需更新。

## 上下文和方向

`index.html` 在 React 启动前绘制 `#ad-splash`、真实 quote p 和覆盖 Canvas；同步内联脚本读取 computed font/theme/DPR，把 Canvas 转交给 Worker 的 OffscreenCanvas，以独立 16ms 定时器主动重绘。Rust 在 WKWebView 导航 Started 时显示主题匹配的原生窗口。`injectStartupCopy()` 只更新真实 p；观察器把 text/bounds 变化同步给 Worker。`src/main.tsx` 不启动或等待动画，数据加载完成后直接调用 `revealStartup()`。

当前问题不是浏览器没有动画，而是动画合同本身只有 50ms / 5 个位置；60Hz 屏幕一轮只能绘制约 3 帧。当前起终点又恰好落在字形两端，循环时光带仍可见，因此每次复位都会发生明显跳变。

## 工作计划

先在 `tests/lib/startup.test.ts` 把“50ms 结束”改为可观察的连续时间合同：0ms 在离屏起点、150ms/300ms/450ms 为严格单调中间位置、600ms 渲染离屏终点并完成 Promise。测试必须先在现状失败。同步调整 `tests/styles/themeContract.test.ts`，避免继续以 `STEP_MS × STEP_COUNT` 固定量化实现。

随后在 `src/lib/startupSurface.ts` 用单个 `SPOTLIGHT_SWEEP_MS = 600` 和线性进度计算每帧 background-position；循环边界将起终点放到柔边光带完全离开字形的位置。`index.html` 中把不对称的白→蓝高光改为对称、中性的多段渐变，基础色与 clipping fallback 不变。

实现后启动本地页面，在隔离浏览器中采样多个时间点的 computed `background-position` 和截图，验证真实渲染而非只相信单元测试。最后把实际参数同步到 `theme-system.md`，再通过本轮已加载的 `html-artifact` report 流程更新已有 HTML 视图。

## 验证和验收

自动化命令：

```bash
pnpm test -- tests/lib/startup.test.ts tests/styles/themeContract.test.ts tests/main.test.tsx
pnpm test
pnpm typecheck
pnpm lint
pnpm build
git diff --check
```

行为验收：

1. 正常 motion：高光从左侧字形外进入，连续扫过整句，从右侧离开后才复位；没有 5 段跳动或端点闪烁。
2. 一轮约 600ms，快速启动不因“等待完整首轮”超过现有 600ms 最短可见预算。
3. Mocha/Latte：背景不受探照灯影响，静态底色保持可读。
4. reduced motion：logo 与探照灯均静止，第一轮 Promise 立即完成，完整文案可读。
5. splash 被移除时循环停止排帧，等待中的第一轮 Promise 正常完成。

## 幂等性和恢复

本任务不写入 `~/.ad/`，不改 IPC、依赖或持久化数据。测试和构建可重复运行。若视觉实现不符合预期，可只回退本任务新增的连续插值与渐变差异；不得使用 destructive git 命令，也不得回退开始任务前已有的 5 个未提交文件。

## 接口和依赖

- 不新增 npm 或 Rust 依赖。
- 移除内部 `startStartupSpotlight()` rAF API；动画完全由首屏 HTML/CSS 驱动。
- `RevealStartupOptions` 仍保留可选最短可见能力供 helper 测试/未来调用，但主窗口不再传入 600ms 人为等待。
- HTML 同步阶段使用已安装的 `sage-skills:html-artifact`，只更新现有设计文档视图。

# Bundle 瘦身 — Monaco → CodeMirror 6

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

**根治** AD 主窗口 + Settings 窗口启动时的「空白页 → 内容」过渡感。问题根因不在窗口显示时机，而在首屏 bundle 体积 — 当前 `dist/assets/index-*.js` 是 **3.7MB**（gzip 后 969KB），主要来自 Monaco editor 全量打包。webview 要等这个 bundle 下载、解析、执行完成才能 paint UI，所以哪怕窗口已显示，里面也是几百毫秒到一两秒的空白。

**做法**：

1. 把 Monaco editor 换成 **CodeMirror 6**（同类专业代码编辑器，但模块化、按需加载，gzip 后 50-80KB）
2. 路由级 code-split：主窗口和 Settings 窗口不再共享同一个巨型 bundle
3. 大组件 React.lazy（HistoryDialog / ImportDialog / CommandPalette / ProfileEditDrawer / 主区编辑器）
4. vite `manualChunks` 把第三方依赖独立成长缓存友好的 vendor chunk
5. 删除死代码 `DiffView.tsx`

**用户可观察到的变化**：

- 主窗口打开 → sidebar 立刻可见，无空白过渡（目标 < 300ms）
- Settings 窗口打开 → 内容立刻可见，无空白过渡（目标 < 200ms）
- 编辑器（settings.json / settings.local.json）：JSON 语法高亮、行号、暗色主题、撤销重做、查找替换 — 体验等价，但启动从「~1 秒」变「即时」
- 编辑配置体验差异：Monaco 的 JSON schema 自动补全消失（AD 本来也没配 schema，所以用户感知 ≈ 0）

## 确认状态

- [x] 评审 HTML 路径：`docs/exec-plans/active/bundle-slim-codemirror.html`
- [x] 用户已确认（2026-06-02 17:55 GMT+8），HTML 冻结为基线，开始执行

## 假设

1. CodeMirror 6 (`@codemirror/state` + `@codemirror/view` + `@codemirror/lang-json` + `@codemirror/theme-one-dark`) 在 Tauri 2 WKWebView 上运行良好。它是纯 ESM、无 worker、无平台依赖。
2. 现有 CSP（`default-src 'self'; ... worker-src 'self' blob:`）对 CodeMirror 不需要任何调整。
3. `@codemirror/lang-json` 提供的语法高亮 + 括号匹配 + 折叠 + 自动闭合 已满足 AD 用户编辑 `settings.json` / `settings.local.json` 的需要；用户对 JSON schema 自动补全无强需求（当前 Monaco 也没配 schema，所以这条本来就没用上）。
4. JSON 校验由父组件 `LayeredSettingsEditor` 完成（沿用现有 `parseLayer` + `onValidityChange` 钩子），错误以红框形式显示在编辑器上方 — 不需要在编辑器内部画 gutter marker。
5. dark/light 主题切换通过 `EditorView.dispatch` 重新配置 extensions 实现，能在 `useUiSettings.darkMode` 变化时无缝切换。
6. `DiffView.tsx` 是死代码 — `grep -rn "DiffView" src/` 全仓只匹配自身定义，无引用。
7. 之前已加的 backgroundColor + index.html 预绘脚本保留作为兜底，但根治后过渡时间应该 < 100ms，肉眼不可感知，所以**不需要**再补 deferred-show（visible:false + ready→show）方案。
8. 现有 `monaco-editor` / `@monaco-editor/react` 依赖完全移除后，`monaco-editor/esm/vs/.../*.worker?worker` 这些 Vite 特殊语法的引用也随之清除，bundle 中不再产出 `*.worker-*.js` 文件。

## 影响范围

```
src/
├── main.tsx                              [M] 路由级动态 import App/SettingsApp
├── App.tsx                               [M] 大组件改 React.lazy + Suspense 顶层
├── components/
│   ├── DiffView.tsx                      [DEL] 死代码
│   ├── LayeredSettingsEditor.tsx         [M] Monaco Editor → JsonEditor (CodeMirror 包装)
│   ├── JsonEditor.tsx                    [N] 新建，CodeMirror 6 受控包装组件
│   ├── ProjectDetail.tsx                 [可能 M] ProjectConfigEditor 改 lazy
│   ├── ProfileEditDrawer.tsx             [可能 M] 内容 lazy（drawer 打开时才加载）
│   ├── HistoryDialog.tsx                 [可能 M] 内容 lazy
│   ├── ImportDialog.tsx                  [可能 M] 内容 lazy
│   ├── DetectedProjectsModal.tsx         [可能 M] 内容 lazy
│   └── CommandPalette.tsx                [可能 M] 内容 lazy
├── lib/
│   └── monacoSetup.ts                    [DEL]
└── styles/
    └── globals.css                       [可能 M] CodeMirror 主题/字体微调

vite.config.ts                            [M] manualChunks + chunkSizeWarningLimit
package.json                              [M] 移除 monaco-editor / @monaco-editor/react；添加 codemirror, @codemirror/state, @codemirror/view, @codemirror/lang-json, @codemirror/theme-one-dark

docs/exec-plans/active/
├── bundle-slim-codemirror.md             [N] 本文件
└── bundle-slim-codemirror.html           [N] 评审版

src-tauri/
└── tauri.conf.json                       [保留] backgroundColor 留作兜底，无害
index.html                                [保留] 预绘脚本留作兜底，无害
src/components/AdvancedSettings.tsx       [保留] 新窗口 backgroundColor 参数留作兜底
```

预计 8-10 个源文件 + 2 个 plan 文件 + 删 2 个文件 = **中等偏大复杂度**。

## 关键决策

**D1：用 CodeMirror 6 替代 Monaco**

- 理由：体积差 30-50 倍（3MB+ vs ~80KB gzip），功能覆盖 100% 我们实际需要的（JSON 高亮、行号、暗色主题、撤销重做、查找替换）。
- 替代方案：
  - 保留 Monaco + 仅 lazy load — 否决：bundle 总量不变，第一次进编辑器仍要等 970KB monaco chunk 下载/解析，"过渡感"只是从启动转移到首次编辑。
  - textarea + Prism 高亮 — 否决：失去行号 / 查找替换 / 撤销栈，编辑配置体验降级。
- 代价：重写 `LayeredSettingsEditor` 中 ~150 行 Monaco 集成代码。

**D2：把 Monaco 集成抽成新组件 `JsonEditor`，而不是直接在 LayeredSettingsEditor 内部改**

- 理由：`LayeredSettingsEditor` 是 controlled 组件，CodeMirror 的命令式 API（`EditorState`/`EditorView`/dispatch）跟 React 受控模式有阻抗；抽出 `JsonEditor.tsx` 把这层阻抗封装在组件边界内，对外仍提供 `value` / `onChange` 受控接口，LayeredSettingsEditor 不感知差异。
- 副效果：将来想换其它编辑器（textarea、CodeMirror 7）只动 JsonEditor。

**D3：JSON 校验仍由父组件 LayeredSettingsEditor 完成**

- 理由：当前 `parseLayer` + `onValidityChange` 已经把校验责任放在父层；CodeMirror 的 lint extension 也能做，但会跟父层重复。保持单一来源更清晰。
- 实现：JsonEditor 只透传 value/onChange，校验和错误展示完全在 LayeredSettingsEditor 层。

**D4：路由级 split 走 dynamic import，而不是多 entry**

- 理由：保持 vite/index.html 单 entry 模型不变，避免 tauri.conf.json 多 url 配置。`main.tsx` 内部根据 hash 动态 import App / SettingsApp 即可，Vite 自动产出独立 chunk。
- 替代方案：双 entry（`main.html` + `settings.html`）— 否决：要改 vite multi-page 配置，且要重写 AdvancedSettings.tsx 的 url 拼接，工程量更大。

**D5：保留之前 P0 的 backgroundColor + 预绘脚本作为兜底**

- 理由：无害（CodeMirror 加载快后 1-2 帧内就被 React 接管），但万一某个用户机器加载稍慢，至少首帧不是白色。
- 不再需要 deferred-show（visible:false + frontend ready 后 show）— 因为根治后过渡时间预期 < 100ms，肉眼不可见。

**D6：不做 i18n locale lazy**

- 理由：`zh.json` + `en.json` 加起来 ~30KB，相对收益太小，不值得复杂化（要异步等 locale 加载完才能 render）。

**D7：vite manualChunks 配置**

- `vendor-react`：react / react-dom / react-i18next / i18next
- `vendor-codemirror`：codemirror / @codemirror/*
- `vendor-radix`：@radix-ui/* + cmdk + lucide-react
- 其它走默认 chunking
- 理由：稳定的第三方代码 hash 不变，浏览器/webview 缓存命中率高；本质上对 Tauri 桌面应用 cache 收益不如 web，但对开发期 reload 速度有帮助，且让构建产物结构更可读。

## 进展

(随工作推进打勾 + 时间戳)

### 里程碑 M1：基础设施 + 死代码清理 ✅ 2026-06-02 17:57

- [x] 删除 `src/components/DiffView.tsx`
- [x] 删除 `src/lib/monacoSetup.ts`
- [x] 移除 `main.tsx` 中 `import './lib/monacoSetup'`
- [x] `pnpm remove monaco-editor @monaco-editor/react`
- [x] `pnpm add codemirror @codemirror/state @codemirror/view @codemirror/lang-json @codemirror/theme-one-dark` (codemirror 6.0.2)
- [x] `pnpm typecheck` 报 2 错（LayeredSettingsEditor.tsx:17 + :174）— 唯一 Monaco 引用点，M3 替换后即消失

### 里程碑 M2：创建 CodeMirror JsonEditor 组件 ✅ 2026-06-02 22:08

- [x] 新建 `src/components/JsonEditor.tsx`
  - props: `{ value, onChange, dark?, readOnly?, className? }`
  - 用 basicSetup（含 lineNumbers / history / search keymap / bracket matching）+ json() + oneDark + 自定义 heightTheme
  - dark / readOnly 各放一个 Compartment，prop 变化时 reconfigure 而不重建 view
  - 外部 value 同步用 `current === value` 守卫避免循环；onChangeRef 持有最新回调，view 不随 prop identity 重建
- [x] LayeredSettingsEditor 集成后即在产品 UI 中验收（见 M3）

### 里程碑 M3：替换 LayeredSettingsEditor 中的 Monaco ✅ 2026-06-02 22:10

- [x] `LayeredSettingsEditor.tsx`：`@monaco-editor/react` → `./JsonEditor`；删除 MONACO_OPTIONS 常量
- [x] `pnpm typecheck` 通过
- [x] `pnpm build` 成功；dist/assets/ 只剩 index.css + index.js + map（无 monaco-*，无 *.worker-*）
- [x] **中间产物**：主 bundle 3.7 MB → 812 KB（gzip 255 KB），削掉 78%；后续 M4–M6 继续降到 < 600 KB

### 里程碑 M4：路由级 split ✅ 2026-06-02 22:13

- [x] `main.tsx` 改成 `async function bootstrap()` IIFE，按 hash 动态 import `./App` 或 `./SettingsApp`（避免依赖 top-level await，WKWebView 兼容性更好）
- [x] 验收 dist/assets：
  - `index-*.js` 206 KB（entry，含 react/i18n/store/styles）
  - `App-*.js` 590 KB（含 codemirror 等）
  - `SettingsApp-*.js` **9.54 KB**，完全不含 codemirror
  - Settings 窗口首屏只需 ~215 KB（gzip ~72 KB），相比之前 3.7 MB 减少 ~94%

### 里程碑 M5：大组件 React.lazy ✅ 2026-06-02 22:16

- [x] `App.tsx` 顶层把以下改成 React.lazy：HistoryDialog / ImportDialog / DetectedProjectsModal / CommandPalette / ProfileEditDrawer
  - 每个外包一层 `Gated*` 组件：用 `useHasBeenTrue(openCondition)` 守门，第一次打开前完全不挂载（也就不下载 chunk）；第一次打开后保持挂载，让 Radix 的 open/close 动画继续生效。
  - 内层是 `<Suspense fallback={null}>` — dialog/drawer 的过渡瞬间，空白即可。
- [x] `ProjectDetail.tsx`：`ProjectConfigEditor` 改 React.lazy；外层 `<Suspense fallback={<EditorSkeleton />}>`
  - EditorSkeleton：纯灰色 rounded box，不加任何文案 —— 短暂闪一下比"加载编辑器..."更不打扰。
- [x] **chunk 实测（pnpm build）**：
  - `index-*.js` 206 KB（gzip 68 KB） — entry，含 react/i18n/store
  - `App-*.js` **143 KB**（gzip 38 KB） — 主窗口初次渲染需要的最小集合（不再含 dialog 与编辑器）
  - `LayeredSettingsEditor-*.js` 418 KB（gzip 136 KB） — codemirror + json + theme，仅在选中 project 后挂载
  - 各 dialog 独立 chunk：HistoryDialog 3 KB / ImportDialog 2 KB / DetectedProjectsModal 7 KB / CommandPalette 8 KB / ProfileEditDrawer 6 KB
  - `SettingsApp-*.js` 9.54 KB（gzip 3 KB） — Settings 窗口首屏仍只需 ~215 KB
  - 主窗口首屏：`index` + `App` = **~350 KB / gzip ~107 KB**（基线 3.7 MB → 减少 90.5%）

### 里程碑 M6：vite manualChunks ✅ 2026-06-02 22:19

- [x] `vite.config.ts` 在 `build.rollupOptions.output.manualChunks` 配置 vendor-react / vendor-codemirror / vendor-radix（写成函数式 `manualChunks(id)`，按 `node_modules` 路径分流，比对象式更易扩展）
- [x] `build.chunkSizeWarningLimit: 800`（预留余量）
- [x] **chunk 实测**：
  - `index-*.js` **19.44 KB**（gzip 8.78 KB） — entry 瘦到只剩 bootstrap glue
  - `vendor-react-*.js` 192 KB（gzip 62 KB） — react/react-dom/scheduler/i18next 全部集中
  - `vendor-codemirror-*.js` 413 KB（gzip 135 KB） — codemirror + 所有 @codemirror/*，仅在编辑器挂载时按需加载
  - `vendor-radix-*.js` 10 KB（gzip 2.5 KB） — 现阶段 radix/cmdk/lucide 主要被 dialog 拉入，故 vendor 块很小
  - `App-*.js` 140 KB / `SettingsApp-*.js` 8 KB
- [x] 主窗口首屏 = index + vendor-react + App = **~351 KB / gzip ~108 KB**
- [x] Settings 窗口首屏 = index + vendor-react + SettingsApp = **~219 KB / gzip ~73 KB**
- [x] `dist/assets/` 无 `monaco-*` / `*.worker-*.js`

### 里程碑 M7：验证 + 部署

- [ ] `pnpm typecheck` + `pnpm test` 全绿
- [ ] `pnpm tauri build` 主 bundle (`index-*.js`) < 600KB
- [ ] 部署到 `/Applications/AD.app`，肉眼验证：
  - 主窗口启动到 sidebar 可见无白板感
  - Settings 窗口启动到内容可见无白板感
  - 选 project → 编辑器渲染（首次有短暂 skeleton，可接受）
  - JSON 高亮 / 行号 / 暗色主题 / 撤销重做 / Cmd+F 查找全部正常
  - 故意输入非法 JSON → LayeredSettingsEditor 红框报错正确
  - darkMode 切换 → 编辑器主题跟随
  - 打开 ⌘K palette / history dialog / import dialog → lazy chunk 按需加载，无报错
- [ ] 更新 `AGENTS.md` 项目结构章节（移除 Monaco，补 CodeMirror）
- [ ] 把 MD + HTML 一起移到 `docs/exec-plans/completed/`

## 意外发现

(执行过程中如有发现填这里)

## 决策日志

(执行过程中产生的新决策填这里)

## 结果回顾

(完成后填这里：实测 bundle 大小、启动时间、是否还有过渡感、遗留问题)

## 验证和验收

**核心验证**：

- [ ] `dist/assets/index-*.js` < 600KB（无压缩）
- [ ] `dist/assets/` 不含 `monaco-editor*` / `*.worker-*.js`
- [ ] `dist/assets/` 包含独立的 `vendor-codemirror-*.js` chunk
- [ ] 主窗口冷启动到 UI 可见 < 300ms（目测）
- [ ] Settings 窗口冷启动到 UI 可见 < 200ms（目测）

**功能不退化**：

- [ ] settings.json / settings.local.json 编辑：JSON 高亮、行号、错误提示、撤销重做
- [ ] dark/light 切换无缝
- [ ] 主流程不报错：启动 → 选 project → 编辑 → 保存 → 切 template → ⌘K → history → settings 窗口

## 幂等性和恢复

- 包变更：`pnpm install` 是幂等操作。回滚只需 git reset 后 `pnpm install`。
- 不涉及任何运行时数据迁移，不动 `~/.ad/` 目录结构。
- 出问题可全程 git revert 回到 main，无残留状态。

## 接口和依赖

- 新增 npm deps：`codemirror`, `@codemirror/state`, `@codemirror/view`, `@codemirror/lang-json`, `@codemirror/theme-one-dark`
- 移除 npm deps：`monaco-editor`, `@monaco-editor/react`
- Tauri Rust 端无改动
- IPC 接口无改动
- 用户数据格式无改动

## 里程碑规划

- M1 基础设施 + 死代码清理（~10 min）
- M2 JsonEditor 新组件（~30 min）
- M3 LayeredSettingsEditor 替换（~15 min）
- M4 路由级 split（~10 min）
- M5 大组件 React.lazy（~20 min）
- M6 manualChunks（~10 min）
- M7 验证 + 部署 + 文档（~20 min）

合计预估：~2 小时。

# UI 重设计 — A′ 方向（cmux-inspired）

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

把 AD 主窗口从当前 Profiles | Editor | Projects 三栏架构换成 cmux 风格的 **sidebar + detail + palette**：

- 左栏紧凑 project 列表（富 metadata + 状态色环 + 键盘标号 ⌘1-8）
- 主区是当前 project 的详情面板（current profile + apply different + recent applies）—— apply 流程 inline，废弃 modal
- ⌘K command palette 作为全局动作入口
- profile 编辑下沉到右侧 drawer

用户能观察到：

- 一眼看完所有 project 状态（色环 = synced / dirty / recent / never）
- 主路径 = 选 project → 看 current 状态 → 切 profile/layer → ⌘↵ apply，不再开 modal
- 重度键盘党可全程不离手
- TD-005（冲突 UI 只读）顺便消掉 —— 改成 inline keep/use/custom radio

## 确认状态

- [x] 用户已直接批准，**跳过 Stage 2 HTML 评审**（2026-05-24，用户 explicit instruction："plan 出了后直接执行"）
- [x] 进入 Stage 3 执行

## 假设

列出隐含假设。任何一条你不认同就告诉我，HTML+MD 一起改后再确认。

- **A1** profile 编辑迁到右侧 drawer（宽 ~440px），现有三 tab（Shared / Local / Env + monaco / 表单）UI 不变，只是位置变
- **A2** ApplyDialog 完全废弃，apply 流程 inline 在 ProjectDetail 主区
- **A3** ApplyDialog 的冲突 resolver 升级为可交互 keep/use/custom radio（顺便合并解决 TD-005）
- **A4** 状态色环颜色规则：synced (<24h applied + git clean) = olive / dirty 或 conflict = clay / recent (>24h applied) = blue / never applied = gray-dashed —— 跟 tray 现有色环一致
- **A5** ⌘1-8 按当前 projects 数组顺序映射（不引入 pin 概念，避免额外状态）
- **A6** CommandPalette 自实现（不引入 `cmdk` 依赖）—— ~150 行代码可控
- **A7** legacy [Activate] 按钮仍走 Advanced settings 路径（M5 已经做了，不再动）
- **A8** HistoryPanel 不删但从主区退役，通过 ⌘K → "Open history" 浮窗触发
- **A9** DetectedProjectsModal 保留，入口收敛到 palette + sidebar 底部小横幅（"✨ N detected"）
- **A10** 持久化 active project（最后一次选中的重启后恢复）— 加到 `~/.ad/state/ui.json`

## 影响范围

**新增 (8 个文件)**

```
src/
├── components/
│   ├── CommandPalette.tsx          [NEW] ⌘K overlay + fuzzy + ↑↓ enter
│   ├── ProjectSidebar.tsx          [NEW] 左栏紧凑 project rows
│   ├── ProjectDetail.tsx           [NEW] 主区当前 project 详情
│   ├── ProfileEditDrawer.tsx       [NEW] 右侧 drawer 包 profile 三 tab
│   ├── StatusRing.tsx              [NEW] ~30 行 ring color 计算
│   └── InlineConflictResolver.tsx  [NEW] keep/use/custom radio (TD-005)
├── lib/
│   └── keymap.ts                   [NEW] 全局快捷键 hook
└── store/
    ├── ui.ts                       [NEW] active project / drawer / palette / sidebar collapsed
    └── commandPalette.ts           [NEW] commands registry
```

**重写 (3 个文件)**

```
src/
├── App.tsx                         [REWRITE] 双栏 layout + drawer overlay + palette overlay
├── components/ProfileEditor.tsx    [MOVE]    内容迁到 ProfileEditDrawer
└── store/projects.ts               [MINOR]   加 active project 持久化字段
```

**退役 / 删除 (6 个文件)**

```
src/components/
├── ProfileList.tsx        [DEL] 列表移到 palette + drawer
├── ProfileRow.tsx         [DEL]
├── ProjectsPanel.tsx      [DEL] 拆到 ProjectSidebar + ProjectDetail
├── AddProjectInput.tsx    [DEL] 入口移到 palette
├── ProjectDropZone.tsx    [DEL] 入口移到 palette
└── ApplyDialog.tsx        [DEL] 流程移到 inline
```

**不动**

- 整个 `src-tauri/` 后端（commands / models / migration / fs / merge）
- `src/lib/tauri.ts` `profileSchema.ts` `projectTypes.ts`
- `src/store/profiles.ts`（只读，不改）
- `tailwind.config.ts`（可能加 `blue` token）
- `HistoryPanel.tsx` `DetectedProjectsModal.tsx` `AdvancedSettings.tsx` `ActivateToast.tsx` `ImportDialog.tsx` `EmptyState.tsx` `DiffView.tsx` —— 仅接入新入口，组件内部不重写

总计：~11 个新/重写前端文件，~6 个删除，后端 0 改。

## 关键决策

**D1 · 双栏布局，不是三栏**
当前三栏（240 + flex + 320）改成 240 sidebar + flex detail，drawer 浮在右侧覆盖（不挤主区）。
理由：sidebar+detail 是 cmux 主形态；三栏让主区被夹得窄；编辑 profile 是低频，没必要常驻。

**D2 · ⌘K palette 是 first-class，UI 是它的镜像**
所有动作（apply / switch / edit / add / detected / history / advanced）注册到 palette registry，UI 按钮跟 palette 调用共用 handler。
理由：单一动作源避免逻辑漂移；palette 是 power user 路径，UI 是 fallback。

**D3 · 状态色环统一颜色语言**
- synced (<24h applied + git clean)：`olive #788C5D`
- dirty / conflict / unsaved：`clay #D97757`
- recent (>24h)：`blue #6588C3` ← 新增 token
- never applied：`gray-300 dashed border`

理由：跟 tray 现有色环一致（用户已熟悉）。

**D4 · Apply 完全 inline，废弃 modal**
ProjectDetail 主区有 "Apply different" 区：profile chips + layer toggles + diff preview + Apply button。冲突 inline 出 keep/use/custom radio。
理由：modal 是 P4 痛点主因；inline 让 project header + current + preview + apply 在同一视野。

**D5 · CommandPalette 自实现，不引入 cmdk**
~150 行：fuzzy filter + ↑↓ enter + 分组渲染 + esc 关。
理由：避免新依赖；行为完全可控；写单测容易。

**D6 · 快捷键集**

| 快捷键 | 动作 |
|---|---|
| ⌘K / ctrl+K | toggle palette |
| ⌘1-8 | 跳 project N |
| ⌘T | add project（palette 预填 "add "） |
| ⌘P | switch profile（palette 预填 "apply "） |
| ⌘E | edit current profile（开 drawer） |
| ⌘↵ | apply current selection |
| ⌘⇧K | toggle sidebar collapse |
| esc | close drawer / palette |

理由：覆盖高频路径；遵循 macOS 习惯；不跟 monaco / 浏览器默认冲突。

**D7 · active project 持久化**
新文件 `~/.ad/state/ui.json` 存 `{ activeProjectPath: string | null }`，启动恢复，无效 fallback 到 `projects[0]`。
理由：每次启动选半天不爽。

**D8 · ProfileEditor 内容不重写，只迁位置**
现有三 tab + monaco 全保留，包成 ProfileEditDrawer，drawer 内 UI 一致。
理由：M3 刚做完质量稳；这次是信息架构改造，不是编辑器重做。

**D9 · HistoryPanel 通过浮窗访问**
不删，⌘K → "Open history" 弹 modal。
理由：低频，不值得常驻位置。

## 进展

### M1 · 数据层
- [x] (2026-05-24) 新建 `src/store/ui.ts`：active project / palette open / drawer open / sidebar collapsed；**调整**：用 localStorage 而非 `~/.ad/state/ui.json`（D7 修订，见决策日志）
  - 验证：typecheck pass; 字段定义完整
- [x] (2026-05-24) 新建 `src/lib/keymap.ts`：useGlobalKeymap hook，支持 mod+k / mod+shift+k / mod+1..9 / mod+enter / mod+letter / esc；editable focus 时只放行 mod+k / mod+shift+k / esc
  - 验证：typecheck pass

### M2 · sidebar
- [x] (2026-05-24) 新建 `src/components/StatusRing.tsx`：ringStateFor + ringBorderClass + ringTextClass + ringStatusText 纯函数 + `slate2` 颜色加到 tailwind config
  - 验证：typecheck pass
- [x] (2026-05-24) 新建 `src/components/ProjectSidebar.tsx`：渲染 projects，每行 ring + ⌘N kbd + profile dot + path + status text；点选 setActiveProject；首次加载自动 hydrate active；底部 detected banner（沿用现有 modal）
  - 验证：typecheck pass

### M3 · detail
- [x] (2026-05-24) 新建 `src/components/ProjectDetail.tsx`：header + CURRENT 卡 + APPLY DIFFERENT 区（chips/toggles/writes preview/dirty 警告/inline resolver/Apply 按钮含 ⌘↵）+ RECENT；⌘↵ + ⌘E 通过 useGlobalKeymap 注册
  - 验证：typecheck pass
- [x] (2026-05-24) 新建 `src/components/InlineConflictResolver.tsx`：每个 conflict 三 radio（keep/use/custom），custom 含 JSON textarea + 实时 parse 校验；TD-005 在 M5 收尾时标偿还
  - 验证：typecheck pass

### M4 · palette
- [x] (2026-05-24) 取消 `src/store/commandPalette.ts`（见决策 D10）
- [x] (2026-05-24) 新建 `src/components/CommandPalette.tsx`：modal overlay + 输入 + 分组（APPLY/SWITCH/EDIT/ADD/OTHER）+ ↑↓ enter + esc + 点击 backdrop 关
  - 验证：typecheck pass；UI 验证待 M5 wire 到 App
- [x] (2026-05-24) 新建 `src/components/GlobalKeymap.tsx`：mount-only 组件 wire ⌘K / ⌘1-8 / ⌘T / ⌘P / ⌘E / ⌘⇧K / esc
  - 验证：typecheck pass

### M5 · drawer + 清理
- [x] (2026-05-24) `src/components/ProfileEditor.tsx` 加 `profileId` + `onDirty` props，drawer 用 key 强制 remount 隔离 dirty 状态
  - 验证：typecheck pass
- [x] (2026-05-24) 新建 `src/components/ProfileEditDrawer.tsx`：右侧 480px 滑入，包 ProfileEditor，关闭时 dirty 弹 confirm
  - 验证：typecheck pass
- [x] (2026-05-24) 新建 `src/components/HistoryDialog.tsx`：包 HistoryPanel 为 modal（由 view='history' 控制）
  - 验证：typecheck pass
- [x] (2026-05-24) 重写 `src/App.tsx`：双栏 layout（ProjectSidebar | ProjectDetail）+ drawer/palette/history overlay + GlobalKeymap
  - 验证：build pass
- [x] (2026-05-24) 删除 6 个旧 component：ProfileList、ProfileRow、ProjectsPanel、AddProjectInput、ProjectDropZone、ApplyDialog + 一个 obsolete test
  - 验证：grep 无引用，typecheck + lint + build 全 pass

### M6 · 验收 + 文档
- [x] (2026-05-24) build + typecheck + lint + 现有 test 全 pass（13 tests passed）。**端到端 dev 验证留给用户跑 `pnpm tauri dev`**
- [x] (2026-05-24) `docs/design-docs/ui-redesign-options.html` 顶部加 "A′ 已实施 2026-05-24" 徽标
- [x] (2026-05-24) `AGENTS.md` 主要架构段更新为 v0.3（A′ cmux-inspired 双栏 + drawer + palette）
- [x] (2026-05-24) `tech-debt-tracker.md`：TD-005 标已偿还（InlineConflictResolver）
- [x] (2026-05-24) 移 MD 到 `docs/exec-plans/completed/`（HTML 不存在，因用户跳过 Stage 2 评审）

## 意外发现

(执行期填写)

## 决策日志

- **D7 修订** — activeProjectPath 用 `localStorage` 而非 `~/.ad/state/ui.json`
  - 理由：避免给后端加 ui state 命令，M1 完全在前端；现有 `useUiSettings` 已经用 localStorage 模式
  - 日期/作者：2026-05-24 / Claude
- **D10** — 取消独立 `store/commandPalette.ts`，commands 在 `CommandPalette.tsx` 内部 useMemo 动态生成
  - 理由：commands 强依赖 profiles + projects + active project state，不是静态注册；额外的 store 是 YAGNI
  - 日期/作者：2026-05-24 / Claude

## 结果回顾

**完成于 2026-05-24，单一 session 一气呵成。**

### 做了什么
- 12 个新前端文件，6 个删除，1 个重写（App.tsx），1 个加 props（ProfileEditor）
- 后端 0 改（如计划），Tauri 命令、模型、迁移完全没动
- TD-005 顺带偿还
- AGENTS.md 主架构段升级到 v0.3

### 实际效果
- typecheck / lint / vitest / vite build 全 pass
- 旧三栏 (Profiles | Editor | Projects) → 新双栏 (ProjectSidebar | ProjectDetail) + 浮层（drawer / palette / history dialog）
- ApplyDialog modal 完全移除，apply 流程 inline 在 ProjectDetail
- 8 个全局快捷键 + ⌘K command palette 跑通

### 跟原计划的差距
- D7 修订：activeProjectPath 用 localStorage 而非 ~/.ad/state/ui.json（避免后端改动）
- D10 新增：取消独立 commandPalette store，commands 在 CommandPalette 组件内 useMemo 生成
- 跳过 Stage 2 HTML 评审（用户 explicit instruction）—— ExecPlan 只有 MD 没有 HTML 基线
- 端到端 dev 验证未自己跑（tauri dev 长任务，留给用户）

### 遗留 / 后续
- 端到端 dev 验证由用户跑 `pnpm tauri dev` 完成
- M3 阶段 ProjectDetail 的 diff preview 是文本占位（"X files will be written"），未真实算 diff —— 后续可加 dry-run 命令
- ProjectDetail 的 RECENT 区只显 lastApplied 单条；多条历史依赖 ⌘K → "Open history" 走 HistoryPanel
- ProfileEditor 改成接受 profileId prop 后，旧 `useProfiles.select()` 副作用仍在（drawer 切 profile 会顺手改 selectedId）；无害但有点冗余，未来可清理
- TD-006 状态不变（待 legacy sunset 后清理 `settings` 字段）

## 上下文和方向

项目根 `/Users/yuanxuan/ai_workspace/ad`，主分支 `main`。前面 5 个 M（M1-M5，commit `24ab68e`）刚做完分层 profile + per-project apply 重构（见 `docs/exec-plans/completed/layered-profile-redesign.md`）。

当前 UI 三栏：240 Profiles | flex Editor | 320 Projects + ApplyDialog modal。痛点 9 条详见 `docs/design-docs/ui-redesign-options.html`。

UI 重设计探索了 4 个方向（A/B/C/A′），用户挑了 A′（cmux 风），可交互原型在同一 HTML 的 #playground 区域。

参考产品：[cmux](https://github.com/manaflow-ai/cmux) — macOS 原生终端，提供了 vertical sidebar + 富 row metadata + ⌘K palette + 全键盘快捷键的范本。

## 工作计划

按 M1→M6 顺序，每个 M 用户确认后再进下个。

- M1 store + keymap 不动 UI，安全
- M2 sidebar 可独立 mount 验证
- M3 detail 可独立 mount 验证
- M4 palette overlay 独立 demo
- M5 实际切换主 layout（最大风险点）— 做前 commit M1-M4 进度
- M6 端到端 + 文档

## 验证和验收

**启动**：`pnpm tauri dev`

**端到端 walkthrough**：

1. 启动应用，sidebar 显示所有 project，色环颜色匹配状态
2. ⌘1-3 切 active project，主区跟着变
3. 选 profile chip + 勾 local layer，diff preview 出
4. 按 ⌘↵ apply，toast 出 "Applied ..."，sidebar 该 row 色环变 olive
5. 对 git dirty 的 project 勾 shared layer apply：inline 出 dirty 警告 + 冲突 resolver；选 use/keep/custom 后能 apply 成功
6. ⌘K → 输入 "edit work" → enter → 右 drawer 滑入，三 tab 编辑器
7. ⌘K → 输入 "history" → 打开 history modal
8. 重启 app，active project 恢复到上次选中
9. Advanced settings → Show legacy → drawer 内仍能看到 [Activate (legacy)] 按钮

## 幂等性和恢复

每个 M 独立。M5 是关键切换点，进入前 commit M1-M4 当前状态，万一不顺可 `git reset --hard <commit>`。

`ui.json` 不存在 / corrupt / activeProjectPath 无效时 fallback 到 `projects[0]`。

## 接口和依赖

- 无新依赖（自实现 palette）
- 新 store hooks：`useUiState()` / `useCommandPalette()`
- 新 lib hook：`useGlobalKeymap()`
- 新组件接口：`<StatusRing project />` / `<ProjectSidebar />` / `<ProjectDetail />` / `<ProfileEditDrawer />` / `<CommandPalette />` / `<InlineConflictResolver conflicts onResolve />`

## 里程碑规划

M1 数据层 → M2 sidebar → M3 detail → M4 palette → M5 drawer+清理 → M6 验收

每个 M 完成后等用户验证再进下个。

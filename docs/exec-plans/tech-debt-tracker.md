# 技术债务追踪

记录 AD 已知的技术债务、优先级和计划解决方案。技术债务不可避免，关键是有意识地管理它。

## 技术债务定义

为短期目标采取的非最优解决方案所产生的未来成本。三个原则：

1. **有意识**：知道自己在产生债务
2. **有计划**：知道何时偿还
3. **有记录**：不会遗忘

---

## 当前技术债务

### 高优先级

| 编号 | 描述 | 影响 | 计划解决 | 相关 ExecPlan |
|---|---|---|---|---|
| TD-006 | M3 layered profile 编辑器保存时通过 `settingsFromLayers` 同步老 `settings` 字段。如果用户在 legacy 模式下直接 Activate，settings 是 M3 保存时算出的平铺，可能与 layers 实时不同步（如果 layers 有未保存改动） | 低：legacy 路径将在 M5 sunset 后路径，无人会用 | M5 完成后实际可移除整个 settings 字段 + 删 settingsFromLayers | — |

### 中优先级

| 编号 | 描述 | 影响 | 计划解决 |
|---|---|---|---|
| TD-002 | `tauri.config.json` CSP 含 `unsafe-eval`（monaco-editor 需要） | 安全面增大 | 评估是否能换走 monaco / 用 worker 隔离 |
| TD-003 | `write_atomic` 不 fsync 父目录，仅 APFS 安全 | 跨 FS 移植性差 | 跨平台前重新评估 |

### 低优先级

| 编号 | 描述 | 影响 | 计划解决 |
|---|---|---|---|
| TD-004 | 旧 `history.jsonl` 单文件兼容读路径仍在 | 代码复杂度 | 老用户全部迁移完后移除 |
| TD-007 | `ProfileEditor` 与 `HistoryPanel` 未接 i18n（约 40 + 15 个 keys）。`HistoryDialog` 包装层已接 i18n，但内部 `HistoryPanel` 硬编码英文 | 中文用户在这两处仍看到英文 | UI Polish Pass 2 — 先设计 profile 字段术语表（中英对照），再批量替换 |
| TD-008 | `pnpm lint` 在 main 上长期不通过：`ProjectSidebar.tsx:252` 用了 `React.ReactNode` 但未 import `React`（no-undef error）；`CommandPalette.tsx:199` useMemo 缺 `openPalette` 依赖。两处均为 v0.3 重设计期遗留，per-project-config-model 重构期发现 | 低：CI 没把 lint 卡死，但 `pnpm lint --max-warnings=0` 命令对开发者直接无效 | 单独清理一轮，并把 lint 加入 pre-push 钩子防回归 |

---

## 处理流程

发现新技术债务时：

1. 添加到本表，分配 `TD-NNN` 编号
2. 评估优先级（影响范围 × 解决难度）
3. 在相关代码处加 `// TODO(TD-NNN): ...` 注释
4. 解决时一并删除注释 + 从本表移除（移到下面"已偿还"节）

---

## 已偿还

| 编号 | 描述 | 偿还时间 | 相关提交 |
|---|---|---|---|
| TD-001 | profile 激活策略覆盖全局 `~/.claude/settings.json` | 2026-05-23 | 分层 profile 重构 ExecPlan（M1-M5）— 默认走 per-project apply，全局覆盖路径藏到 Advanced。详见 `docs/exec-plans/completed/layered-profile-redesign.md` |
| TD-005 | ApplyDialog 冲突 UI 只读 | 2026-05-24 | UI 重设计 A′ ExecPlan 中由新组件 `InlineConflictResolver.tsx` 解决：每个 conflict 三 radio（keep existing / use incoming / custom JSON），重 invoke apply 带 resolutions。详见 `docs/exec-plans/completed/ui-redesign.md` |

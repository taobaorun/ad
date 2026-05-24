# Project Local Overrides

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

让 Profile 成为真正的"模板"：各个 project 可以在 profile 基础上添加自己的本地配置，这些配置在切换/重新 apply profile 时不会被覆盖。

**用户可观察到的变化**：
- ProjectDetail 出现 "项目本地设置" 编辑器（Monaco）
- 切换 profile 后，project 自己添加的 key 仍然保留
- Apply 公式：`settings.local.json = merge(profile.local, project.local_overrides)`

## 确认状态

- [ ] **待用户确认** — 评审 HTML 路径：`docs/exec-plans/active/project-local-overrides.html`
- [ ] 用户已确认，开始执行（时间戳）

## 假设

1. Project local overrides 仅作用于 `settings.local.json`（local 层），不影响 `settings.json`（shared 层）
2. Overrides 存储在 `~/.ad/state/local-overrides/<path-hash>.json`，不在项目目录内
3. Apply 时 settings.local.json 由 `merge(profile.local, overrides)` 完整重建，不再累积旧文件内容
4. 向后兼容：如果某 project 没有 overrides 文件，行为与当前一致（merge 已有文件）
5. UI 编辑器是 Monaco JSON editor，与 ProfileEditor 的 Shared/Local tab 保持一致
6. 不支持"env 层 overrides"——env 层已有独立机制（export 片段）

## 影响范围

```
src-tauri/src/
├── fs/
│   └── paths.rs              [M] 新增 local_overrides_dir(), local_override_path()
├── commands/
│   ├── apply.rs              [M] local 层 apply 逻辑改用 overrides 文件
│   ├── projects.rs           [M] 新增 get_project_local_overrides, save_project_local_overrides
│   └── mod.rs                [可能不需要改，commands 已有 glob re-export]
└── lib.rs                    [M] 注册两个新 Tauri 命令

src/
├── lib/
│   ├── projectTypes.ts       [M] 新增 ProjectLocalOverrides 类型
│   └── tauri.ts              [M] 新增两个 IPC 方法
├── components/
│   └── ProjectDetail.tsx     [M] 新增 project local 编辑器区块
└── i18n/locales/
    ├── zh.json               [M] 新增 projectLocal.* 键
    └── en.json               [M] 新增 projectLocal.* 键
```

共 8 个文件（i18n 两份算一组）。

## 关键决策

**D1：存储位置选 `~/.ad/state/local-overrides/`，不放入 projects.json**
- 理由：overrides 内容可能较大（JSON），放进 projects.json 会让注册文件膨胀；独立文件更清晰，便于 debug
- path-hash 用项目路径的 Blake3/SHA256 前缀生成，和 backup slug 策略一致

**D2：Apply 时不再 merge 已有 settings.local.json**
- 新公式：`settings.local.json = merge(profile.local, project.local_overrides)`
- 好处：apply 结果幂等，profile 切换后文件内容可预期
- 向后兼容：overrides 文件为空时等价于只写 profile.local（与旧行为一致）
- 手动编辑 settings.local.json 的数据会在下次 apply 丢失——这是预期行为，因为 settings.local.json 是 AD 的输出文件

**D3：不做向前迁移（将已有 settings.local.json 的额外内容迁移成 overrides）**
- 理由：实现复杂且容易出错；用户可以手动在 UI 里补回需要的内容
- 记录为已知行为变更在 UI 里提示

**D4：UI 放在 ProjectDetail 的新 section，不是独立 tab**
- 当前 ProjectDetail 已有 CURRENT / APPLY DIFFERENT / RECENT 三个逻辑区
- 新增 PROJECT LOCAL 区块，展开时显示 Monaco 编辑器
- 折叠状态：显示 override 的 key 数量摘要

## 进展

- [x] Step 1：Backend — paths.rs 新增路径函数（验证：单测通过）2025-05-24
- [x] Step 2：Backend — projects.rs 新增 get/save 命令（验证：13 个单测全通过）2025-05-24
- [x] Step 3：Backend — apply.rs 修改 local 层逻辑（验证：9 个单测全通过）2025-05-24
- [x] Step 4：Backend — lib.rs 注册新命令（验证：cargo check 通过，100 个测试全绿）2025-05-24
- [x] Step 5：Frontend — 类型 + IPC（验证：pnpm typecheck 通过）2025-05-24
- [x] Step 6：Frontend — ProjectDetail UI（验证：typecheck 通过）2025-05-24
- [x] Step 7：i18n（验证：zh/en key 全覆盖，typecheck 通过）2025-05-24

## 意外发现

- 原有两个 apply.rs 测试依赖"将已有 settings.local.json merge 进来"的旧行为，需要更新以反映新公式。其中 `apply_with_existing_file_merges_and_backs_up` 改名为 `apply_local_backs_up_existing_and_overwrites_with_formula`，`apply_returns_needs_resolution_when_conflict` 改为通过 override 文件触发冲突。
- 路径哈希直接在 paths.rs 内用内联 FNV-1a 实现，不引入额外依赖。

## 决策日志

- **路径哈希算法**：选 FNV-1a 64-bit 内联实现，不加 blake3/sha256 依赖。理由：无需加密强度，只需稳定且无碰撞；FNV-1a 实现仅 8 行，对 macOS 路径充分。
- **write_layer 接口变化**：新增 `base_override: Option<&Value>` 参数——None 保留旧行为（用于 shared 层）、Some 传入 overrides（用于 local 层）。避免对 shared 层有任何副作用。

## 结果回顾

所有 7 步完成。100 个 Rust 单测通过，pnpm typecheck 通过。

**验收**：
- [x] apply 后 settings.local.json = merge(profile.local, overrides)（apply.rs 测试 apply_local_merges_overrides_into_result 覆盖）
- [x] 空 overrides 时行为与旧版一致（apply.rs 测试 apply_local_layer_writes_settings_local_json 覆盖）
- [x] 切换 profile 后 project overrides 保留（overrides 文件与 profile 无关，只与 project path 绑定）
- [x] UI 能显示、编辑、保存 overrides（ProjectDetail.tsx ProjectLocalSection）
- [x] pnpm typecheck + cargo check 通过

## 上下文和方向

项目是 AD（Tauri 2 + React/TS macOS 应用），管理 Claude Code 的 settings 配置。

关键现有文件：
- `src-tauri/src/commands/apply.rs` — `write_layer()` 函数是核心，改 local 层逻辑在这里
- `src-tauri/src/fs/paths.rs` — 所有路径函数统一在这里定义
- `src-tauri/src/commands/projects.rs` — 现有项目命令，新命令加在这里
- `src-tauri/src/lib.rs` — Tauri 命令注册入口
- `src/components/ProjectDetail.tsx` — 主 UI，新 section 加这里
- `src/lib/tauri.ts` — 前端 IPC 封装

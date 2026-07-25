# 项目 Codex 配置继承开关

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。实施契约见 `docs/plans/2026-07-25-001-feat-project-codex-inheritance-plan.md`。

## 目的 / 全局视角

现在项目派生 `CODEX_HOME` 总会结构化继承 Base `~/.codex/config.toml`。用户需要对每个项目选择是否继承：默认开启以保持兼容；关闭后只保留 AD 必需字段与 Project Plugin overlay，Base auth 复用保持独立。切换必须通过 Preview → Apply，不能点击后立即改写运行时。

这项工作还修复一个数据边界：当前生成后的 config 已无法区分 Base 与 Project 条目。新增版本化 runtime manifest，记录 applied policy、profile 和明确的 Project overlay，使 on→off 可以安全收敛并由既有 ExecutionEngine 回滚；既有 Base 镜像保留为不活跃缓存，不做高风险物理删除。

## 确认状态

- [x] 评审 HTML 路径：`docs/exec-plans/completed/codex-config-inheritance-toggle.html`
- [x] 用户通过 `$compound-engineering:lfg 实现` 授权自动执行（2026-07-25 12:31 GMT+8）；HTML 冻结为基线，开始执行

## 影响范围

- Project 持久化模型、setter、TypeScript schema/store
- Codex derived runtime state、版本化 manifest、freshness 与 terminal gate
- conversion Preview/Apply、config synthesis、Plugin marketplace/package 镜像启停
- ProjectDetail 开关、conversion dialog/profile 边界、Apply progress、zh/en i18n
- Rust/TypeScript 集成测试、架构文档、生产构建与本地安装

## 关键决策

1. Project 保存 desired `inheritBaseConfig`，derived runtime manifest 保存 applied 策略；状态比较二者。
2. runtime manifest 作为标准 MutationPlan 资源参与 backup/rollback，不在 Preview 直接写盘；Preview 使用纯内存 runtime descriptor。
3. on 复用 `synthesize_project_codex_config(Some(base), ...)`；off 复用 `None` seam。
4. off 模式不物理删除既有缓存，只保证 config 不引用 Base 内容且停止新增 Base 镜像。
5. Base auth 检测与复用不受继承开关控制。
6. off 模式不支持 Base profile；UI 清空/禁用，后端拒绝矛盾请求。
7. Codex 原生 `<project>/.codex/config.toml` 始终只读且保持高优先级。

## 进展

- [x] 2026-07-25 12:31 完成仓库研究、Agent 原生评估与方案收敛
- [x] 2026-07-25 12:31 创建 implementation-ready 统一计划与冻结 HTML 基线
- [x] 2026-07-25 12:46 完成无交互文档审查；收敛缓存删除范围，补齐首次 Preview descriptor、单一 manifest finalization、legacy 冲突、写入口盘点、安全边界和 UI 状态矩阵
- [x] U1 项目期望设置与类型化 IPC
- [x] U2 版本化 runtime manifest 与策略感知 freshness
- [x] U3 配置合成、Project overlay、迁移与事务收敛
- [x] U4 status、terminal gate 与 profile 边界
- [x] U5 ProjectDetail/Conversion UX 与 Apply loading
- [x] U6 文档、全量测试、生产构建与本地安装
- [x] 代码简化与多维代码审查；9 个有效问题已修复并完成定向回归
- [x] 2026-07-25 15:02 修复 legacy 禁用 Plugin 被误报为所有权冲突；禁用条目及其无活跃引用 marketplace 在首次迁移时收敛清理，启用且来源不明的条目继续 fail closed
- [x] 2026-07-25 21:34 完成全部 GitHub Codex Review 闭环；累计 54 个线程全部补回归、修复、逐条回复并解决，最终代码树复审无新增建议
- [x] 浏览器测试已执行到驱动门禁；内置浏览器无可用会话且 `agent-browser` 未安装，按 pipeline 规范记为 Skip
- [x] 精确提交、PR 与 CI；PR #10 已通过 Rebase and merge 合入 `main` 为 `05cceae`，压缩后的精确代码树前后端 GitHub CI 全绿
- [x] 2026-07-25 21:49 最终生产构建安装并验证；替换前应用保留在 `/tmp/AD.app.pre-final-05cceae-20260725-214849`
- [x] MD + HTML 移到 `docs/exec-plans/completed/`

## 意外发现

- 当前 `project_overlay_from_config` 会把完整派生配置中的 Base 与 Project 条目混在一起，不能作为关闭继承后的 overlay 来源。
- ExecutionEngine 已支持 digest-protected Delete、backup、compensation 和 rollback；manifest 进入 MutationPlan 后无需另建事务系统。
- Preview 当前会提前持久化 `profile_id`，与“Preview 无副作用”冲突，本次需一并改为 applied manifest 驱动。
- 现有首次 Preview 依赖提前写 runtime registry；需改为纯内存 descriptor，否则无法同时满足零写盘与 bootstrap discovery。
- 多 Plugin 子计划不能各自写 manifest；必须在 conversion route 合并后唯一 finalization。
- 首轮代码审查发现 legacy runtime 无 manifest 时不能从已合并 config 猜测 Plugin 所有权；迁移现改为只接受明确 Project 来源，其余歧义 fail closed。
- 真实运行时中的 `claude-mem@thedotmack` 为 `enabled = false`，但旧校验仍要求证明其 Project 所有权，并继续校验只被禁用条目引用的 marketplace；这会产生无 UI 解法的伪冲突。
- status/terminal 在 registry cache 缺失时原本可能退回 Base `CODEX_HOME`；现从 Base+Project 确定性派生 descriptor，未准备或未注册的运行时均 fail closed。
- Preview/runtime inspect 都存在异步旧结果覆盖新 Project/新检查的竞态；现以 generation/request id 丢弃过期响应。
- Apply 期间 footer 虽禁用，但标题栏 Close 与 Escape 仍可关闭；`Dialog.closeDisabled` 补齐了完整操作锁。
- `ce-test-browser` 无法获得内置浏览器，且备用 `agent-browser` 未安装；技能禁止临时引入第三套浏览器栈，因此不把自动化测试通过冒充为真实 UI 验收。
- 初次全仓格式门禁暴露历史差异；在隔离保存用户未提交改动后完成代码格式收敛，并用 `.prettierignore` 排除生成物/长文档，最终 `pnpm format:check` 与 `cargo fmt --check` 均全仓通过。
- GitHub 后端 CI 的干净 runner 没有 `dist/`，Tauri `generate_context!` 在 `cargo test` 编译期失败；CI 现显式创建空的前端目录，本地移走既有 `dist/` 后完整复现验证通过。
- 第二轮 GitHub 复审发现 Settings 异步响应、Claude Skill 作用域、目录原子替换提交点和只读目录复制顺序四个边界；均以定向故障/竞态测试固定。
- 后续 GitHub 复审发现 Project runtime 选择需携带规范项目路径、Claude Profile 新旧表示需双向兼容、Project Skill 所有权需写入 manifest、成功操作需保留回滚入口，以及无 Base read-set 的写入不得更新继承摘要；均已补回归并修复。
- 首次最终 DMG 打包被从临时 AD 卷启动的进程占用；仅终止该临时卷进程并卸载对应 `/dev/disk4` 后重跑成功，未触碰已挂载的 Magpie/Blender 卷。
- GitHub 将 Rebase and merge 限制为最多 100 个提交；原分支虽为线性、`main` 已是祖先且最终树可合并，但 130 个提交仍被 API 标记为 `rebaseable: false`。保留本地备份引用后压缩为一个提交，逐树验证无内容变化，并以 `--force-with-lease` 安全更新远端后完成 Rebase and merge。

## 决策日志

- 2026-07-25：插件内容变化继续在下一次 Preview 检测，不把 Launch 扩展成扫描/刷新步骤。
- 2026-07-25：旧 cache 无 provenance 时保守保留，避免猜测性删除用户数据。
- 2026-07-25：`shareBaseAuth` 独立开关留到后续，本次只保证配置策略不改变认证语义。
- 2026-07-25：文档审查后取消物理缓存清理，manifest 缩减为 applied policy/profile/明确 Project overlay；legacy 同名所有权有歧义时 fail closed。
- 2026-07-25：Apply loading 是用户明确要求，纳入正式产品 requirement 与状态矩阵，而不是计划外附带优化。
- 2026-07-25：legacy config 中只有能由本次明确 Claude Project Plugin 来源证明的条目才写入 manifest；无法证明的条目返回 `ResourceChanged`。
- 2026-07-25：legacy 中未启用的 Plugin 不承载有效项目行为，不写入 Project overlay；仅由这些禁用条目引用的 marketplace 同步丢弃。启用条目的所有权保护保持不变。
- 2026-07-25：Project 隔离策略即使 runtime registry cache 丢失也不得退回 Base；未准备状态必须先 Preview → Apply。
- 2026-07-25：Project 显式设置即使与旧继承值相等，也要作为 overlay 覆盖到新生成配置，保证 on→off 不丢设置。
- 2026-07-25：浏览器测试基础设施缺失时按 LFG pipeline 记为 Skip 并继续其他发布门禁，不安装未经项目批准的新浏览器依赖。

## 验证和验收

- `pnpm typecheck`
- `pnpm test`
- `cd src-tauri && cargo test`
- `pnpm tauri build`
- 真实 Tauri 验证 on→off→on、Preview/Apply loading、terminal gate、Base auth、Project Plugin 保留、重启持久化
- 确认 `<project>/.codex/config.toml` digest 前后不变

## 幂等性和恢复

所有运行时变化走 MutationPlan；Apply 前检查 read-set/digest，失败使用既有 backup/compensation rollback。重复 Apply 应收敛为同一 config/manifest。旧数据缺字段默认继承；旧缓存无法证明来源时不删除。代码实施在当前脏工作树上增量进行，不覆盖已有相关修复。

## 接口和依赖

- 不新增第三方依赖。
- 扩展 Project JSON/Tauri command、conversion options、runtime status strict schema。
- 新增派生运行时内部 manifest 文件，不改变 bundle identifier 或 Codex 原生项目配置格式。

## 结果回顾

- 实际迁移：旧项目默认继承；新 manifest 严格记录 applied policy/profile/Project overlay。legacy 条目只有来源可证明时迁移，歧义 fail closed。
- 自动化验证：`pnpm format:check`、`pnpm lint`、`pnpm typecheck`、24 个前端文件共 131 项测试、Vite build 与 bundle 门禁通过；Rust `fmt`、`clippy -D warnings`、248 项单元测试通过（4 项按设计 ignored），全部集成测试通过；`git diff --check` 通过。
- 生产构建：`pnpm tauri build` 成功，产物为 `AD.app` 与 `AD_1.0.1_aarch64.dmg`。
- 本地安装：合并提交 `05cceae` 的精确代码树已安装到 `/Applications/AD.app`，与构建产物内容一致且可执行文件 SHA-256 同为 `7479fc9aff015dc2e40653256b1d0e71c18a1b275719c3985341fe06eac2d2fc`；DMG SHA-256 为 `367e41d4f1eead14a333e99cd96498dee4f719a1e0a9b25a85a9868ac26a6a73`。quarantine 已清除；本次覆盖前版本保存在 `/tmp/AD.app.pre-final-05cceae-20260725-214849`。本地未签名包的严格 codesign 校验仍报告资源签名不完整。
- 浏览器验收：Skip；内置浏览器无可用会话，fallback CLI 未安装。组件交互由 RTL 测试覆盖，但不等同于真实浏览器/真实 Tauri UI 验收。
- PR 与 CI：GitHub PR #10（`https://github.com/taobaorun/ad/pull/10`）已于 2026-07-25 21:39 GMT+8 通过 Rebase and merge 合入 `main`；压缩后的 head `aad3722` 与合并提交 `05cceae` tree 完全一致，Frontend 与 Backend jobs 全绿，54 个 Codex review 线程均已解决。

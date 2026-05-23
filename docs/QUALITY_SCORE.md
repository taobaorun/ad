# 质量评分

衡量 AD 代码 / 设计 / 文档质量的标准。每次 PR 都应该按这个 rubric 自检一遍。

## 五个维度

| 维度 | 关注点 | 关键问题 |
|---|---|---|
| **Correctness** | 干的是不是用户想要的事 | 边界条件考虑了吗？错误路径测试了吗？ |
| **Reliability** | 崩溃 / 中断后能否恢复 | 写文件用 atomic 吗？有备份吗？幂等吗？ |
| **Readability** | 后人能否在 5 分钟内读懂 | 命名 / 模块边界 / 注释解释 why 不是 what |
| **Architecture** | 是否符合分层和职责 | 业务逻辑跑 OS 集成里了吗？fs 跑 commands 里了吗？ |
| **Security** | 是否泄漏敏感信息 | key 进 log 了吗？剪贴板暴露安全吗？ |

## 评分标准

每个维度 0-3 分：

- **3 — 优秀**：超出预期，可以作为模板/示范
- **2 — 合格**：满足要求，正常通过
- **1 — 需改进**：通过但有明显短板，应该有 follow-up
- **0 — 不通过**：必须修改才能合并

总分 ≥ 10/15 可以合并；< 10 必须修复。

## 自检 checklist

### Correctness（3）

- [ ] 验收标准（acceptance criteria）写出来了，不是脑补
- [ ] 测试覆盖至少一条 happy path + 一条错误路径
- [ ] 边界条件想过：空输入 / null / 极大值 / 并发
- [ ] 手动跑过：`pnpm tauri dev`，UI 行为符合预期

### Reliability（3）

- [ ] 写文件全部走 `fs::atomic::write_atomic`
- [ ] 覆盖前先备份
- [ ] 错误不静默：返回 `Err` 或 `tracing::warn!`+ context
- [ ] 步骤幂等：失败后重试不会脏化状态

### Readability（3）

- [ ] 函数 ≤ 50 行（合理拆分）
- [ ] 命名读起来像英语句子（`activate_profile`、`backups_dir`）
- [ ] 注释解释 why，不是 what
- [ ] 没有 dead code / 未用 import

### Architecture（3）

- [ ] 业务逻辑在 `commands/`，fs 在 `fs/`，OS 集成在 `tray/` / Tauri plugin
- [ ] 跨边界类型用 zod / serde 严格定义
- [ ] 没有循环依赖（`cargo modules` / 手工检查）
- [ ] 改动不引入新的"上帝模块"（一个文件管所有）

### Security（3）

- [ ] API key / token 不进 log，不进 Debug
- [ ] 用户输入校验（zod schema、扩展名检查、URL scheme 限制）
- [ ] 不执行用户提供的命令字符串
- [ ] 新依赖看过 `cargo audit` / npm advisories

## 红灯（任何一个出现都必须停下）

- [ ] `unwrap()` / `expect()` 出现在生产路径（commands、fs、tray、main 都算）
- [ ] 写 settings 没走 `write_atomic`
- [ ] 写 settings 没备份
- [ ] 新增了"以防万一"的代码（YAGNI）
- [ ] PR 描述 ≠ 实际改动（描述里说改 X，diff 里改了 Y）

## CI 强制门

CI 失败的都是红灯：

- `pnpm format:check`
- `pnpm lint`（`--max-warnings=0`）
- `pnpm typecheck`
- `pnpm test`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

本地通过 + CI 通过才能合并。

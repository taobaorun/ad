# AD

管理 Claude Code (`~/.claude/`) settings 配置的 macOS 桌面应用。Tauri 2.x（Rust 后端 + React 前端），双窗口架构（主窗口 + Settings 窗口）。

文档语言: zh

---

## Agent 工作规范（必须遵循）

### 任务复杂度判断

| 复杂度 | 判断标准 | 工作方式 |
|--------|----------|----------|
| 简单 | 单文件修改、小 bug 修复、简单查询 | 直接执行 |
| 中等 | 涉及 2-5 个文件、需要设计思考 | 可选创建 ExecPlan |
| 复杂 | 涉及 5+ 文件、多模块改动、架构变更 | **必须创建 ExecPlan** |

### ExecPlan 工作流程

对于复杂任务，必须遵循 [PLANS.md](docs/PLANS.md)：

```
1. 创建 ExecPlan → docs/exec-plans/active/<任务名>.md + .html
2. 把 HTML 路径展示给用户评审，等确认后才能执行
3. 执行时只更新 MD 的 Progress（HTML 冻结为基线快照）
4. 完成后 MD + HTML 一起移到 docs/exec-plans/completed/
```

**ExecPlan 创建后不能自动执行，必须等用户确认。**

### 任务收尾清理（必须执行）

涉及本地开发、测试、生产构建、应用安装或 PR 交付的任务，在全部验证和安装完成后，Agent 必须自动清理本次任务产生且可重新生成的中间产物，无需等待用户再次提醒：

- 清理 Rust/Tauri 构建缓存：`src-tauri/target/`（优先使用 `cargo clean --manifest-path src-tauri/Cargo.toml`）
- 清理前端与 Tauri 生成目录：`dist/`、`src-tauri/gen/`
- 清理 TypeScript 增量和生成配置前，先用 `git ls-files --error-unmatch <path>` 检查；仅删除未被 Git 跟踪的 `*.tsbuildinfo`、`vite.config.{js,d.ts}`、`tailwind.config.{js,d.ts}`、`vitest.config.{js,d.ts}`。本仓库已跟踪的 `tailwind.config.js` 必须保留
- 清理本次任务明确创建的 `/tmp` 文件、监控状态、调试安装包和废弃的旧版 `AD.app`
- 清理后运行 `git status --short`，确认没有误删源码或产生未预期的工作区变更

清理必须在构建验证、安装复制和产物交付之后执行。用户明确要求保留 `.app`、`.dmg` 或其他交付物时，保留指定产物。不得删除 `node_modules/`、项目级 `.claude/` 配置、`~/.ad/` 用户数据、源码、文档或当前 `/Applications/AD.app` 正式安装版本。

---

## 技术栈

Tauri 2.x · React 18 · TypeScript 5 · Vite 6 · Tailwind 3 · CodeMirror 6 · Zustand 5 · react-i18next · Rust edition 2021 · pnpm 9 · macOS only

## 关键文档

| 文档 | 用途 |
|------|------|
| [docs/design-docs/architecture.md](docs/design-docs/architecture.md) | **系统架构、项目结构、运行时数据位置** |
| [docs/CODE_STYLE.md](docs/CODE_STYLE.md) | **写代码前必读** — 命名 / 提交 / i18n 规范 |
| [docs/DESIGN.md](docs/DESIGN.md) | 设计原则（数据安全、边界、错误处理） |
| [docs/PLANS.md](docs/PLANS.md) | ExecPlan 模板和规范 |
| [docs/design-docs/index.md](docs/design-docs/index.md) | 设计文档索引 |
| [docs/product-specs/index.md](docs/product-specs/index.md) | 产品规格索引 |
| [docs/issues/index.md](docs/issues/index.md) | Bug 报告索引 |

## 常用命令

```bash
pnpm install                # 安装依赖
pnpm tauri dev              # 开发模式
pnpm tauri build            # 生产构建（.app + .dmg）
pnpm typecheck              # 前端类型检查

# 安装到 /Applications（本地未签名）
cp -R src-tauri/target/release/bundle/macos/AD.app /Applications/
xattr -dr com.apple.quarantine /Applications/AD.app
```

## 代码规范摘要

详见 [docs/CODE_STYLE.md](docs/CODE_STYLE.md)：

- **注释 / 错误 / 日志 / Git commit**：全部英文
- **Rust**：`snake_case` 函数变量、`PascalCase` 类型
- **TypeScript**：`camelCase` 变量函数、`PascalCase` 组件/类型
- **Git 提交**：Conventional Commits（`feat(skills):`、`fix(fs):`）
- **UI 文案（i18n）**：所有面向用户的字符串走 `src/i18n/locales/{zh,en}.json`，禁止硬编码
  - 组件内用 `const { t } = useTranslation()`，key 按 `模块.子模块.字段` 分层
  - **zh.json 和 en.json 必须同步添加**
  - 跨窗口同步：`localStorage` key `ad.lang.v1`，通过 `storage` 事件实现 Settings↔主窗口实时同步
  - 中文文案可能比英文长，组件需加 `whitespace-nowrap` 或留弹性宽度

## 注意事项

- macOS only — 不要为其他平台加抽象层
- bundle identifier `com.jiaxy.ad`，改它会让 macOS 视为新 app
- 运行时数据在 `~/.ad/`，详见 [architecture.md](docs/design-docs/architecture.md)
- 文件原子写依赖 APFS rename 语义（`src-tauri/src/fs/atomic.rs`）
- bundle name 受 `tauri.conf.json:productName` 控制（`AD`）

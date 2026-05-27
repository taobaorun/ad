# AD

SwitchHosts 风格的桌面应用，用于管理 Claude Code (`~/.claude/`) 的 settings 配置。原项目名 `cc-switch`，已重命名为 `AD`（package/crate 等机器标识符仍为小写 `ad`）。

文档语言: zh

---

## Agent 工作规范（必须遵循）

本文件是 Agent 进入项目的入口。Agent 在执行任务时必须遵循以下规范：

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
2. 把 HTML 路径展示给用户（用户在浏览器评审），等用户确认后才能执行
3. 执行时持续更新 MD 的 Progress（HTML 在 Stage 2 批准后冻结，执行期不动）
4. 完成后把 MD 和 HTML 一起移到 docs/exec-plans/completed/，编写结果回顾
```

**重要：ExecPlan 创建后不能自动执行，必须等用户确认方案后才开始。**

### 强制行为

- [ ] 任务开始前：读取本文档了解上下文
- [ ] 复杂任务：创建 ExecPlan（MD + HTML）
- [ ] **ExecPlan 创建后：把 HTML 路径展示给用户并等待确认**
- [ ] 执行过程中：只更新 MD 的 Progress（HTML 已冻结为基线快照）
- [ ] 任务完成后：MD + HTML 一起移到 completed/，编写回顾
- [ ] 发现技术债务：记录到 docs/exec-plans/tech-debt-tracker.md
- [ ] 重要设计决策：写入 docs/design-docs/<feature>.html，并在 index.md 加链接

---

## 技术栈

- **桌面框架**：Tauri 2.x（Rust 后端 + WebView 前端）
- **前端**：React 18 + TypeScript 5 + Vite 6 + Tailwind 3
- **状态管理**：Zustand 5
- **后端**：Rust (edition 2021, MSRV 1.77)
- **打包**：pnpm 9 + cargo
- **平台**：macOS only（minimumSystemVersion 11.0）

## 关键文档

- [代码规范](docs/CODE_STYLE.md) — **写代码前必读**
- [设计原则](docs/DESIGN.md)
- [产品理念](docs/PRODUCT_SENSE.md)
- [可靠性规范](docs/RELIABILITY.md)
- [安全规范](docs/SECURITY.md)
- [质量评分](docs/QUALITY_SCORE.md)
- [ExecPlan 规范](docs/PLANS.md)
- [产品规格索引](docs/product-specs/index.md)
- [设计文档索引](docs/design-docs/index.md)
- [Bug 报告索引](docs/issues/index.md)

## 常用命令

```bash
# 安装依赖
pnpm install

# 启动开发模式（Vite + Tauri，会编译 Rust）
pnpm tauri dev

# 生产构建（生成 .app 和 .dmg）
pnpm tauri build

# 前端类型检查
pnpm typecheck

# 前端 lint / format
pnpm lint
pnpm format:check

# 前端测试
pnpm test

# Rust 检查 / lint / 测试（在 src-tauri/ 下）
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test
cargo fmt --check

# 安装到 /Applications（本地未签名）
cp -R src-tauri/target/release/bundle/macos/AD.app /Applications/
xattr -dr com.apple.quarantine /Applications/AD.app
```

## 项目结构

```
ad/
├── src/                     # 前端源码（React + TS）
│   ├── components/          # UI 组件
│   │   ├── ui/              # shadcn/radix 基础组件（button/dialog/input/tabs 等）
│   │   ├── ProjectSidebar.tsx   # 左栏 project 列表
│   │   ├── ProjectDetail.tsx    # 主区 project 详情 + apply flow
│   │   ├── ProfileEditDrawer.tsx # 右侧 profile 编辑浮层
│   │   ├── CommandPalette.tsx   # ⌘K 全局动作面板
│   │   ├── GlobalKeymap.tsx     # 全局快捷键注册
│   │   ├── ProfileEditor.tsx    # profile 内容编辑（Shared/Local/Env 三 tab）
│   │   ├── HistoryDialog.tsx    # 应用历史对话框
│   │   ├── HistoryPanel.tsx     # 历史条目列表（被 HistoryDialog 引用）
│   │   ├── InlineConflictResolver.tsx  # apply 冲突 inline 解决
│   │   ├── StatusRing.tsx       # 项目状态色环
│   │   ├── DiffView.tsx         # diff 展示
│   │   ├── EmptyState.tsx       # 空状态占位
│   │   └── ...                  # ActivateToast / AdvancedSettings 等
│   ├── store/               # Zustand stores（profiles / projects / ui / uiSettings）
│   ├── lib/                 # 工具函数（tauri IPC / keymap / pathAutocomplete 等）
│   ├── i18n/                # i18next init + locales/{zh,en}.json（默认 zh）
│   └── App.tsx              # 应用入口（双栏布局 + overlay 层）
├── src-tauri/               # Tauri / Rust 后端
│   ├── src/
│   │   ├── commands/        # Tauri 命令（前端可调用）
│   │   ├── fs/              # 文件原子写、路径解析
│   │   ├── terminal/        # 外部终端 launcher（Ghostty / cmux / Terminal.app / Custom）
│   │   ├── tray/            # macOS 菜单栏托盘
│   │   ├── migration.rs     # 旧 profile 迁移
│   │   ├── models.rs        # 共享数据模型
│   │   ├── lib.rs           # 库入口
│   │   └── main.rs          # 二进制入口
│   ├── tests/               # Rust 集成测试
│   ├── icons/               # 应用图标
│   ├── Cargo.toml
│   └── tauri.conf.json
├── tests/                   # 前端测试
├── docs/                    # 文档（harness）
└── package.json
```

## 关键运行时数据位置

AD 的数据全部在 `~/.ad/`（v0.2 后从 `~/.claude/` 搬出，启动时自动迁移）。CC 自己的数据仍在 `~/.claude/`，AD 只读不写（除 legacy 激活路径外）。

- profiles：`~/.ad/profiles/<id>.json`（v0.2 layered shape，含 `layers: { shared, local, env }`）
- 备份：`~/.ad/backups/<ISO8601>-<project-slug>-<layer>.json`（apply 产生）+ `~/.ad/backups/<ISO8601>-<uuid>.json`（legacy 全局激活产生，向后兼容）
- 历史：`~/.ad/history/<ts>.<uuid>.json`（旧版 `history.jsonl` 仍兼容读）
- active 指针：`~/.ad/active`
- 项目注册：`~/.ad/state/projects.json`（AD 内部 project↔profile 映射）
- 扫描根：`~/.ad/state/scan_roots.json`（auto-detect 源；默认含 builtin `~/.claude/projects`）
- 测试 home 覆盖：`AD_HOME=<path>` env var

## 主要架构（v0.4）

**项目状态真理源 = `<project>/.claude/settings*.json`，profile 退化为只读 template**：每个 project 的当前配置就是它自己 .claude 目录下的两个文件，AD 不再额外维护"项目侧 profile"镜像。Profile（也就是过去的 profile 文件）现在纯粹是"初始化或切换 template 时复制内容的来源"——改 profile 不再影响已经从它生成的项目。env 层在切换 template 时合并到 `settings.local.json` 顶层 `env` 字段，CC 启动时自动加载，不再需要手动 export。

**UI 形态（v0.4）**：双栏 + drawer + command palette
- 左栏 `ProjectSidebar` 不变：紧凑 project rows + 状态色环 + ⌘1-9 快捷键
- 主区 `ProjectDetail` 是项目自身的配置编辑器：header（项目名 / 路径 / git 状态）+ TemplateBreadcrumb（"由模板 X 初始化" + Switch template 按钮）+ 内联 `ProjectConfigEditor`（三 tab Shared/Local/Env，Save 直接写盘到 .claude/）
- `<LayeredSettingsEditor>` 是公共组件（Monaco JSON + KV env 表），ProfileEditor 和 ProjectConfigEditor 都用它
- ⌘K `CommandPalette` 入口：apply / switch project / **switch template** / **edit template** / add / history
- 右侧 `ProfileEditDrawer` 现在是 **template 编辑器**（编辑只读模板，不影响当前项目），从 ⌘K palette 的 "edit template <name>" 触发
- `SwitchTemplateDialog` 弹出选模板 → 调 `apply_profile_to_project`（保留原 IPC）→ 三层都写入 .claude/settings*.json，冲突走 InlineConflictResolver

主路径：sidebar 选 project → 主区编辑 .claude/settings*.json → Save。从模板初始化或切换走 Switch template 按钮 / ⌘K palette。

旧路径（保留）：Advanced settings → 开启 legacy toggle → template 编辑器（drawer）顶部出现 [Activate (legacy)] 按钮，点击后覆盖 `~/.claude/settings.json`，给"我只想全局换一份配置"的简单用法兜底。

完整 v0.4 设计见 `docs/exec-plans/completed/per-project-config-model.{md,html}`（活动期间在 active/ 下）。
v0.3 历史：`docs/exec-plans/completed/ui-redesign.{md,html}` + `docs/design-docs/ui-redesign-options.html`（含可交互原型 + 4 方向对比）。
分层 profile 模型基础（schema 不变）：`docs/exec-plans/completed/layered-profile-redesign.{md,html}`。
外部终端集成：`docs/exec-plans/completed/terminal-launcher-integration.{md,html}` + `docs/design-docs/terminal-launcher.html`（4 backend 行为矩阵）。

## 代码规范摘要

详见 [docs/CODE_STYLE.md](docs/CODE_STYLE.md)：

- **注释 / 错误 / 日志**：全部英文
- **Rust**：`snake_case` 函数变量、`PascalCase` 类型、`SCREAMING_SNAKE_CASE` 常量
- **TypeScript**：`camelCase` 变量函数、`PascalCase` 组件/类型、`SCREAMING_SNAKE_CASE` 常量
- **Git 提交**：Conventional Commits，scope 对应模块（`feat(tray):`、`fix(fs):`、`refactor(commands):`）
- **UI 文案**：默认中文，可切英文（react-i18next）；所有面向用户字符串必须走 `src/i18n/locales/{zh,en}.json`，禁止硬编码

## 注意事项

- macOS only — 不要为其他平台加抽象层
- bundle identifier `com.jiaxy.ad`，改它会让 macOS 视为新 app（丢失通知/辅助功能权限）
- 运行时数据目录是 `~/.ad/`，重命名前必须先迁移用户数据
- 文件原子写依赖 APFS rename 语义（`src-tauri/src/fs/atomic.rs` 顶部有 caveat）
- bundle name 受 `tauri.conf.json:productName` 控制，目前是 `AD`（产物 `AD.app`）；package 名是 `ad`（lowercase）

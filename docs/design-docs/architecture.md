# AD 系统架构（v1.0）

## 双窗口架构

AD 运行两个独立 Tauri WebView 窗口，各自有独立的 React 入口和 localStorage：

- **主窗口** (`App.tsx`)：双栏布局 — 左栏 ProjectSidebar + 主区 ProjectDetail/ProjectConfigEditor
- **Settings 窗口** (`SettingsApp.tsx`)：侧栏导航 — 通用 / 终端 / Skills / 快捷键 / 遗留

跨窗口状态同步通过 `localStorage` 的 `storage` 事件实现（一个窗口写 → 另一个窗口收到事件并更新）。已同步的状态：dark mode、终端配置、全局快捷键、语言。

## 项目配置模型

**项目状态真理源 = `<project>/.claude/settings*.json`，profile 退化为只读 template**：每个 project 的当前配置就是它自己 .claude 目录下的两个文件，AD 不再额外维护"项目侧 profile"镜像。Profile 现在纯粹是"初始化或切换 template 时复制内容的来源"——改 profile 不再影响已有项目。env 层在切换 template 时合并到 `settings.local.json` 顶层 `env` 字段，CC 启动时自动加载。

## Skill 管理

AD 管理 Claude Code 的 skill（`~/.claude/skills/`）：
- **Skill Sources**：git 仓库或本地目录，clone/pull 到 `~/.ad/skill-library/`
- **全局 Skills**：通过 symlink 安装到 `~/.claude/skills/`，所有项目可见
- **项目 Skills**：每个项目可单独启用/禁用 skill（allowlist/blocklist）
- 管理界面在 Settings 窗口的 Skills tab

## UI 交互

- 左栏 `ProjectSidebar`：紧凑 project rows + 状态色环 + ⌘1-9 快捷键
- 主区 `ProjectDetail`：header + TemplateBreadcrumb + 内联 `ProjectConfigEditor`（三 tab Shared/Local/Env，Save 直接写盘到 .claude/）
- `<LayeredSettingsEditor>` 是公共组件（CodeMirror 6 JSON via `JsonEditor.tsx` + KV env 表），ProfileEditor 和 ProjectConfigEditor 都用它
- ⌘K `CommandPalette`：switch project / switch template / edit template / add / history
- 右侧 `ProfileEditDrawer`：template 编辑器（编辑只读模板），从 ⌘K 触发
- `SwitchTemplateDialog`：选模板 → `apply_profile_to_project` → 写 .claude/settings*.json，冲突走 InlineConflictResolver

## 运行时数据位置

AD 的数据全部在 `~/.ad/`（v0.2 后从 `~/.claude/` 搬出，启动时自动迁移）。CC 自己的数据仍在 `~/.claude/`，AD 只读不写（除 legacy 激活路径外）。

| 数据 | 路径 |
|------|------|
| profiles | `~/.ad/profiles/<id>.json`（layered shape，含 `layers: { shared, local, env }`） |
| 备份 | `~/.ad/backups/<ISO8601>-<project-slug>-<layer>.json` |
| 历史 | `~/.ad/history/<ts>.<uuid>.json` |
| active 指针 | `~/.ad/active` |
| 项目注册 | `~/.ad/state/projects.json` |
| 扫描根 | `~/.ad/state/scan_roots.json` |
| skill library | `~/.ad/skill-library/<source-id>/` |
| skill sources | `~/.ad/state/skill-sources.json` |
| 测试 home 覆盖 | `AD_HOME=<path>` env var |

## 项目结构

```
ad/
├── src/                     # 前端源码（React + TS）
│   ├── components/          # UI 组件
│   │   ├── ui/              # shadcn/radix 基础组件（button/dialog/input/tabs 等）
│   │   ├── ProjectSidebar.tsx   # 左栏 project 列表
│   │   ├── ProjectDetail.tsx    # 主区 project 详情
│   │   ├── ProjectConfigEditor.tsx # 项目配置编辑器（三 tab Shared/Local/Env）
│   │   ├── ProfileEditDrawer.tsx # 右侧 template 编辑浮层
│   │   ├── ProfileEditor.tsx    # template 内容编辑（Shared/Local/Env 三 tab）
│   │   ├── LayeredSettingsEditor.tsx # 公共分层编辑器（CodeMirror JSON + KV env 表）
│   │   ├── CommandPalette.tsx   # ⌘K 全局动作面板
│   │   ├── GlobalKeymap.tsx     # 全局快捷键注册
│   │   ├── SwitchTemplateDialog.tsx # 切换 template 弹窗
│   │   ├── SkillSources.tsx     # Settings: skill source 管理 + 全局 skill 开关
│   │   ├── ProjectSkills.tsx    # 项目级 skill 配置
│   │   ├── SkillToggle.tsx      # skill 开关组件
│   │   ├── JsonEditor.tsx       # CodeMirror 6 受控包装
│   │   ├── HistoryDialog.tsx    # 应用历史对话框
│   │   ├── HistoryPanel.tsx     # 历史条目列表
│   │   ├── InlineConflictResolver.tsx  # apply 冲突 inline 解决
│   │   ├── StatusRing.tsx       # 项目状态色环
│   │   ├── ImportDialog.tsx     # 导入对话框
│   │   ├── DetectedProjectsModal.tsx # 自动发现项目弹窗
│   │   └── EmptyState.tsx       # 空状态占位
│   ├── store/               # Zustand stores
│   │   ├── profiles.ts      # template（原 profile）CRUD
│   │   ├── projects.ts      # 项目列表、选中态、apply
│   │   ├── skills.ts        # skill source / entry / scope 管理
│   │   ├── ui.ts            # 临时 UI 状态（弹窗、选中等）
│   │   └── uiSettings.ts    # 持久化偏好（dark mode / terminal / 快捷键 / 语言）
│   ├── lib/                 # 工具函数
│   │   ├── tauri.ts         # Tauri IPC 封装（所有 invoke 集中定义）
│   │   ├── skillTypes.ts    # skill 相关 TS 类型
│   │   ├── keymap.ts        # 快捷键注册
│   │   ├── pathAutocomplete.ts # 路径自动补全
│   │   └── ...              # utils / profileSchema / projectTypes 等
│   ├── i18n/                # i18next init + locales/{zh,en}.json（默认 zh，跨窗口同步）
│   ├── main.tsx             # entry：hash 路由动态 import App / SettingsApp（独立 chunk）
│   ├── App.tsx              # 主窗口
│   └── SettingsApp.tsx      # Settings 窗口入口
├── src-tauri/               # Tauri / Rust 后端
│   ├── src/
│   │   ├── commands/        # Tauri 命令（前端可调用）
│   │   │   ├── skills.rs    # skill source CRUD / scan / scope / clone / update
│   │   │   ├── profiles.rs  # template CRUD
│   │   │   ├── projects.rs  # 项目注册 / 列表
│   │   │   ├── apply.rs     # template → project 写盘
│   │   │   ├── settings.rs  # 项目 settings.json 读写
│   │   │   ├── discover.rs  # 自动发现项目
│   │   │   ├── scan_roots.rs # 扫描根管理
│   │   │   ├── history.rs   # 操作历史
│   │   │   ├── terminal.rs  # 外部终端启动
│   │   │   ├── shortcut.rs  # 全局快捷键
│   │   │   ├── importers.rs # 导入
│   │   │   ├── activate.rs  # legacy 全局激活
│   │   │   └── path_complete.rs # 路径补全
│   │   ├── fs/              # 文件系统工具
│   │   │   ├── atomic.rs    # 原子写（APFS rename）
│   │   │   ├── git.rs       # git clone / pull 封装
│   │   │   ├── merge.rs     # JSON 合并
│   │   │   └── paths.rs     # 路径解析
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

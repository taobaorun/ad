# AD 核心设计信念

指导 AD 系统设计的核心信念。每个技术决策都应该能追溯到这些信念之一。

## 1. 用户的数据比 AD 自己重要

> AD 出问题可以重装；用户的 `settings.json` 弄丢了 CC 就废了。

具体表现：
- 写文件全走 `write_atomic`，崩溃不留半截
- 覆盖前必备份，备份文件 append-only 不删
- 错误暴露给用户（toast / dialog），不静默吞

## 2. 简洁 > 灵活

> 配置项不是越多越好，灵活是 over-engineering 的烟雾弹。

- profile 是文件，不是数据库
- 历史是 JSON 文件目录，不是 SQLite
- 没有插件系统、没有脚本钩子、没有自定义主题
- 抽象只在两次重复出现之后才提取

## 3. 平台原生感 > 跨平台一致

> AD 是 macOS 工具，不是"也能跑在 macOS 的 Electron 应用"。

- tray、notification、dialog、剪贴板 —— 全部走 macOS 原生 API（通过 Tauri）
- 配色、字体、动效跟 macOS 默认走，不发明视觉语言
- 不为未来 Linux / Windows 预留抽象层

## 4. 显式 > 隐式

> 让代码意图能被读出来，而不是靠"约定俗成"。

- 配置项有 schema，不是 free-form JSON
- IPC 边界类型显式定义，前后端共享
- 命名能读出意图：`activate_profile` 而不是 `apply`、`switch`

## 5. 可测性是第一公民

> 不能测的代码就是设计不对。

- 文件操作通过 `AD_HOME` env var 注入测试 home，永远不写真实 `~/.claude/`
- 业务逻辑（`commands/`）和 OS 集成（`tray/`）严格分层，前者纯函数化
- 集成测试覆盖崩溃路径（`tests/atomic_crash.rs`）

## 6. 信任 OS / 框架，不重新发明

- 文件原子性靠 APFS journal（macOS 担保），不实现自己的 transaction
- 通知靠 `tauri-plugin-notification`，不维护自己的 toast 队列
- 进程探测靠 `sysinfo`，不写自己的 ps/ pgrep wrapper

## 7. 没有 fallback，错就是错

- HOME 解析失败 → `FsError::NoHome`，不去猜 `/Users/$USER`
- profile JSON 解析失败 → 跳过这个 profile + log warn，不"尽力修复"
- 备份不存在 → restore 操作直接拒绝，不"用空文件代替"

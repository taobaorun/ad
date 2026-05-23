# 设计原则

AD 的设计哲学和系统设计核心信念。每个技术决策都应该能追溯到这些原则之一。

## 1. 用户数据是神圣的

> 永远不在没有备份的情况下覆盖用户已有的文件。

- 写 `~/.claude/settings.json` 之前必须先读出旧内容备份到 `~/.claude/ad/backups/<ISO8601>.json`
- 写所有持久化文件走 `fs::atomic::write_atomic`：写临时文件 → fsync → rename，崩溃不会留半截文件
- 删除是 opt-in，从不主动清理用户文件（包括 `.legacy/`、旧备份）

## 2. macOS only，毫不妥协

> 不为多平台抽象付任何代价。

- APFS rename 原子性、tray-icon、native notification、entitlements、code signing —— 全部 macOS 原生
- 如未来真要做 Linux/Windows，是大改造，不要现在就预留 hooks
- `pgrep` 风格的进程探测、剪贴板 API、文件路径分隔符——按 macOS 写就行

## 3. 信任 OS，少做 fallback

- HOME 解析靠 `dirs::home_dir()`，没有就 `FsError::NoHome`，不去猜 `/Users/$USER`
- 文件原子性靠 APFS journal，不重新发明事务
- 通知靠 `tauri-plugin-notification`，不自己维护 toast queue
- 不写"以防万一某天 macOS 不支持 X"的代码

## 4. 简单 > 灵活

> 配置项不是越多越好。

- profile 是文件，不是数据库
- 历史是 append-only 的 JSON 文件目录，不是 SQLite
- tray 状态不缓存，每次重新读取
- 如果某个东西能用 `std::fs` 解决，就不要引 `tokio::fs`

## 5. 边界清晰

| 边界 | 谁负责 |
|---|---|
| 文件系统 / 持久化 | `src-tauri/src/fs/`（atomic、paths） |
| 业务逻辑 | `src-tauri/src/commands/`（每个文件一个命令族） |
| OS 集成（tray / notification / clipboard） | `src-tauri/src/tray/`、Tauri plugin |
| 渲染 / 交互 | `src/components/` |
| 跨组件状态 | `src/store/`（Zustand） |
| IPC 边界类型 | `src/types/`（zod schema 校验） |

不允许在 `commands/` 里直接 `use tauri::Window`，那是 OS 集成；commands 应该是纯业务逻辑 + fs 调用。

## 6. 错误暴露给用户，不静默

- 每个 fs / IPC 错误都会变成前端 toast
- 后端 `tracing::warn!` / `error!` 必须带足够上下文（模块名 + 关键 ID）
- 不允许 `unwrap_or_default()` 吃掉错误

## 7. 测试隔离 HOME

> 测试不能污染开发者的 `~/.claude/`。

- 所有 fs 测试通过 `AD_HOME=<tempdir>` env var 重定向
- 共享 env 的测试加 `#[serial_test::serial(home_env)]` 串行化
- 集成测试用 `tempfile::TempDir` 自动清理

---

参考资料：[design-docs/core-beliefs.md](design-docs/core-beliefs.md) 是更通用的工程原则；本文档是 AD 项目特化的取舍。

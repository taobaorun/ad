# 可靠性规范

AD 操作的是用户的 `~/.claude/settings.json` —— 写坏了用户的 Claude Code 就废了。可靠性是第一公民。

## 写文件硬规则

### 1. 全部走 `write_atomic`

`src-tauri/src/fs/atomic.rs` 的 `write_atomic(target, bytes)`：

```
write to <target>.tmp.<uuid> → fsync → rename to target
```

崩溃发生在 fsync 之前 → 临时文件不可见  
崩溃发生在 fsync 之后 / rename 之前 → 临时文件存在但未替换原文件  
崩溃发生在 rename 之后 → 已经成功

**禁止**直接用 `std::fs::write` / `tokio::fs::write` 写持久化数据。

### 2. 覆盖前必备份

写 `~/.claude/settings.json` 之前必须：

1. 读出当前内容（如果存在）
2. `write_atomic` 写入 `~/.claude/ad/backups/<ISO8601>.json`
3. 然后才写新内容

如果当前文件不存在（首次激活），跳过备份是允许的。

### 3. 历史文件 append-only

每次激活产生一个独立 JSON 文件 `~/.claude/ad/history/<ts>.<uuid>.json`，永不修改、永不删除（除非用户主动清理）。

旧的单文件 `history.jsonl` 仍兼容**读**，但**不写**。

## 路径硬规则

- HOME 通过 `dirs::home_dir()` 解析，不存在就 `FsError::NoHome`
- 测试用 `AD_HOME` env var 重定向（见 `src-tauri/src/fs/paths.rs:home`）
- 永不硬编码 `/Users/<name>/.claude/`
- 永不用相对路径写持久化数据

## 错误处理硬规则

- 文件 IO 错误必须包裹 `FsError::io(path.display().to_string(), e)`，让用户看见路径
- IPC 命令返回 `CmdResult<T>` = `Result<T, CommandError>`，不 panic
- `unwrap()` / `expect()` 只允许在测试中
- panic hook 走 tracing，不直接 println

## 进程隔离

- AD 不假设自己是单实例，但实际上 Tauri 默认就是单实例（macOS 行为）
- 写 `settings.json` 时如果检测到 `claude` / `claude-code` 进程在跑，提醒用户重启
- 不杀死任何外部进程，永远

## 启动时迁移

`src-tauri/src/migration.rs::migrate_legacy_profiles` 在启动时把旧格式 profile 文件搬到 `~/.claude/profiles/.legacy/`：

- 必须幂等：第二次启动不重复迁移
- 必须保留原文件副本（不删除）
- 失败时记 warn，不阻塞启动（CC 体验不能因为 AD bug 完全卡死）

## 崩溃恢复

如果 AD 启动时发现 `~/.claude/settings.json` 是空文件 / 损坏 JSON：

- 不自动覆盖
- 在主窗口显眼提示用户从最近备份 restore
- 备份按时间戳逆序展示

## 测试要求

- `commands/activate.rs` 必须有「写到一半 abort」的集成测试（`tests/atomic_crash.rs`）
- 所有 fs 测试用 tempdir，**不允许**测试碰真实 `~/.claude/`
- CI 必须跑 `cargo test --all-targets`，所有测试通过

## 已知风险

- **APFS only**：`write_atomic` 不 fsync 父目录。如果将来跑在 ext4 / FAT，rename 可能丢失 → 重新评估（见 `fs/atomic.rs` 顶部 caveat）
- **Tauri 重启不杀子进程**：dev 模式下 hot-reload 期间老的 ad 进程可能仍持有文件锁；release 模式无此问题

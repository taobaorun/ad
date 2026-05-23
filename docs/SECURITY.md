# 安全规范

AD 处理用户的 Anthropic API key 和其他凭据，需要按"敏感数据处理工具"标准设计。

## 威胁模型

**主要威胁**：
1. API key 通过 AD 自身泄漏（写到 log、复制到剪贴板时被截屏 / 误粘贴）
2. 写错配置导致 CC 运行时把 key 发到错误的 base URL
3. AD 进程被替换 / 被注入恶意代码（macOS 沙箱外的常规风险）

**不在威胁模型内**：
- 物理访问设备 → AD 假设 macOS 用户登录态是可信的
- 整台 Mac 被入侵 → key 在磁盘上明文这件事是 CC 本身的设计，AD 不试图加密
- 网络中间人 → CC 自己处理 TLS

## 数据处理硬规则

### 1. API key 不进 log

```rust
// Good
tracing::info!(profile_id = %id, "activate_profile: ok");

// Bad — never log api key, even truncated, even in debug
tracing::debug!(?settings, "writing settings");
```

任何 `Debug` impl 涉及 settings 的字段必须自定义，遮蔽敏感字段（用 `***` 替代）。

### 2. 备份文件权限

`~/.claude/ad/backups/<ts>.json` 包含完整 settings（含 key），权限应保持默认 0644 → 这是用户 home 下的私有文件，OS 已经隔离。**不要**主动 chmod 0600 把权限收紧再松开 — APFS 上没必要，多此一举。

但**不能**把备份目录暴露在 `~/Public/` 这种位置。

### 3. 剪贴板写入需明示

复制 API key 这种敏感字段到剪贴板必须：
- UI 上显式按钮触发（不是悬停 / 右键自动）
- toast 提示"已复制 — 60 秒后自动清空"
- 启动一个 timer 60 秒后清空剪贴板（如果剪贴板内容仍是同一个 key）

### 4. profile 导入校验

`commands/importers.rs` 从文件 / URL 导入 profile：

- URL 导入：`http://` / `https://` only，禁 `file://` / `ftp://`
- URL 大小限制：1MB（`MAX_BODY_BYTES`）
- timeout：5 秒（`HTTP_TIMEOUT`）
- 解析后必须用 zod / serde 严格校验 schema，多余字段拒绝（防止偷渡 hooks / scripts）
- 文件导入：扩展名必须 `.json`

### 5. 不执行用户内容

settings 里如果出现 `command` / `script` 字段（CC 的 statusLine / hooks 等都接受 shell 命令），AD 把它们当字符串存储和展示，**不解析、不执行**。AD 永远不替用户跑这些命令。

## Tauri allowlist

`src-tauri/tauri.conf.json` 的 CSP 已设置：

```
default-src 'self'; img-src 'self' asset: https://asset.localhost data:;
style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-eval';
```

`unsafe-eval` 是为了 monaco-editor。如果将来不用 monaco，移除它。

Tauri 命令白名单：只暴露 `commands/` 目录里 `#[tauri::command]` 标注的函数，前端拿不到 `tauri-plugin-fs` 的通用 fs 接口。

## Code signing & notarization

- 本地 dev / 自用 build：未签名是允许的，用 `xattr -dr com.apple.quarantine` 临时绕过 Gatekeeper
- 分发给他人：必须 `pnpm release:mac`（签名 + 公证），见 [docs/release.md](release.md)

## 升级安全

未来加自动更新前，必须：
- 校验更新包签名（Tauri updater 内置）
- 更新元数据走 HTTPS
- 任何"升级"都必须显式用户确认，不静默替换 binary

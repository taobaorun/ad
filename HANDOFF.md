# cc-switch — 交接说明

按时间顺序排：**现在 → 第一次试用 → 想分发 → 长期维护**。

> 项目在：`/Users/yuanxuan/ai_workspace/cc-switch/`
> 源真相：`SPEC.md` / `PLAN.md` / `TASKS.md` / `FINAL_REPORT.md`

---

## 第 0 步 — 备份现有配置（30 秒，强烈建议）

cc-switch 在迁移时会把原文件留在 `.legacy/`，但稳妥起见：

```bash
cp -R ~/.claude/profiles ~/.claude/profiles.bak.$(date +%s)
cp ~/.claude/settings.json ~/.claude/settings.json.bak.$(date +%s) 2>/dev/null || true
```

如果出问题：删掉 `~/.claude/cc-switch/`，把 `.bak.*` 文件名还原即可。

---

## 第 1 步 — 本地 dev 模式跑起来（5–15 分钟）

```bash
cd /Users/yuanxuan/ai_workspace/cc-switch
pnpm tauri dev
```

**第一次启动很慢**：Tauri 要编译 macOS WebKit/AppKit binding（一次性，之后增量编译都很快）。期间你会看到大量 `Compiling X v0.Y.Z` 滚动。Apple Silicon Mac 大约 5–10 分钟。

**期望看到的现象**：

1. 终端最后一行：`Local: http://localhost:1420/`，然后弹出一个标题为 `cc-switch` 的窗口
2. 左侧栏出现 `homi` 和 `alibaba`（你现有的两个 profile，自动迁移成新 schema）
3. macOS 菜单栏右上角出现一个**紫色小圆点**（tray icon，初始状态颜色是默认紫，激活后变成 profile 自己的 color）
4. `~/.claude/profiles/.legacy/` 出现，里面是你原来的两个 JSON + `MIGRATION.md`

**如果窗口起不来**：检查终端 stderr。常见：

- `xcode-select --install`（你应该已有 CLT，但万一）
- 端口 1420 占用：杀掉占用它的进程或改 `vite.config.ts` 里的 port

---

## 第 2 步 — 试用清单（按这个顺序，约 5 分钟）

| 步骤 | 操作                                                 | 预期结果                                                                                                                      |
| ---- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| 1    | 点左侧栏的 `Homi`                                    | 右侧出现 Monaco JSON 编辑器，显示 settings 内容                                                                               |
| 2    | 改一个 env 值（比如 `ANTHROPIC_MODEL`），点 **Save** | 顶部 unsaved 标记消失；profile 落盘                                                                                           |
| 3    | 点 **Diff** 按钮                                     | 显示当前 `~/.claude/settings.json` vs profile 的两栏 diff                                                                     |
| 4    | 关掉 Diff，点 **Activate**                           | 第一次会弹 macOS 通知权限请求 → 选 Allow；右下角出现 toast 列出运行中的 `claude` 进程；底部出现 "Copy restart command"        |
| 5    | 检查 `~/.claude/settings.json`                       | 内容应该是你刚激活 profile 的 settings 块                                                                                     |
| 6    | 检查 `~/.claude/cc-switch/backups/`                  | 里面有刚生成的备份文件                                                                                                        |
| 7    | 点左下角 **History**                                 | 列出本次激活记录，带 Restore 按钮                                                                                             |
| 8    | 切到另一个 profile 激活                              | 再回 History，Restore 第一次的备份 → settings.json 应该字节级回到第一次激活后的状态                                           |
| 9    | 点菜单栏紫色小圆点                                   | 弹出下拉菜单：所有 profile（active 那个有 ●）+ Show cc-switch + Quit；点不同 profile 应该和主窗口激活效果一致                 |
| 10   | 点工具栏 **下载图标**（Import）                      | 弹出对话框，可选 File / URL / Gist；File 选 `~/.claude/profiles/.legacy/homi.json` 试试导入（会被改名以避免和现有 homi 冲突） |

每激活一次 Claude Code 都需要**重启它**才能拿到新 env。Toast 里有一键 Copy 重启命令（默认 `pkill -f "claude " && exec $SHELL -l`）。

---

## 第 3 步 — 试用过程中的常见问题

### 编辑器字体丑 / 字号小

`src/components/ProfileEditor.tsx` 里 `fontSize: 13`，改大；字体走系统默认（`-apple-system`）。

### 通知没弹

- 先看 macOS 系统设置 → 通知 → cc-switch，确认允许
- 如果没显示 cc-switch：终端跑 `tccutil reset Notifications com.ccswitch.app`，再启动一次

### 切换 profile 后 Claude Code 还用旧 env

正常。env 是 Claude Code 进程启动时读的，必须重启进程。Toast 里那个 "Copy restart command" 就是干这个的。

### Tray icon 是默认灰色（没颜色）

说明当前没有 active profile，激活一个就有了。

### 「响应 > 1 MiB」错误（导入 URL 时）

这是 SSRF 加固里的硬上限。你的 profile JSON 真的超 1 MB 是异常情况，先看下原文件大小。

### Migration 把我已有的 profile 改坏了

原文件在 `~/.claude/profiles/.legacy/`，复制回去就还原。Migration 是幂等的，再启动一次不会再迁移。

---

## 第 4 步 — 决定要不要打 build

### A. 本地用 / 不分发

不需要做任何事。你 `pnpm tauri dev` 跑着用就行。

需要"安装版"也可以：

```bash
pnpm tauri build --debug
```

出来的 `.app` 在 `src-tauri/target/debug/bundle/macos/`，拖进 `Applications/` 即可。**unsigned**，第一次打开 Gatekeeper 会拦——右键 → 打开。

### B. 给别人用 / 上传 GitHub Releases

需要 Apple Developer 账号（$99/年）。详细步骤在 `docs/release.md`。简化版：

1. 加入 Apple Developer Program
2. Keychain Access → 申请 Developer ID Application 证书 → 安装 → 导出 `.p12`（记下密码）
3. https://appstoreconnect.apple.com/access/integrations/api → 创建 API Key（角色：Developer）→ 下载 `AuthKey_<KEYID>.p8`
4. GitHub repo → Settings → Secrets and variables → Actions，配 7 个 secret：

   | Secret                   | 值                                            |
   | ------------------------ | --------------------------------------------- |
   | `APPLE_SIGNING_IDENTITY` | `Developer ID Application: 你的名字 (TEAMID)` |
   | `APPLE_CERT_BASE64`      | `base64 -i cert.p12` 的输出                   |
   | `APPLE_CERT_PASSWORD`    | 导出 `.p12` 时设的密码                        |
   | `KEYCHAIN_PASSWORD`      | 任意随机字符串                                |
   | `APPLE_API_KEY_ID`       | API key 的 10 字符 ID                         |
   | `APPLE_API_ISSUER`       | API issuer UUID                               |
   | `APPLE_API_KEY_BASE64`   | `base64 -i AuthKey_*.p8` 的输出               |

5. 准备一个 1024×1024 真图标 PNG，丢到 `src-tauri/icons/icon.png`，然后：

   ```bash
   pnpm tauri icon src-tauri/icons/icon.png
   ```

   生成 icns/ico/各种尺寸 png。

6. tag + push：

   ```bash
   git init && git add . && git commit -m "init"
   # 推到自己的 GitHub repo
   git tag v0.1.0
   git push --tags
   ```

   GitHub Actions 自动跑 `release.yml`，5–10 分钟后会有一个 Release，附带签名公证好的 `.dmg`。

7. **首次发布务必 smoke test**：在另一台 Mac（或 VM）下载 dmg、安装、运行：
   ```bash
   spctl -a -vvv /Applications/cc-switch.app
   # 期望: accepted, source=Notarized Developer ID
   ```

---

## 第 5 步 — 长期维护（按需做）

### 5.1 真图标设计

`src-tauri/icons/32x32.png` 等是占位紫块。设计一个 1024×1024 的真图标后跑 `pnpm tauri icon`。

### 5.2 Auto-update（v0.2.0 计划）

当前 v0.1.0 不带自动更新；用户得手动下新 dmg。要加：

- `tauri-plugin-updater` + 一个签名密钥
- GitHub Releases 上传 `latest.json`
- 详见 https://tauri.app/v1/guides/distribution/updater/

### 5.3 review 里 Optional 的 8 条（FYI 级别，长期改）

都在我之前的 review 里。摘几个：

- `read_history` 全部读到内存→反转→截断，长期用户可加文件轮转
- `migration.rs` 单个 corrupt JSON 会中止整批，应该 log + skip
- `tauri.conf.json` 的 CSP 含 `unsafe-eval`，确认 Monaco 不需要后可删
- `ProfileEditor.tsx` `useEffect` 依赖去重

### 5.4 端到端测试（`tauri-driver`）

现在只有组件级 Vitest 和 Rust 单元/集成测试。三条黄金路径建议加 E2E：

1. 创建 → 编辑 → 激活
2. 导入 URL → 激活
3. 历史回滚

### 5.5 监控我没用 git 这件事

项目目录不是 git repo（CLAUDE-mem 第 10748 条记忆的发现）。强烈建议：

```bash
cd /Users/yuanxuan/ai_workspace/cc-switch
git init
git add .
git commit -m "Initial commit: cc-switch v0.1.0"
```

之后再 push 到 GitHub。

---

## 第 6 步 — 出事了怎么办

### 我激活后 Claude Code 完全不响应

1. 看 `~/.claude/settings.json` 的内容是不是你期望的
2. 不是 → 进 cc-switch History panel → Restore 上一个备份
3. 是的 → Claude Code 自身问题，不是 cc-switch 的事

### `~/.claude/settings.json` 损坏了 / 丢了

所有备份在 `~/.claude/cc-switch/backups/<ISO8601>.json`，按时间排序，挑一个 cp 回去。

### cc-switch 卡住 / 不开

```bash
pkill -9 cc-switch
# 删掉可能有问题的状态
rm -rf ~/.claude/cc-switch/active   # 重置 active 指针，profile 文件不动
# 再开
pnpm tauri dev
```

### Migration 把 profile 改坏了

原文件在 `~/.claude/profiles/.legacy/`，复制回去。**不要删 `.legacy/`**——cc-switch 自己也不会删。

### 删掉 cc-switch 但保留 profile

```bash
rm -rf /Users/yuanxuan/ai_workspace/cc-switch
# 你的 profile + settings.json 完全不动；只丢 cc-switch 的 backup/history
# 如需恢复 shell-alias 工作流，备份过的 .bak 文件还在
```

---

## 第 7 步 — 下一次需要我（Claude）做什么时

下次直接说：

- "改 cc-switch 加 X" → 我会读 SPEC/PLAN/TASKS 上下文继续
- "v0.2.0 加 auto-update" → 走 spec-driven 流程
- "review 我的修改" → /agent-skills:code-review-and-quality

如果项目长期演进，记得让我帮你**升级 Tauri / Rust 依赖**，每 3-6 个月做一次。

---

## 速查卡片

```bash
# 日常开发
pnpm tauri dev                  # GUI + HMR

# Pre-commit 自检
pnpm format:check && pnpm lint && pnpm typecheck && pnpm test
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --all-targets

# 本地 unsigned build
pnpm tauri build --debug

# 想发布 (需要 Apple 凭据 + git tag)
pnpm tauri icon src-tauri/icons/icon.png   # 一次性
git tag v0.1.0 && git push --tags          # 触发 CI release pipeline

# 出事时回滚
ls ~/.claude/cc-switch/backups/            # 找最近的备份
cp ~/.claude/cc-switch/backups/<ts>.json ~/.claude/settings.json
```

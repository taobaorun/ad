# 代码风格指南

本文档定义 AD 项目的代码规范，所有贡献者（人类和 AI Agent）必须遵循。

> **为什么重要**：一致的风格降低代码审查的认知负担，让 diff 更清晰，并帮助 AI Agent 产出与代码库风格一致的代码。

AD 是 Tauri 项目，前端是 TypeScript + React，后端是 Rust。两侧规范分别约定。

---

## 通用规则（不可覆盖）

### 注释

所有代码注释必须使用**英文**。注释解释 *why*，而非 *what*——代码本身已经展示了做了什么。

```rust
// Good — explains why
// We don't fsync the parent dir after rename: APFS journals rename metadata,
// so on macOS this is safe. If we ever ship to Linux, revisit this.
std::fs::rename(&tmp, target)?;

// Bad — restates code
// Rename tmp to target
std::fs::rename(&tmp, target)?;
```

```typescript
// Good
// Tauri rejects long invoke payloads silently — keep the body under 1MB
const MAX_PAYLOAD_BYTES = 1024 * 1024;

// Bad
// Set max payload to 1MB
const MAX_PAYLOAD_BYTES = 1024 * 1024;
```

### 错误信息

异常 / `Result::Err` / 日志输出 / 面向用户的错误必须使用**英文**，且具有可操作性。

```rust
// Good
return Err(FsError::InvalidPath(format!(
    "profile id contains invalid characters: {id}"
)));

// Bad
return Err(FsError::InvalidPath("出错了".into()));
return Err(FsError::InvalidPath("error".into()));
```

### 日志

```rust
// Good
tracing::warn!(?err, profile_id = %id, "activate_profile: backup write failed");

// Bad
tracing::warn!("出错了");
tracing::info!("done");
```

### 作者信息

新建 Rust / TS 文件时，如有 doc comment 顶部块（多见于 `lib.rs`、`main.rs` 这种入口），不强制写 `@author`。在 `git log` 已经有完整作者信息的项目里，doc 内重复 `@author` 是冗余。

### UI / 面向用户文案

AD 是英文界面（面向开发者）。UI 字符串硬编码英文是允许的；如果将来加中文，必须走 i18n 资源文件，绝不硬编码在组件里。

---

## Rust 规范

### 命名

| 元素 | 规范 | 示例 |
|---|---|---|
| 函数 / 变量 / 模块 | `snake_case` | `activate_profile`, `profile_id` |
| 类型 / trait | `PascalCase` | `ProfileFile`, `ActivationLogEntry` |
| 常量 / `static` | `SCREAMING_SNAKE_CASE` | `MAX_BODY_BYTES`, `USER_AGENT` |
| 生命周期 | 短小写 | `'a`, `'src` |

### 错误处理

- 优先使用 `Result<T, E>` + `?`，避免 `unwrap()` / `expect()` 在生产路径
- 测试代码中可以 `unwrap()`
- 自定义错误类型用 `thiserror`
- 跨 crate 用 `anyhow` 携带 context

```rust
// Good
let bytes = std::fs::read(path)
    .map_err(|e| FsError::io(path.display().to_string(), e))?;

// Bad — silent swallow
let bytes = std::fs::read(path).unwrap_or_default();
```

### 模块组织

- 每个文件聚焦单一主题
- `mod.rs` 只做 re-export 和顶层文档
- 单文件目标 ≤ 500 行；超过就拆分

### 测试

- 单元测试：与代码同文件，`#[cfg(test)] mod tests`
- 集成测试：`src-tauri/tests/<topic>.rs`
- HOME 隔离：用 `AD_HOME` env var + `tempfile::TempDir`，必须 `#[serial_test::serial(home_env)]` 串行化
- 测试名形式：`<行为>_<条件>`，如 `paths_resolve_under_home_override`

```rust
#[test]
#[serial_test::serial(home_env)]
fn paths_resolve_under_home_override() {
    // Arrange / Act / Assert
}
```

---

## TypeScript / React 规范

### 命名

| 元素 | 规范 | 示例 |
|---|---|---|
| 变量 / 函数 / hook | `camelCase` | `activeProfile`, `useProfiles` |
| 组件 / 类型 / interface | `PascalCase` | `ProfileEditor`, `ActivationResult` |
| 常量 | `SCREAMING_SNAKE_CASE` | `MAX_RETRIES`, `TRAY_ID` |
| 文件名（组件） | `PascalCase.tsx` | `ProfileEditor.tsx` |
| 文件名（hook / util） | `camelCase.ts` | `useProfiles.ts`, `formatTimestamp.ts` |

### 组件

- 函数组件 + hooks，不写 class 组件
- props 类型显式声明：`function Foo(props: FooProps)` 或解构
- 避免无意义的 wrapping div，必要时用 Fragment
- 样式优先 Tailwind className，避免 inline style 除非动态色值（profile 颜色这种）

### 状态管理

- 跨组件共享状态走 Zustand store
- 单组件局部状态用 `useState` / `useReducer`
- store 命名：`useProfiles`、`useUI`，不带 `Store` 后缀

### 类型

- 优先 `interface` 表达对象 shape；联合 / 泛型用 `type`
- API / IPC 边界类型用 zod schema 校验，避免裸 `as Foo`
- 禁止 `any`；如必要写 `unknown` + narrowing

---

## Tauri IPC 边界

- 后端命令在 `src-tauri/src/commands/` 下，`#[tauri::command]` 标注
- 命令命名：`<resource>_<action>`，如 `read_history`、`activate_profile`
- 前端通过 `@tauri-apps/api` 的 `invoke` 调用，调用点统一封装在 `src/lib/tauri.ts`
- 后端返回错误用 `CmdResult<T> = Result<T, CommandError>`；前端 catch 转为 toast / 错误状态

---

## 错误处理

### Rust

```rust
// Good
match std::fs::read_to_string(&path) {
    Ok(s) => Ok(s),
    Err(e) if e.kind() == ErrorKind::NotFound => Ok(String::new()),
    Err(e) => Err(FsError::io(path.display().to_string(), e)),
}

// Bad — overly broad
let s = std::fs::read_to_string(&path).unwrap_or_default();
```

### TypeScript

```typescript
// Good
try {
  await invoke('activate_profile', { id });
  toast.success('Profile activated');
} catch (err) {
  console.error('activate_profile failed', err);
  toast.error(formatError(err));
}

// Bad — silent
try { await invoke('activate_profile', { id }); } catch {}
```

---

## 导入

### Rust

```rust
// std → external → crate-internal，每组一个空行
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::fs::paths::profiles_dir;
use crate::models::ProfileFile;
```

### TypeScript

```typescript
// react / 框架 → 第三方 → 内部 alias → 相对
import { useEffect } from 'react';

import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

import { Profile } from '@/types';
import { formatError } from '@/lib/format';

import './ProfileEditor.css';
```

提交前清理未使用的导入。

---

## Git 提交

遵循 [Conventional Commits](https://www.conventionalcommits.org/)：

```
<type>(<scope>): <英文摘要，祈使语气>
```

| Type | 用途 |
|---|---|
| `feat` | 新功能 |
| `fix` | 修复缺陷 |
| `refactor` | 不改变行为的代码重构 |
| `test` | 添加 / 更新测试 |
| `docs` | 文档变更 |
| `chore` | 构建脚本、CI、依赖更新 |

scope 对应模块：`tray`、`fs`、`commands`、`migration`、`ui`、`store`、`docs`...

```bash
# Good
git commit -m "feat(tray): wrap brand icon in active-profile color ring"
git commit -m "fix(fs): handle non-utf8 paths in atomic write"
git commit -m "refactor(commands): extract activation core into pure fn"

# Bad
git commit -m "update"
git commit -m "修复bug"
git commit -m "fixed stuff"
```

---

## 项目特定覆盖

| 规则 | 覆盖值 | 原因 |
|---|---|---|
| Rust crate 名 | `ad`（lowercase） | Cargo / npm 强制小写 |
| 显示名 | `AD` (productName, 窗口标题, README 标题) | 用户偏好 |
| `~/.claude/ad/` 目录名 | lowercase | 文件系统友好 |
| 测试 home 覆盖 env var | `AD_HOME`（uppercase） | env var 约定 |

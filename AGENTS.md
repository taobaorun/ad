# Agent Instructions

## 本机 AD 更新与构建清理

- 更新或替换 `/Applications/AD.app` 时，默认不创建或保留旧版应用备份；只有用户明确要求时才备份。
- 替换后，确认版本、签名校验通过且应用正常启动，再清理本次构建产物，包括 `src-tauri/target`、`dist`、`src-tauri/gen` 及 TypeScript/Vite 生成的缓存文件。

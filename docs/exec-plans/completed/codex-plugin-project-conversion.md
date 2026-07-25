# 实现 Codex Plugin 自动转换与项目隔离运行时

本 ExecPlan 已完成并归档。冻结 HTML 保留首次批准基线，最终实现与验证结果以本 MD 和 as-built 设计文档为准。

## 目的 / 全局视角

让 AD 在 Claude Code → Codex 转换中自动处理兼容 Plugin，不要求用户运行 Codex 命令。User scope 仍可写入所选 Codex installation；Project scope 创建 `~/.ad/codex-homes/<project-id>` 作为项目专属 `CODEX_HOME`，由 AD 合成继承默认用户配置，并把 marketplace、Plugin cache 与 enable state 全部隔离在该 runtime。只有从 AD 为此项目启动 Codex 时才生效。配置与 Plugin 不修改默认 `~/.codex`，但 ChatGPT 登录态作为用户级身份在所有项目复用；项目 API Key 等差异由 Profile 功能覆盖。

Disabled Plugin 显示无需变更；完整 package 优先安装到隔离 runtime；无法打包但有可移植组件时拆解到原生 Project 载体；LSP-only 等真正不兼容组件显示具体原因。

## 确认状态

- [x] **评审完成** — HTML 基线：`docs/exec-plans/completed/codex-plugin-project-conversion.html`
- [x] 用户已确认，开始执行（2026-07-20 16:51:56 +0800）

## 假设

- 设计基线为 `docs/design-docs/codex-plugin-conversion.md`；用户继续修改边界时，先同步更新 MD + HTML，再重新评审。
- “效果等同 Codex 命令”指对齐官方 marketplace/cache/config 契约，不表示转换过程中启动 Codex CLI/app-server。
- Codex 没有原生 Project Plugin enable/disable；Project Codex Home 是 AD 提供的 runtime isolation，不冒充官方项目配置能力。
- Project runtime 位于 `~/.ad/codex-homes/<project-id>`，不放进 repo，避免 auth/session/cache 被误提交。
- 默认 Codex installation 的 config、marketplace 与 Plugin cache 是只读继承源；共享 auth 可由 Codex 自己 refresh，是唯一明确写入例外。
- AD 物化 base config + Project Plugin overlay；repo `.codex/config.toml` 保留原位并由 Codex作为更高 precedence 层加载。
- ChatGPT 登录态必须跨 Project runtime 复用，不允许把“每项目重新登录”作为 fallback。文件型认证通过受控 `auth.json` symlink 共享；AD 不读取或复制凭据内容。
- Keychain credential key 按 canonical `CODEX_HOME` 隔离。Keychain-only Base 必须在 Preview 阻塞并引导到共享 Home/Profile 或 file store，不能假装已复用。
- 项目 API Key、模型和环境变量由 Profile 功能负责；本计划只消费可选 `profile_id`，不实现 Profile 的凭据存储。
- Base 已启用 Plugin 若要在 project runtime 继承，package 必须形成独立镜像；不创建可写回默认 home 的 symlink。
- `.codex/config.toml`、Project hooks/rules 仍受 Codex project trust 控制；AD 不替用户更改 trust。
- LSP→MCP bridge 不属于本轮，不静默引入第三方依赖。
- Remote marketplace 的 Prepare 可以联网，但只能写入 AD-owned 临时 stage；Preview 前必须固定 ref/revision 与 tree digest，不能直接修改任何有效 Codex Home。
- 当前工作树已有用户修改：`src/lib/startupSurface.ts`、`src/main.tsx` 与 startup/theme 测试。本任务不得覆盖、格式化或提交这些无关变更。

## 影响范围

预计涉及以下文件；实施时允许按职责新增小模块，但不得扩散到无关设置编辑器：

```text
src-tauri/src/agents/
├── claude_ports/plugins.rs             [M] source inventory
├── codex.rs                             [M] derived installation discovery
├── codex_runtime.rs                     [M] project-bound CODEX_HOME launch
├── codex_ports.rs                       [M] selected installation resolution
├── codex_plugins.rs                     [M] isolated install targets/plans
├── project_codex_runtime.rs             [N] identity, registry, config synthesis
├── project_codex_auth.rs                [N] shared auth binding metadata
├── plugin_conversion.rs                 [N] descriptor/component classification
├── conversion.rs                        [M] dispositions/resolutions
├── conversion_route.rs                  [M] runtime/package/component plans
├── capabilities.rs                      [M] Directory storage
├── operations.rs                        [M] Directory state/receipt
├── execution_fs.rs                      [M] stable tree digest/staging/COW copy
├── execution.rs                         [M] directory backup/apply/rollback
└── mod.rs                               [M] exports
src-tauri/src/fs/
├── paths.rs                             [M] project runtime/state paths
└── git.rs                               [M] structured safe Git invocation
src-tauri/src/commands/
├── agents.rs                            [M] runtime prepare/inspect IPC
└── terminal.rs                          [M] launch selected derived installation
src-tauri/tests/
├── project_codex_runtime.rs             [N]
├── plugin_conversion.rs                 [N]
├── conversion_route.rs                  [M]
└── conversion_execution.rs              [M]
src/lib/agentTypes.ts                    [M]
src/components/
├── AgentConversionDialog.tsx            [M]
├── AgentConversionArtifacts.tsx         [M]
└── ProjectDetail.tsx                    [M] runtime status/launch
src/i18n/locales/{zh,en}.json             [M]
tests/components/
├── AgentConversionDialog.test.tsx       [M]
└── ProjectDetail.test.tsx               [M]
docs/design-docs/architecture.{md,html}   [M, as-built]
docs/design-docs/agent-conversion-workbench.{md,html} [M, as-built]
docs/product-specs/multi-agent-support.{md,html}       [M, as-built]
```

## 关键决策

1. Project Plugin 默认安装到 AD Managed Project Codex Home，不再升级写入默认 User home。
2. Project runtime 是派生 Agent installation；所有 port 与 LaunchRecipe 都绑定其 installation id 和 canonical project path。
3. AD 直接复现 Codex 磁盘契约，不调用 Codex 进程；用户点击 Launch 后才正常启动 Codex。
4. `~/.codex/config.toml` 是只读 base。AD 结构化合成 project home `config.toml`；repo `.codex/config.toml` 继续由 Codex作为更高层加载。
5. 完整 package 优先，组件拆解是 legacy fallback，避免丢失 Plugin identity/version/upgrade 语义。
6. Directory 是 ExecutionEngine 的一等资源，project marketplace/cache 不能绕开 plan、backup 与 rollback。
7. 不 symlink 默认 config/cache。继承的 packages 使用独立副本；APFS clonefile 只能作为复制优化。
8. ChatGPT 登录态属于用户身份：文件认证通过 auth symlink 复用且不读内容；Keychain-only 必须阻塞或切换共享 Home/Profile 路线，绝不要求每项目重新登录。
9. 项目 API Key 属于 Profile：Project runtime 仅保存/接收 `profile_id`，不把 API Key 纳入 Plugin 转换。
10. Disabled Plugin 是 unchanged；LSP-only 不生成空安装。
11. 以 OpenAI importer fixture 形状建立 parity tests，但不引入或 vendor Codex crate。
12. Plugin cache 与 marketplace 先落盘，`config.toml` 最后写入并作为唯一激活点；任何失败都不能留下 enabled 但无 package 的状态。

## 进展

- [x] (2026-07-20 17:08 +0800) Milestone 1：建立 Project runtime / Plugin conversion contract 的 RED tests。（验证：默认 config/plugin 零写入、共享登录不重复、runtime identity、disabled/LSP/package dispositions 均先失败）
- [x] (2026-07-20 17:24 +0800) Milestone 2：实现 ProjectCodexRuntime registry、config synthesis 与 shared auth binding。（验证：base inheritance、auth link 元数据、Keychain-only 阻塞、project overlay、unknown keys、path normalization、base/external-change conflict 测试通过）
- [x] (2026-07-20 17:37 +0800) Milestone 3：扩展 Directory MutationPlan 与安全复制。（验证：目录 create/replace、tree digest、备份、补偿、rollback、symlink escape 测试通过；COW 保持非必要优化，当前使用普通独立复制）
- [x] (2026-07-20 17:56 +0800) Milestone 4：实现 isolated marketplace/package install 与 inherited package mirror。（验证：所有 target 位于 project home；默认 config/cache tree digest 不变；重复 Apply unchanged；连续安装保留既有 overlay）
- [x] (2026-07-20 18:18 +0800) Milestone 5：实现 Plugin 分类、package-first route 与 partial package fallback。（验证：Project package、portable transform、partial、LSP-only 均产生正确 plan/result）
- [x] (2026-07-20 18:30 +0800) Milestone 6：实现 derived installation launch、Profile 接口、runtime UI 与 i18n。（验证：LaunchRecipe 带精确 `CODEX_HOME`/cwd/可选 profile；共享用户登录无需重复登录；敏感值不展示）
- [x] (2026-07-20 18:59 +0800) Milestone 7：as-built MD/HTML 已同步；多轴审查修复 stage allowlist 与连续安装 overlay 丢失；前端/Rust 全量门禁、Tauri 生产构建和真实截图项目只读验收通过。

## 意外发现

- 发现：仓库已支持发现 distinct `CODEX_HOME` installation，LaunchRecipe 也已有 env map，但当前 discovery 只看进程环境和默认 home，不能持久注册 project-derived installation。
  证据：`src-tauri/src/agents/codex.rs::discover_codex_candidates`、`codex_runtime.rs` 与 `terminal/mod.rs::render_launch_command`。
- 发现：Codex 官方公开 `CODEX_HOME`，并明确它覆盖 config、auth、logs、sessions、skills 和 package metadata；目录必须预先存在。
  证据：Codex Environment variables（2026-07-20 核验）。
- 发现：Codex 配置 precedence 是 CLI > Project > Profile > User > System。切换 `CODEX_HOME` 会替换 User/Profile root，不会继续读取默认 `~/.codex`。
  证据：Codex Config basics / Advanced config 与 OpenAI config loader source。
- 发现：profiles 可以原生叠加同一 `CODEX_HOME` 的 base config，但不能从另一个 home include `~/.codex/config.toml`。
  证据：Codex profile 文档与 `codex-rs/core/src/config`。
- 发现：credential file 位于 `CODEX_HOME/auth.json`；keyring/backend 的实现也会使用 home identity。公开文档未承诺跨 home 复用认证。
  证据：Codex Authentication 文档与 `codex-rs/login/src/auth/storage.rs`。
- 发现：FileAuthStorage 读取和刷新会跟随 symlink，而 logout 的 `remove_file` 只删除 project link；Auto/Keyring storage 又按 canonical home 派生 key，不能把两者当成同一种共享机制。
  证据：`codex-rs/login/src/auth/storage.rs` 的 load/save/delete 与 `compute_store_key`。
- 发现：Plugin store、remote sync、marketplace materialization 全部以传入的 `codex_home` 为根，因此 isolated home 能隔离 package 与 enable state。
  证据：`codex-rs/core-plugins/src/store.rs`、`marketplace_add.rs` 与 remote sync source。
- 发现：当前 CodexPluginsPort 只支持 User scope list/toggle，install 固定 Unsupported；Project route 永远没有真实 package target。
  证据：`src-tauri/src/agents/codex_plugins.rs` 与 `conversion_route.rs`。
- 发现：截图项目的 `jdtls-lsp@claude-plugins-official` 实际为 disabled，安装包只含 `lspServers.jdtls`；Codex manifest 不含 LSP。
  证据：只读检查 harness 项目、Claude Plugin manifest 与 OpenAI Plugin manifest source。
- 发现：Codex 官方 issue #18115 仍在请求 repo-scoped marketplace registration 与 Plugin enable/disable。
  证据：<https://github.com/openai/codex/issues/18115>（2026-07-20 核验）。
- 发现：初版 Project install 只要求 stage 位于 `~/.ad`，边界过宽；安全审查要求只接受 `~/.ad/staging/codex-plugin-conversion` 的物理目录。
  证据：新增越界 stage 拒绝回归测试，并在 lexical + canonical 两层校验 containment。
- 发现：初版后续单次安装会从 Base 重新合成 config，可能移除已安装的 Project Plugin。
  证据：新增连续安装回归测试；现在先从已验证的 generated config 恢复 Project marketplace/plugin overlay，再叠加新 Plugin。

## 决策日志

- 决策：把 Project package target 从默认 User home 改为 AD Managed Project Codex Home。
  理由：用户要求真正按项目生效且不污染 `~/.codex`；完整 runtime isolation 能保留 Plugin package 语义。
  日期/作者：2026-07-20 / Codex，用户已确认
- 决策：由 AD 合成 base config inheritance，config/cache 不使用 symlink。
  理由：Codex 无跨 home include；config/cache 的运行时写操作会穿透修改默认 home。
  日期/作者：2026-07-20 / Codex，用户已确认
- 决策：Project runtime 默认复用用户 ChatGPT 登录态，文件认证通过 `auth.json` symlink 实现。
  理由：登录态是本机用户身份而非项目配置；AD 只管理引用元数据，不读取或复制 token。Codex refresh 写回用户 auth 是预期共享语义。
  日期/作者：2026-07-20 / Codex，用户已确认
- 决策：Keychain-only 不降级为每项目登录；项目 API Key 归 Profile 管理。
  理由：Codex Keyring key 按 `CODEX_HOME` 派生，无法用文件链接复用；重复登录违背产品需求，而差异化 API Key 本来就是 Profile 的职责。
  日期/作者：2026-07-20 / Codex，用户已确认
- 决策：保留 partial package fallback，但完整 package 优先。
  理由：package 保留 identity/version/components；legacy Plugin 的 Skills、Commands→Skills 与 MCP 可在 Project package 中继续生效，LSP/Hook/App/Agent 作为 residual 明确呈现。
  日期/作者：2026-07-20 / Codex，用户已确认；实施中按真实 carrier 收敛
- 决策：本轮不实现完整 Profile 管理和 LSP/MCP bridge，只接入既有/后续 Profile 选择边界。
  理由：Profile 独立负责 API Key 等差异化配置；LSP bridge 是新供应链与 sidecar，不属于 Plugin 等价转换。
  日期/作者：2026-07-20 / Codex，用户已确认
- 决策：AD 自动化安装明确拆为 Prepare/Preview、Apply、Launch，且 config 最后激活。
  理由：用户需要看清 AD 如何达到 Codex install 的等价效果；分阶段可以把网络解析、确定性写盘和进程启动隔离开，并保证失败时没有悬空 enabled state。
  日期/作者：2026-07-20 / Codex，用户已确认

## 结果回顾

- Project runtime：以 canonical project path + Base installation 生成稳定 id，注册在 `~/.ad/state/codex-project-runtimes`，运行目录位于 `~/.ad/codex-homes/<project-id>`。AD scoped launch 注入精确 `CODEX_HOME`、cwd 和可选 `--profile`；Base/generated config 过期时拒绝启动。
- 配置与身份：Base TOML 只读合成，未知键保留，已知相对路径正规化；Project marketplace/plugin overlay 在连续安装时保持。文件型 `auth.json` 只创建受控 symlink，未读取或复制凭据；Keychain-only 明确阻塞，不要求每项目重复登录。
- Plugin 自动化：Prepare 在专用 AD stage 解析和转换 package；Apply 通过 Directory-aware ExecutionEngine 依次写 auth link、marketplace、versioned cache，最后写 config 激活。Base 已启用 Plugin 形成独立镜像；相同 digest 幂等；故障补偿与 digest-protected rollback 均有测试。
- 转换结果：Disabled 为 unchanged；原生 Codex package 为 copy；Claude Skills/Commands/MCP 可转换为 package；同时含 LSP/Hook/App/Agent 等组件时为 partial 并保留 residual；LSP-only 为具体 unsupported。User install 仍使用 Codex 官方 marketplace flow。
- 审查修复：安装 source 从任意 `~/.ad` 收紧到 `~/.ad/staging/codex-plugin-conversion` 的 lexical + canonical containment；新增连续安装回归，避免后续 Plugin 覆盖既有 Project overlay。无未解决 Critical/Required finding。
- 全量验证：前端 `typecheck`、`lint`、100 tests、Vite build 全部通过；Rust all-targets 为 lib 216 passed / 4 ignored，加全部 integration tests，通过 `cargo check` 与严格 clippy；`pnpm tauri build` 生成 `AD.app` 和 `AD_1.0.1_aarch64.dmg`。
- 真实 harness 只读验收：`jdtls-lsp@claude-plugins-official=false`，官方 catalog 条目仅有 `lspServers=jdtls`，预期 disposition 为 `unchanged/source_disabled`。未执行 Apply；项目 Claude settings、Base Codex config、项目 Codex config 与 marketplace catalog 的 SHA-256 前后完全一致。
- 后续边界：Codex User Plugin acquisition、Keychain 共享 Home 产品路线、完整 Profile 凭据管理和 LSP→MCP bridge 仍需独立计划；本轮不引入隐式 scope escalation 或第三方 sidecar。

## 上下文和方向

AD 的转换由 `ClaudeToCodexRoute` 盘点 Settings / Skills / Plugins，构造 backend-owned MutationPlan，再由 ExecutionEngine 校验 digest、备份、原子应用和 rollback。现有 Project scope 已能写 `.codex/config.toml` 与 `.agents/skills`，但 PluginsPort 把 Codex install 降级为 marketplace 手工流程。

本任务增加一个更高层抽象：Project scope 可解析到一个 derived Codex installation。默认 Codex home 只作为 base read-set；所有 package/config writes 解析到 derived installation root。转换与 runtime preparation 共享相同的 ProjectCodexRuntime registry，避免 Preview 指向 isolated home、Launch 却回到默认 home。

网络获取可在 Preview 后 stage 到 AD 临时目录，但 stage 不进入任何有效 Codex home。Apply 在 execution lock 内重新核验 base/source/target digest。Launch 前若 base digest 变化，必须先生成 refresh plan；不能静默用过期继承配置。

## 工作计划

### Milestone 1 — Contract 与 RED tests

建立临时 default home、project home 与 repo fixture。测试首先证明：Project package conversion 不得触碰 default config/plugin store；文件登录态由受控 link 复用且不读取内容；Keychain-only 不会退回每项目登录；derived installation 必须稳定；LaunchRecipe 必须包含正确 `CODEX_HOME`；disabled、enabled LSP-only、native package、legacy portable 与 inherited user Plugin 得到不同 disposition。

### Milestone 2 — Project runtime 与 config synthesis

新增 `project_codex_runtime.rs`，以 canonical project path + base installation id 生成稳定 identity，状态写入 `~/.ad/state/codex-project-runtimes`，home 写入 `~/.ad/codex-homes`。新增 auth binding，只保存 Base auth path、link path、backend kind 与状态，不读取 credential bytes。File backend 创建受控 symlink；Keychain-only 返回可行动的阻塞结果。

合成器读取 base `config.toml`，保留 unknown TOML，覆盖 allowlisted Plugin overlay。建立官方 schema 对应的 path-bearing key normalizer；已知相对路径转换为基于 base config directory 的绝对路径，未知疑似路径不猜测。记录 base/overlay/generated digests。生成文件被外部修改时返回 conflict，不覆盖。

### Milestone 3 — Directory 安全执行

为 ResourceStorage / TargetState / receipt 增加 Directory。目录摘要稳定排序 relative path、entry kind、mode 与 content digest，不跟随越界 symlink。实现 staging copy、可选 APFS clonefile、普通 copy fallback、同父目录 rename、operation backup、逆序补偿与 digest-protected rollback。

### Milestone 4 — Isolated Plugin store

实现完整自动化流水线：Prepare 把 local/Git marketplace 放到有效 home 之外的 AD-owned stage，校验 catalog、resolved revision、Plugin source 和 manifest；可映射的 Claude package 在 stage 中生成最小 `.codex-plugin/plugin.json`，并固定 marketplace/package tree digest。Preview 展示 route、version、components、targets、residual 和 conflicts。

Apply 加 project execution lock 并复核全部 digest，然后按 `runtime/auth link → marketplace snapshot → plugins/cache/<marketplace>/<plugin>/<version> → config.toml` 顺序执行。目录使用同父 staging + rename；`config.toml` 必须最后写，作为 Plugin 的激活点。后置校验重新读取 config/manifest/digest 后才生成 complete receipt；失败按逆序补偿。

Base 中已启用的 Plugin 必须解析 package source并镜像。缺失 cache、manifest 无法解析或 source 不可证明时，runtime refresh 阻塞并逐项显示，不能只复制 enabled config。

不得调用 Codex CLI/app-server。Git 使用结构化 `Command::args`，不能将 marketplace source 拼进 shell。

### Milestone 5 — Package-first conversion 与 fallback

在 Claude inventory 中读取声明层、known marketplaces、installed registry 与 manifest，只提取 Plugin 必需字段。Auth 模块只执行 `symlink_metadata`/路径校验，不打开认证文件。完整 package 先进入 isolated store；legacy Skills/Commands/Hooks/MCP 在 package 不可用时进入 project native carriers。Partial conversion 保留所有未迁移组件，LSP-only 输出具体 reason。

### Milestone 6 — Launch 与 UI

扩展 Codex discovery/registry，持久呈现 project-derived installation。CodexLaunchPort 对 derived context 生成：

```text
cwd = <canonical-project>
env.CODEX_HOME = ~/.ad/codex-homes/<project-id>
```

所有 terminal backend 复用现有安全 quoting。Launch 前运行只读 freshness check；过期时回到 Preview/Apply，不在启动路径偷偷改文件。

UI 显示 base home、project home、inheritance freshness、Plugin 数量、认证复用方式、可选 Profile 与明确的 scoped launch。默认状态是“复用用户登录”，不出现“项目需要登录”。Keychain-only 显示阻塞原因与共享 Home/Profile 或 file store 选择。文案同步 zh/en，不读取凭据来推断登录内容。

### Milestone 7 — 文档、审查与交付

更新 architecture、workbench design 与 product spec。执行多轴 code review，重点检查 installation confusion、default-home write leak、path relocation、command injection、symlink escape、TOCTOU、directory digest、sensitive-data exclusion、补偿顺序与 unsupported 假成功。

完成全量门禁和 `pnpm tauri build`。真实 harness 项目只做 preview：`jdtls-lsp=false` 为 unchanged；不在真实 default/project Codex home 执行安装。

## 验证和验收

定向门禁：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test project_codex_runtime
cargo test --manifest-path src-tauri/Cargo.toml --test plugin_conversion
cargo test --manifest-path src-tauri/Cargo.toml --test conversion_route
cargo test --manifest-path src-tauri/Cargo.toml --test conversion_execution
pnpm test -- tests/components/AgentConversionDialog.test.tsx tests/components/ProjectDetail.test.tsx
```

全量门禁：

```bash
pnpm typecheck
pnpm lint
pnpm test
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm tauri build
```

行为验收：

1. Project package Apply 前后，default `~/.codex/config.toml`、marketplace 和 Plugin cache tree digest 完全不变；auth refresh 不计入该断言。
2. Project package 的 config/cache/marketplace 全部位于稳定 project home。
3. 从 AD scoped launch 时 Plugin 可见；相同 cwd 普通启动默认 Codex 时 Project Plugin 不生效。
4. Base config unknown keys 保留；Project repo config 仍以更高 precedence 生效。
5. Base 改变触发 refresh plan；generated config 外部修改触发 conflict。
6. Base 已启用 Plugin 有独立 package mirror；缺失 package 不生成悬空 enable state。
7. 文件型用户登录可跨全部 project runtime 复用，无重复登录；AD 只处理 auth link 元数据，凭据内容及 session/log/SQLite 不进入 read-set、backup、receipt 或日志。
8. Keychain-only Base 不生成“可用”runtime，也不提示项目单独登录；它返回共享 Home/Profile 或 file store 的明确 resolution。
9. 选择项目 Profile 时，LaunchRecipe 只引用 `profile_id`/其非敏感启动结果；API Key 不进入 Plugin MutationPlan。
10. Disabled 为 unchanged；LSP-only 为 `unsupported_component:lsp`；partial 不丢信息。
11. traversal、越界 symlink、同名异源 marketplace、非法 name/version 全部拒绝。
12. Apply 失败完整补偿；rollback 不覆盖 Apply 后的外部修改。
13. Remote Prepare 只修改临时 stage；最终 Preview 固定 source/ref/revision/tree digests，Apply 时任何 drift 都使 plan 失效。
14. 在 marketplace/cache 尚未全部就绪或任一步失败时，project config 中不得出现新 Plugin 的 `enabled = true`；成功后 cache path 必须是 `plugins/cache/<marketplace>/<plugin>/<version>`。
15. Apply 不启动 Codex；Launch 只在 freshness 全部匹配时注入 `CODEX_HOME`/cwd/profile，过期时回到 Preview，不偷偷刷新。

## 幂等性和恢复

Preview 只读；stage 不被默认或 project Codex home 发现。Apply 在 project-runtime execution lock 内复核 Claude source、base config、base inherited package 与 project targets。相同版本/摘要为 unchanged；相同版本不同摘要为 conflict。

Config 与 Directory target 写前进入 operation backup，失败逆序补偿。Rollback 核对 post-apply digest。Default config/plugin store 永不进入 mutable resource/backup，仅作为 immutable read precondition；auth link 本身可恢复，但其目标内容永不备份。测试只使用临时 `AD_HOME` / default `CODEX_HOME`，不读取真实用户凭据。

Project unregister 不删除 runtime。独立 cleanup flow 必须先展示 config/package 与 Codex-owned session/logs 的路径和不可恢复影响，再允许删除；共享 auth link 只删除链接，绝不删除 Base auth。cleanup 不属于本轮转换 Apply。

## 接口和依赖

优先不新增第三方依赖；复用 `serde_json`、`toml`、`sha2`、`tempfile`、`uuid` 与现有原子写。APFS clonefile 如需 FFI/crate，必须保持普通 copy fallback，不让性能优化成为正确性依赖。

计划结束时至少存在等价接口：

```rust
enum ResourceStorage { File, Symlink, Directory }

struct ProjectCodexRuntime {
    project_id: String,
    project_path: PathBuf,
    base_installation_id: InstallationId,
    runtime_installation_id: InstallationId,
    runtime_home: PathBuf,
    base_config_digest: Option<ContentDigest>,
    generated_config_digest: ContentDigest,
    auth_binding: SharedAuthBinding,
    profile_id: Option<String>,
}

enum SharedAuthBinding {
    FileSymlink { source: PathBuf, target: PathBuf },
    KeychainRequiresSharedHome,
    MissingBaseLogin,
}

enum PluginConversionTarget {
    ProjectRuntimePackage,
    ProjectComponents,
    UserPackage,
    Unsupported,
    Unchanged,
}

struct ConfigSynthesisInput {
    base_config: Option<TomlDocument>,
    project_plugin_overlay: PluginOverlay,
}

struct LaunchRecipe {
    program: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: String,
}
```

具体类型名可调整，但 derived installation identity、default config/plugin read-only、共享用户登录、Profile 边界、config inheritance、isolated package store、sensitive-data exclusion 与 project-bound launch 语义不可弱化。

## 里程碑规划

1. Runtime contract：稳定 identity、default config/plugin 零写入、共享用户登录、launch binding 可独立验证。
2. Config synthesis：继承、overlay、path relocation、freshness/conflict 可独立验证。
3. Safe directory engine：与 Plugin 业务解耦验证 backup/apply/rollback。
4. Isolated store：marketplace/package/config 在 project home 独立可用。
5. Conversion + UI + shipping：package-first、fallback、真实 preview、全量门禁。

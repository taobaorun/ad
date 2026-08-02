# 在 Skill 冲突中列出每个候选的具体来源地址

本 ExecPlan 是一个活文档。`进展`、`意外发现`、`决策日志`、`结果回顾` 必须随工作推进保持更新。

## 目的 / 全局视角

当 AD 发现两个或更多同名 Skill 时，冲突提示不再只显示来源名称或“未知来源”，而会逐项列出可核验的来源信息。来自 AD Skill Catalog 的候选显示来源名称、Git 或本地地址、分支和子目录；无法回溯 Catalog 的外部/Agent 原生候选显示其“当前安装位置”，并明确说明这不是源仓库地址。

用户无需猜测两个 `html-artifact` 分别来自哪里，就能前往对应来源保留、移除或重命名重复 Skill。来源信息只用于展示和排查，现有 Preview → Apply 写入合同仍只接受 backend-issued opaque key，绝不接受 UI 回传的地址或物理路径。

## 确认状态

- [x] **用户已确认** — HTML 基线：`docs/exec-plans/active/skill-conflict-source-addresses.html`
- [x] (2026-08-02) HTML 已冻结；SHA-256：`256af90b5ef58fad58a92f73923d4e5e3c0e7c94fb1c9acc871de0e336b81981`

## 假设

- 本轮覆盖 Project Agent Workspace 的 Skill 冲突；Plugin 冲突继续显示声明层级，不伪造不存在的 Plugin acquisition 来源。
- “具体来源地址”包含两种明确区分的事实：Catalog acquisition source（Git/local）与无法回溯 Catalog 时的当前 installed path。
- Git 来源的 branch 与 subdirectory 是来源身份的一部分；只显示仓库根地址不足以区分 monorepo 或多分支来源。
- 来源地址是 display-only provenance，不是 mutation target；项目安装 link、immutable artifact path、ownership target path 不会冒充 acquisition source。
- 本地绝对路径可以展示。AD 的 Skill Sources 界面和 catalog IPC 已经展示这类路径，且本功能面向本机配置管理。
- Git 地址在进入 Project Workspace inventory 前必须移除 URL userinfo、password、query 和 fragment；SCP 风格 SSH 地址（例如 `git@host:org/repo.git`）保持可辨识。
- 无法得到安全、可信地址时显示“来源地址不可用”，不根据 opaque `sourceId` 猜测。
- 当前工作树已有未提交的冲突引导、i18n、测试和 `AGENTS.md` 清理规则改动；实施必须增量叠加，不覆盖或拆散这些已有修改。

## 产品合同

### 需求

- **R1 — 冲突候选可辨识。** 每个同名 Skill 候选都单独显示来源类型、可读名称和具体地址；候选顺序稳定，列表不再以 description 代替来源地址。
- **R2 — Catalog 来源完整。** Git/local Catalog 候选显示 catalog 中的 `location`；存在 branch 或 subdirectory 时同时显示，安装后仍优先显示 acquisition source，而不是项目安装 link。
- **R3 — 非 Catalog 来源诚实降级。** 外部或 Agent 原生 Skill 无 Catalog 记录时显示“当前安装位置”；若连安全安装位置也不可用，则显示明确 fallback。
- **R4 — 地址可完整阅读。** 长 URL 和本地路径使用等宽字体并允许换行，不以省略号截断；来源名称与地址有清晰的视觉和无障碍语义。
- **R5 — 地址只读。** 地址不变成可点击执行入口，不进入 install/update/remove/enable/disable 请求，也不能取代 `resourceKey`、`targetId` 或 ownership evidence。
- **R6 — 敏感信息不泄漏。** Project Workspace inventory 不输出 Git URL credentials、query token 或 fragment；脱敏在 Rust IPC 输出边界完成，前端不承担安全净化。
- **R7 — 兼容现有资源。** Plugin inventory、Settings inventory、已有 Skill 操作、coverage 和冲突 fail-closed 行为保持不变。

### 验收示例

1. 两个 Catalog 来源都包含 `html-artifact`：冲突卡列出两个来源名称和各自不同的 Git/local 地址；branch/subdirectory 可见。
2. Catalog Skill 安装到项目后：冲突卡仍显示 Catalog 原地址，不显示 `<project>/.claude/skills/...` 或 `<project>/.agents/skills/...` 作为源仓库。
3. Codex/Claude 原生目录中存在没有 Catalog 记录的同名 Skill：候选标记为“当前安装位置”，并显示各自真实目录，用户能区分 `.codex`、`.agents`、`.claude` 等落点。
4. Catalog 中存在 credential-bearing HTTPS 地址：inventory 和渲染 DOM 中均不含 username、password、query 或 fragment；仓库 host/path 仍可识别。
5. 用户点击“管理 Skill 来源”或“重新检查”：现有行为继续工作；任何 Preview/Apply 请求都不包含来源 address。

### 范围边界

本轮不会自动选择胜出来源、合并两个 Skill、自动删除冲突、改变 Catalog source CRUD，也不会新增“打开路径/打开 URL/复制地址”动作。它只把后端已经掌握的来源事实安全地带到冲突 UI，并保持现有人工解决流程。

## 影响范围

```text
src-tauri/src/agents/
├── resource_inventory.rs              [M] display-only provenance source contract
├── collection_inventory.rs            [M] source-group aggregation and precedence
├── collection_skills.rs               [M] catalog and installed-path observations
└── skill_catalog.rs                    [M] reusable safe display-location formatter

src/lib/
└── agentResourceInventoryTypes.ts      [M] strict Zod source schema

src/components/
└── AgentCollectionPanel.tsx            [M] structured conflict source rows

src/i18n/locales/
├── zh.json                              [M] source kind/address/fallback labels
└── en.json                              [M] synchronized English labels

src-tauri/src/agents/
├── collection_actions_tests.rs         [M] source-group inventory regressions
└── skill_catalog.rs                    [M] formatter unit tests

tests/
├── lib/agentTypes.test.ts               [M] strict IPC contract coverage
└── components/AgentCollectionPanel.test.tsx [M] conflict source UI and action boundary
```

`src/lib/tauri.ts` 已统一通过 `ProjectWorkspaceInventorySchema.parse` 校验返回值，不需要修改。`project_inventory.rs` 的 inventory revision 会自然纳入新增字段，不需要建立第二套 revision 逻辑。

## 关键决策

1. **来源信息属于 `ResourceProvenanceView`。** 同名冲突已经按 `(logicalId, sourceId)` 形成多个 resource；每个 resource 对应一个来源。把 source 放在 provenance 上可同时覆盖“已安装 declaration”和“尚未安装、declarations 为空”的 Catalog 候选。
2. **使用结构化来源，不拼接展示字符串。** Rust 输出 `kind / displayName / location / branch / subdirectory`，React 决定标签和排版，双语文案不进入后端。
3. **来源优先级为 Catalog acquisition source > installed path > unavailable。** 组内同时存在 Catalog observation 与已安装 snapshot 时优先 Catalog；只有无法回溯 Catalog 时才显示 installed path，并明确改变标签。
4. **Mutation identity 继续 opaque。** 来源字段仅出现在 inventory response，不加入 `ProjectCollectionActionRequest`，也不参与 target resolution。地址变化只会使 inventory revision 更新，不能直接授权写盘。
5. **后端完成 URL 脱敏。** formatter 对标准 URL 移除 userinfo、query、fragment；local path 与合法 SCP SSH 地址保持原样。无法安全格式化时返回 unavailable，而不是输出可能含 secret 的原字符串。
6. **保持可选字段兼容。** `provenance.source` 是 optional；Plugin 与未知旧来源不必伪造 source。严格 Zod schema 仍拒绝未知字段和空地址。
7. **冲突 UI 使用竖向结构化行。** 每项以稳定 resource/declaration key 标识，来源名称在上、完整地址在下；地址 `break-all`/等宽显示，移除当前 `truncate`。

## 实施单元

### U1 — 建立来源展示合同与安全 formatter

**目标：** 在 Rust 与 TypeScript 之间建立可选、严格、只读的来源 provenance，并先用 RED tests 固定安全边界。

**文件：** `resource_inventory.rs`、`skill_catalog.rs`、`agentResourceInventoryTypes.ts`、对应 Rust/TypeScript contract tests。

**做法：**

- 新增 source kind 与 source view；字段保持 camelCase IPC / snake_case enum 约定。
- 将 Git display-location formatter 放在 Catalog domain 可复用位置，覆盖 HTTPS、`ssh://`、SCP 和 local path。
- 前端 Zod schema 对 kind、非空 location、可选 branch/subdirectory 做 strict 校验。
- 明确注释 source 是 display-only，action request schema 不增加 address/path。

**完成证据：** formatter 测试证明 credentials/query/fragment 不出现在输出；Zod 接受合法 Git/local/installed 来源并拒绝空地址、未知 kind 与多余字段。

### U2 — 在 Skill inventory 保留并归并真实来源

**目标：** 从 catalog/snapshot/Claude fallback observation 生成正确 source，并在 source-group 汇总时保留最可信来源。

**文件：** `collection_inventory.rs`、`collection_skills.rs`、`collection_actions_tests.rs`。

**做法：**

- `CollectionObservation` 携带 optional source。
- Catalog observation 使用 entry 的 display name、source type、safe location、branch、subdirectory。
- 已安装 snapshot 和 Claude fallback 生成 installed-path provenance；不得使用 immutable artifact path 或 ownership target metadata伪造 acquisition source。
- group 汇总时优先 Catalog；同 sourceId 出现互相矛盾的 Catalog metadata 时 fail closed/diagnostic，不静默选择。

**完成证据：** 两个同名 Catalog Skill 返回两个 `conflict` resources 和不同 source；安装后的 group 仍返回 Catalog 地址；无 Catalog 记录的候选返回 installed-path 标签。

### U3 — 渲染可核验的冲突来源清单

**目标：** 把当前 description chips 改成每个候选都能完整阅读的来源行。

**文件：** `AgentCollectionPanel.tsx`、`zh.json`、`en.json`、`AgentCollectionPanel.test.tsx`。

**做法：**

- `conflictCandidates()` 返回结构化候选，优先 resource provenance source，Plugin declaration 继续使用层级 fallback。
- Skill 冲突行显示来源名称、来源类型、完整地址，以及可选 branch/subdirectory。
- installed path 使用“当前安装位置”，Catalog 使用“Git 来源/本地来源”；缺失信息显示“来源地址不可用”。
- 保留“管理 Skill 来源”“重新检查”以及现有 loading/error/race 保护。

**完成证据：** 组件测试在同一 alert 内找到两组来源名称和完整地址；长地址不使用 truncate；按钮行为和 reload 次数保持正确。

### U4 — 全量验证、安装与收尾

**目标：** 证明合同、UI、构建和本机安装均稳定，并清理可再生构建产物。

**做法：**

- 运行聚焦 Rust/React/Zod 测试后执行全量前端、Rust、format、lint、typecheck 和 release build。
- 安装最新 `AD.app` 到 `/Applications`，在真实冲突场景确认两个来源地址可见且写操作仍暂停。
- 按 `AGENTS.md` 在安装完成后清理 `src-tauri/target/`、`dist/`、`src-tauri/gen/` 和未跟踪生成配置，保留正式安装版本与用户数据。
- 更新本 MD 的实际证据；HTML 保持冻结；完成后 MD + HTML 一起移到 `docs/exec-plans/completed/`。

**完成证据：** 全量门禁通过、正式 app 可观察到来源地址、清理后 `git status --short` 仅包含预期源码/文档改动。

## 进展

- [x] (2026-08-02) 完成只读代码研究，确认 Catalog 已保存完整来源但 Project Workspace inventory 丢弃它；确认同名冲突已按 source group 建模。
- [x] (2026-08-02) 创建 MD + HTML ExecPlan 评审基线。
- [x] (2026-08-02) 用户回复 LGTM，批准并冻结 HTML。
- [x] (2026-08-02) U1：来源合同与安全 formatter 完成。RED：TypeScript 2 个合同测试失败、Rust 9 个缺失合同编译错误；GREEN：Rust lib 327 passed / 1 ignored、TypeScript contract 23 passed、typecheck、fmt 与 diff check 通过。
- [x] (2026-08-02) U2：后端 inventory 来源归并完成。RED：新增 producer test 以缺少 `CollectionObservation.source` 的 E0560 失败；GREEN：collection actions 6/6、Skill Catalog 8/8、Rust lib 329 passed / 1 ignored、strict Clippy、fmt 与 diff check 通过。
- [x] (2026-08-02) U3：前端冲突来源清单完成。RED：聚焦组件测试 3 failed / 8 passed，证明旧 UI 只显示 description 且截断地址；GREEN：组件 11/11、双语 locale 2/2、typecheck、lint、format 与 diff check 通过。
- [x] (2026-08-02) U4：完成评审、全量验证、release 构建、本机安装与产物清理。前端 30 files / 169 tests 全部通过；Rust lib 331 passed / 1 ignored，除两个被当前执行环境在子进程启动前以 SIGKILL 终止的既有 crash-boundary 测试外，`--all-targets` 其余测试全部通过；strict Clippy、fmt、lint、typecheck、format 与 diff check 通过。`AD.app` 1.0.1 已安装到 `/Applications`，二进制 SHA-256 为 `20766a834f8a6ed05ab97fa55b9da82a82c86a31ce74cbd314f96b3b9dc96495`，启动日志到达 `ad ready`。随后清理 5.4 GiB Rust/Tauri 缓存、`dist/`、`src-tauri/gen/` 与未跟踪生成配置。

## 验证和验收

### 聚焦测试

```bash
cargo test --manifest-path src-tauri/Cargo.toml collection_actions
cargo test --manifest-path src-tauri/Cargo.toml skill_catalog
pnpm vitest run tests/lib/agentTypes.test.ts tests/components/AgentCollectionPanel.test.tsx
```

### 全量门禁

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
pnpm tauri build
git diff --check
```

### 人工验收

- 在 Project Workspace 打开含两个同名 Skill 的项目。
- 冲突标题的候选数与实际候选行一致。
- 每行可完整读到来源地址；Catalog 分支/子目录与 installed path 标签正确。
- URL 不出现 credentials、query token 或 fragment。
- 冲突资源继续为 read-only，管理来源与重新检查按钮正常。
- 切换中英文后标签完整，长英文/路径不挤出主窗口。

## 幂等性和恢复

inventory 读取与 UI 展示均为只读，可重复执行。新增 source 不改变 ResourceKey、DeclarationKey、ownership 或 action availability；formatter 失败时降级为 unavailable，而不是阻断整个 inventory 或泄漏原值。

若新增合同导致旧 fixture 或前端解析失败，先回退 optional `provenance.source` 的 producer/consumer 改动；现有 opaque source ID 与冲突 fail-closed 行为仍可继续工作。实施不迁移 `~/.ad` 数据、不重写 Skill Catalog、不删除任何 Skill。

## 意外发现

- 当前 `SkillCatalogEntry` 已保存 `source_id / display_name / source_type / location / branch / subdirectory`，丢失发生在 `catalog_skill_observations` 转换为 `CollectionObservation` 时。
- 已安装 Skill 的 snapshot path、Claude fallback canonical path 和 ownership target path 描述的是安装落点，不等于 acquisition source；UI 必须区分标签。
- Catalog source 与已安装 snapshot 会以相同 sourceId 进入同一 group，但 configured snapshot 当前会成为 winner，因此 source 不能只从 winner 读取。
- Git acquisition 路径已有较严格的 URL 限制，但 inventory 仍需防御手工篡改的 catalog bytes，不能直接序列化原始 location。
- 严格 TypeScript schema 要求 Git branch 非空；评审发现 Rust producer 原本可能透传空白或控制字符 branch，导致整个 inventory 在前端解析失败。现已在 Rust IPC 边界拒绝这些值，并增加回归测试。
- 当前执行环境中，`execution_recovery_crash` 的两个既有测试所启动的 helper 会在进入 `main` 前收到 SIGKILL；同一 helper 从 shell 直接执行正常，且其余 `cargo test --all-targets` 测试均通过。本轮没有修改该 crash harness，并把这一环境限制作为验证例外保留记录。

## 决策日志

- 决策：同时支持 Catalog source 与 installed path，但以不同 kind/标签展示。
  理由：只显示 Catalog 无法解决用户截图中 Agent 原生目录的同名冲突；把 installed path 称为源仓库又会误导。
  日期/作者：2026-08-02 / Codex
- 决策：来源 metadata 放在 provenance，而不是 declaration 或 description。
  理由：每个 conflict resource 已代表一个 source group，且未安装 Catalog candidate 没有 declaration；description 是内容说明，不是来源合同。
  日期/作者：2026-08-02 / Codex
- 决策：地址只能由 Rust 输出，不能进入 mutation request。
  理由：满足可观察性需求，同时保持已批准 Project Agent Workspace 的 backend-issued identity 与 sealed target 安全边界。
  日期/作者：2026-08-02 / Codex

## 结果回顾

Skill 冲突现在按候选逐项显示可核验来源：Catalog Git/本地来源保留 acquisition address、branch 与 subdirectory；无法回溯 Catalog 的 Agent 原生 Skill 明确显示当前安装位置；来源缺失或 metadata 自相矛盾时诚实降级并保持 fail closed。Git URL 的 userinfo、password、query 与 fragment 在 Rust inventory 边界被移除，地址从未进入 Preview/Apply mutation contract。

冲突卡已替换原来的描述 chip，长地址使用等宽、可换行布局，并保留“管理 Skill 来源”和“重新检查”。Plugin 仍使用真实的声明层级，不伪造 acquisition 来源。组件与 inventory 集成测试覆盖两个同名 Catalog 来源、安装位置 fallback、不可用来源、Settings 启动失败、完整地址渲染和写操作边界。

评审提出的唯一合同级问题（空 Git branch 可使严格前端 schema 拒绝 inventory）已经修复；credential-bearing catalog fixture 也在真实 inventory 边界验证了脱敏。本机 release 构建、安装与 smoke launch 成功，可再生构建/调试产物已按 `AGENTS.md` 自动清理，正式 `/Applications/AD.app` 与用户数据保留。

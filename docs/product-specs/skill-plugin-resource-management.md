# Product Contract: Harness 中的 Skill / Plugin 管理与安装

Authority: 用户于 2026-08-13 确认的资源管理决策、2026-08-15 确认的 Harness 产品定位、2026-09-03 确认的“安装关系默认跟随 Source 当前内容”原则，以及 2026-10-09 确认的 Codex Plugin 手动同步机制

Product Context: `docs/product-specs/project-agent-workspace.md`

## Actor and observable outcome

AD 用户可以从与“项目”并列的顶级 Harness 工作区进入 Skills / Plugins 子功能，集中管理来自 Git 仓库或本地目录的 Skill 和 Plugin，然后从该受管库选择资源、目标 Agent（Codex 或 Claude Code）及 project/user 作用域完成安装和卸载。Harness 是面向 Agent 的可复用能力管理工作区；本期只开放 Skills / Plugins 子功能。该子功能中的受管库是 AD 安装 Skill / Plugin 的唯一资源池，未进入受管库的内容不能通过 AD 安装。

安装是在一个作用域与 Agent 上建立对受管资源的使用关系。所有安装关系唯一且默认地跟随 Source 当前内容。支持直接来源读取的安装在本地目录修改后读取当前内容；Codex 全局 Plugin 及其项目继承使用物理缓存，用户在 Harness 刷新或更新 Source，或更新所选安装后同步缓存，无需重新安装。AD 不增加自动同步后台服务，不转换或改写 Skill / Plugin 原始内容，也不向用户提供“快照/软连接”等安装机制选择。

## Requirements

- **R1 — Harness 顶级工作区与内部能力导航。** AD 提供与“项目”并列的顶级 Harness 入口，将其定位为面向 Agent 的可复用能力工作区。Harness 内部提供能力导航，本期 Skills / Plugins 是唯一可用子功能；该子功能以 Skill / Plugin 资源为主体展示已导入内容，负责 source 导入、扫描、更新和删除。MCP 可以作为不可用的未来能力位置显示，但不得提供入口、配置或已支持暗示。Harness 不是资源市场，不承担发现或推荐。
  - Acceptance: 桌面产品顶级导航显示 Harness；进入后一级标题为 Harness，内部能力导航显示当前可用的 Skills / Plugins；MCP 显示为禁用且带“即将推出”状态，不能进入或执行任何动作；资源视图围绕资源而不是 source 配置表；界面中不出现市场搜索、推荐或远程发现承诺。
  - Owner / method: Product / Design 通过真实桌面界面走查验收导航、信息层级和能力边界。
  - Provenance: 用户于 2026-08-15 明确要求将顶级“资源”升级为 Harness、将 Skills / Plugins 管理作为子功能，并确认 Harness 给 Agent 使用、采用 Harness 内部能力导航且先预留 MCP 位置。

- **R2 — 多来源纳管与可辨识选择。** 本期 source 支持 Git 仓库和本地目录。不同 source 中同名的资源是独立候选项，不在 Harness 受管库中判定为冲突；用户在安装选择时必须能看到来源类型并辨别具体来源。
  - Acceptance: 可同时导入 Git 和本地 source；两个不同 source 的同名资源可同时出现且可分别选择；project/user 安装选择中显示 source 类型和足以区分候选项的来源信息。
  - Owner / method: Engineering 通过 source 导入与同名资源端到端场景验收；Product 确认界面可辨识性。
  - Provenance: 用户确认 AD 统一管理多种来源，本期支持 Git / 本地，安装时由用户根据来源选择。

- **R3 — 原始内容不可转换。** AD 只管理资源的身份、来源、更新、兼容性、安装状态和生命周期，不转换、改写、裁剪或补全 Skill / Plugin 内容。同一 source 内如果存在重复安装标识，应在导入或扫描时作为非标资源拦截，不把它交给用户解决。
  - Acceptance: 安装前后可证明资源原始文件字节不变；同 source 内重复安装标识的资源不会进入可安装状态，并给出简洁的不合规原因。
  - Owner / method: Engineering 使用内容摘要对比和非标 fixture 验收。
  - Provenance: 用户明确“Skill/Plugin 原始内容不能转换，AD 只做资源管理”，并要求拦截不符合规范的 Skill。

- **R4 — 作用域与 Agent 精确安装。** 用户选择资源后，必须选择 Codex 或 Claude Code，并明确安装到 project 或 user 作用域。project 安装、启用、禁用和卸载只能改变当前项目与所选 Agent；user 安装只改变所选基础 Agent，并供该 Agent 的所有项目使用。AD 对用户提供统一产品语义，不要求用户理解 Agent 的安装位置或注册差异。
  - Acceptance: 安装候选只来自 Harness 的 Skills / Plugins 受管库；在项目 A 为 Codex 执行 project 安装后，项目 A 的 Claude Code 和项目 B 的两个 Agent 状态与文件均不变；对 Codex 执行 user 安装后 Claude Code 不变，Codex 的项目可以使用该资源；卸载具有同样的作用域边界。
  - Owner / method: Engineering 通过两项目 × 两 Agent × project/user 作用域的隔离验收和安装/卸载端到端验证。
  - Provenance: 用户确认项目内的操作不影响其他项目，user 安装只属于所选 Agent，并要求 Skill / Plugin 安装区分 Codex 与 Claude Code。

- **R5 — Agent 兼容性如实呈现。** Skill / Plugin 可以只支持某个 Agent。如果资源依赖目标 Agent 不具备的专有能力，AD 应显示“不支持该 Agent”并阻止安装，不伪造兼容、不修改原始内容。
  - Acceptance: 一个仅支持 Claude Code 的 Plugin 在 Codex 候选中显示不支持且无法执行安装，在 Claude Code 下仍可正常安装。
  - Owner / method: Engineering 使用 Agent 兼容/不兼容 fixture 验收；Product 确认阻塞信息可理解。
  - Provenance: 用户选择“明确不支持并拦截”。

- **R6 — 安装关系默认跟随 Source 当前内容。** 当前产品不做版本管理，每条 Skill/Plugin 安装关系唯一且默认地跟随其 Source 当前内容。本地 Source 的当前内容是目录此刻的内容；Git Source 的当前内容是在 Harness 中最近一次完成同步后的 checkout。支持直接来源读取的安装在本地内容修改后自然更新。Codex 全局 Plugin 及其项目继承采用手动同步：用户在 Harness 刷新/更新 Source，或更新所选安装后，物理缓存同步到当前来源内容，下一次 Agent 会话读取该内容。应用启动与来源变化本身不触发同步；不安装独立后台服务。未同步的受管缓存继续可用，并显示实际安装版本、来源版本和可用更新；缓存异常提供修复。界面不暴露安装机制选择。
  - Acceptance: 修改原始目录后，支持直接来源读取的安装在新会话读取修改内容；Codex 全局 Plugin 在手动同步前保持原安装内容，Harness Source 刷新/更新后全局缓存及已准备的继承项目缓存同步，新会话读取修改内容；对 Git Source 执行一次同步后，至少两个既有安装不重新安装即可读取新 checkout；运行中的 Agent 会话不作为热更新验收对象；界面不提供“快照/软连接”安装方式开关。
  - Owner / method: Engineering 通过 Local/Git × Skill/Plugin × Codex/Claude Code × project/user 的来源更新矩阵、Agent 新会话和安装身份验收；Product 确认用户流程中只有统一的“安装”语义。
  - Provenance: 用户于 2026-09-03 确认“不做版本管理时安装应自然使用最新内容”，取消安装机制开关。用户于 2026-10-09 进一步确认 Codex Plugin 可通过 Harness 更新/刷新手动同步，不要求自动同步或独立后台服务。

- **R7 — 安装目标冲突必须先卸载。** 不同 source 的同名资源可以在中心并存，但安装到当前作用域/Agent 时，如果目标位置已被占用，AD 必须阻止新安装。对 AD 管理的旧安装，展示其来源并提供标准卸载；用户卸载后再单独发起新来源的安装，AD 不执行隐式替换或一键切换。
  - Acceptance: 同一作用域/Agent 的占位冲突不会改写旧安装；界面指明已安装资源的 source 类型；只有卸载操作完成后，用户才能另行安装新来源。
  - Owner / method: Engineering 通过同目标、异 source 的安装顺序和文件不变性验收。
  - Provenance: 用户明确“如果冲突，提示需要卸载，然后才能安装其他来源”。

- **R8 — 非 AD 托管资源只展示、不接管。** 如果用户通过其他途径在 project/user Agent 作用域中安装了 Skill / Plugin，AD 应在对应资源清单中将其标识为“非 AD 托管”。如果它占用了 AD 资源的安装目标，AD 只说明占用状态并允许用户重新检查。AD 不把它自动加入 Harness 受管库，不提供安装来源推断，也不删除、卸载、备份、恢复或接管该资源。
  - Acceptance: project/user 清单能展示 Agent 中已存在但无 AD ownership 的 Skill / Plugin，并明确标记为“非 AD 托管”；该资源不出现在 Harness 受管库中；其操作区不提供 AD 卸载、删除、替换、移除并备份、恢复或接管动作；检查前后外部项字节与路径不变。
  - Owner / method: Engineering 使用 external file/directory/symlink 占位 fixture 验收。
  - Provenance: 用户明确外部项处理交给用户，AD 不提供移除、备份或恢复能力。

- **R9 — 卸载是可复用的标准动作。** 每个受 AD 管理的 project/user Agent 安装都必须有对应卸载动作。该动作只清理选定作用域中可证明属于 AD 的绑定和附加配置，不修改 Source，也不清理其他安装关系。
  - Acceptance: 同一资源安装到两个项目及一个 user 作用域后，在项目 A 卸载只清理 A 的绑定和 Agent 状态；user 卸载只清理所选基础 Agent 的 user 安装；其他安装和中心 Source 均不变。
  - Owner / method: Engineering 通过跨项目、跨 Agent、跨作用域的卸载隔离验收和 ownership 证明。
  - Provenance: 用户确认“有安装就有卸载”，并重申项目操作不影响其他项目、user 操作不跨 Agent。

- **R10 — 显式移除单个受管资源前完成安装清理。** 用户可以从 Harness 受管库显式移除某一个 Skill 或 Plugin，而不影响同一 source 中的其他资源。该资源仍有 project/user 安装时，移除前必须告知受影响的项目数、user 安装数和 Agent 安装数；用户确认后，AD 对每个受管安装调用对应作用域的标准卸载动作，并展示整体及逐项进度。所有必要卸载完成后才将该资源移出受管库，不修改 Git 仓库或本地目录，也不留下悬空绑定。
  - Acceptance: 单个资源移除确认清楚呈现影响数量；取消时零变更；确认后各 project/user Agent 安装按标准卸载规则清理并展示进度；任一项失败时逐项呈现失败，不报告整体成功，且资源保持可管理以便重试未完成项；成功后同一 source 中其他资源仍在 Harness 受管库，source 原始内容不变。
  - Owner / method: Engineering 通过多项目、多 user Agent 安装的资源移除、取消和部分失败场景验收；Product 审核破坏性确认的信息充分性。
  - Provenance: 用户要求显式删除前告知所有受影响安装，并确认删除时按各作用域的卸载方式清理；2026-09-03 进一步确认这与 Source 内容自然缺失时“不自动清理”是两个不同动作。

- **R11 — 已移除资源必须先重新加入受管库。** 单个 Skill / Plugin 被显式移出 Harness 受管库后，source 刷新、内容更新、应用重启或作用域扫描都不得将它自动恢复为受管资源。用户若要再次安装，必须先在 Harness 的 Skills / Plugins 子功能中主动重新添加该资源；资源重新进入受管库后，才可以选择作用域与 Agent 发起安装。“重新添加资源”和“安装”是两个明确动作。
  - Acceptance: 移除后的资源不出现在 project/user 的 AD 安装候选中；刷新 source、更新内容、重启 AD 和重新扫描后仍不恢复；用户在 Harness 中主动重新添加后，它重新出现在受管库和安装候选中，但在用户主动发起安装前不会写入任何项目/Agent。
  - Owner / method: Engineering 通过移除、刷新、重启、扫描、重新添加和 project/user 安装的完整生命周期验收；Product 确认入口和两个动作的可理解性。
  - Provenance: 用户明确资源中心是唯一安装来源，“没有就需要先添加到资源里面”，不提供额外安装途径。

- **R12 — 界面是产品化的管理体验。** Harness 应先表达 Agent 能力工作区定位，其 Skills / Plugins 子功能呈现为有策展感的资源库；project/user 安装视图聚焦“已安装”、“可安装”和当前最主要的下一步操作。信息通过层级、状态和按需展开呈现，不把机制说明和所有诊断信息平铺在主界面。
  - Acceptance: 正常状态不向用户解释软链接、adapter、ownership 等实现机制；每个页面或主分区有一个清晰主操作；阻塞状态仅提供简洁原因与一个可执行的下一步；空状态、加载、错误、确认和操作反馈均有明确视觉设计；真实桌面界面在代表性窗口尺寸下完成人工视觉走查。
  - Owner / method: Product / Design 以真实桌面产品完成任务走查和视觉评审；Engineering 验收状态覆盖与操作可达性。
  - Provenance: 用户明确 AD 是一个需要设计美感的产品，不能把所有说明与功能平铺到界面。

- **R13 — Source 缺失只改变安装健康状态。** Source 当前内容删除或改名某个已安装资源时，Source 仍正常前进，AD 不阻止本地修改或 Git 同步，也不自动卸载、迁移或清理相关安装关系。旧安装进入“来源已缺失”状态；恢复同一 source/resource 身份后自然恢复。新增资源不会自动安装；改名视为旧资源缺失和一个新的未安装资源。
  - Acceptance: 删除一个已被多个 project/user 作用域安装的资源后，Source 更新成功且没有任何自动卸载写入；各受影响资源项显示“来源已缺失”并保留手动“卸载”动作；恢复原身份后提示消失且安装关系恢复；新增或改名后的新资源只作为可安装候选出现。Git 更新预览使用一句精简提示说明将有多少安装变为缺失并要求确认；本地 Source 在下次扫描后显示同样的持续状态；Harness Source 卡片只显示精简的受影响安装数量。
  - Owner / method: Engineering 通过 Local/Git 的删除、改名、恢复、新增和零自动清理场景验收；Product / Design 审核“来源已缺失”“N 个已安装资源缺失”“卸载”等精简文案和信息层级。
  - Provenance: 用户于 2026-09-03 确认 Source 删除或改名不得阻止前进，不自动清理，只在产品中以精简提示持续呈现；新增资源仍需用户主动选择安装。

- **R14 — 当前模型为未来按安装关系管理版本留出边界。** 当前所有安装关系只有“跟随最新”策略，不提供版本选择、pin、rollback 或历史恢复。AD 应为每条安装关系保留其 source/resource 身份，并记录当次解析到的 source revision、Git commit（适用时）和内容摘要，使未来可以在不改变安装身份的前提下增加“固定 revision / 恢复跟随最新”。版本策略未来属于每条安装关系，不全局锁定 Source。
  - Acceptance: 当前安装记录可以区分安装身份、跟随策略和最近解析 revision；同一资源在不同 project/user 安装关系中具有独立扩展版本策略的身份空间；产品界面不出现未实现的版本控制；系统不为此要求保留历史内容，版本能力上线前的旧 revision 不承诺可恢复。
  - Owner / method: Engineering 通过安装记录契约与兼容性测试证明身份和 revision 元数据可扩展；Product 确认当前界面无版本承诺，未来版本策略按安装关系而非全局 Source 表达。
  - Provenance: 用户于 2026-09-03 确认未来版本管理以每条安装关系为粒度、默认跟随最新；当前只保留版本身份元数据，不建设隐藏历史版本仓库。

## In scope

- 一个与项目并列的顶级 Harness 工作区、内部能力导航，以及其中的 Skills / Plugins 管理子功能。
- MCP 的禁用导航位置及“即将推出”状态，不包含任何 MCP 管理行为。
- Git 仓库与本地目录 source 的导入、扫描、更新、状态和删除。
- source 内单个 Skill / Plugin 的添加、移除、移除后的持续排除状态，以及用户主动重新添加。
- project/user 作用域中针对 Codex 或 Claude Code 的 Skill / Plugin 安装、必要的 Agent 状态管理和卸载。
- 安装关系跟随 Source 当前内容、Local/Git 更新传播、来源缺失状态、Agent 兼容性、非标资源拦截、安装目标冲突和 external/unowned 占位边界。
- 受管 project/user 安装卸载，以及显式移除中心资源时对全部受管安装的明示确认与清理。
- 每条安装关系的“跟随最新”策略及 source revision、Git commit、内容摘要等未来版本扩展元数据。
- Harness、Skills / Plugins 子功能和 project/user 安装界面的产品化信息架构、状态与视觉验收。

## Out of scope

- Skill / Plugin 版本选择、pin、更新通道、版本回滚，以及历史内容保留或恢复承诺。
- 快照/软连接等安装机制选择开关，以及要求用户理解 Agent cache、注册或链接位置的界面。
- Source 新增资源的自动安装，Source 缺失资源的自动卸载/迁移/清理，以及运行中 Agent 会话热更新。
- 资源市场、资源推荐、搜索公共生态或远程发现。
- OSS 等 Git / 本地之外的具体 source 类型；保留未来扩展可能，但本期不提供占位功能。
- MCP 管理及其他 Harness 子功能；本期仅允许展示不可交互的 MCP 未来能力位置。
- Skill / Plugin 内容转换、跨 Agent 改写、兼容层注入、裁剪或重新打包。
- 为不支持的 Agent 伪造降级版 Plugin / Skill。
- 删除、备份、恢复、接管或自动修复 external/unowned 项。
- 从 Harness 发起对多个具体项目的批量安装，以及绕过 Harness 受管库的 AD 安装。
- Agent 的具体文件位置、注册格式、配置载体与适配算法；这些由后续技术设计确定。

## Constraints and confirmed decisions

- AD 的 project 操作不得影响其他项目；user 操作只影响所选基础 Agent；Source 更新会按定义改变所有跟随该 Source 的安装所解析到的内容，但不执行自动卸载或其他隐式清理。
- Harness 是面向 Agent 的全局能力管理面，但不提供跨项目安装；本期仅包含 Skills / Plugins 子功能，MCP 仅为禁用的未来能力标识。“移除中心资源”是用户在明确影响范围后发起的全局生命周期操作，不属于项目操作。
- Harness 中的 Skills / Plugins 受管库是 AD 安装 Skill / Plugin 的唯一资源池；project/user 安装不能自行发现或临时引入资源。
- 安装关系默认且唯一地跟随 Source 当前内容；支持直接来源读取的安装在本地目录变化后读取当前内容，Codex 全局 Plugin 及其项目继承在 Harness 手动同步完成后，下一次 Agent 会话读取当前内容。本期不支持版本管理、历史恢复或自动回退。
- Source 中资源缺失不会阻止 Source 前进，也不会触发自动清理；只产生精简、持续且可行动的“来源已缺失”状态。Source 新增资源必须由用户主动安装。
- 当前每条安装关系记录“跟随最新”及已解析 revision 身份，但不保证保留 revision 对应的历史内容；未来版本管理按安装关系扩展，不全局锁定 Source。
- Skill / Plugin 原始内容不可被 AD 改写。
- 不同 source 的同名资源在中心不构成冲突；安装时的实际目标占位仍必须拦截。
- Codex 与 Claude Code 必须具有统一的用户操作模型，但能力不对等时必须如实表达不支持。
- 只对可证明属于 AD 的安装执行卸载与清理。
- 非 AD 安装的 Skill / Plugin 只在对应 project/user 视图中标识为“非 AD 托管”，不会自动进入 Harness 受管库或变为 AD-owned。
- 正常产品界面不暴露软链接、快照、cache、adapter、ownership 等实现术语，也不提供安装机制选择。

## Delegated engineering defaults and boundaries

- 后续设计可为 Codex 和 Claude Code 分别实现 Agent adapter，自主决定软连接、current pointer、启动参数或会话启动投影等安装机制，以及安装位置、注册载体、启用状态与安全执行步骤，前提是原始内容不变、既有安装无需手工更新即可在下一次会话读取 Source 当前内容，且作用域、安装和卸载结果与本契约一致。无法可靠满足该结果的 Agent/资源组合必须如实标记为不支持。
- 工程可在不改变产品语义的前提下自主设计 source/resource/install 身份、每条安装关系的 latest tracking 与 resolved revision 元数据、路径安全、所有权证明、并发控制、失败恢复和重试；不得借未来版本扩展之名提前保留无界历史内容。
- 产品/设计可自主选择卡片、列表、抽屉、状态标记和文案，前提是保留本契约中的主导航、操作归属、必要来源辨识、阻塞与确认语义，并通过真实桌面视觉走查。
- 产品/设计应以“来源已缺失”“N 个已安装资源缺失”“卸载”等短句呈现 R13，不在正常界面解释缺失传播、软连接或版本模型。
- 移除中心资源时，工程可决定受管卸载的执行顺序和重试机制，但必须提供整体及逐项进度；部分失败必须如实呈现，不得移除仍会造成悬空安装的中心资源。
- 工程可自主选择如何持久表达“source 中该资源已被用户移除”，但该状态必须跨刷新、内容更新、重启和项目扫描保持稳定，并只允许由 Harness 中的主动重新添加动作解除。

## Repository baseline and superseded semantics

- 当前产品已具备 project/user + Agent 作用域、capability/adapter 边界、Source catalog、安装记录与部分来源关联基础。
- `docs/product-specs/project-agent-workspace.md` 中“项目操作不跨项目”的产品原则继续有效。
- 既有代码或文档中与本契约不一致的安装机制开关、Source 更新因已安装资源缺失而被阻止、Plugin cache 状态不可辨识等语义不再作为产品权威；后续设计必须按 R6、R13 和 R14 收敛。显式移除 Harness 受管资源仍按 R10 执行确认后的安装清理，不与 Source 内容自然缺失混为一类操作。
- 代码和既有设计只用于证明当前能力与约束，不扩展本契约的产品范围。

## Deferred adjacent ideas

- Harness 的 MCP 管理子功能；当前只有禁用导航位置，正式纳入时需单独确认 MCP 的来源、作用域、安装/启用和兼容性语义。
- 安装关系的版本选择、pin、回滚策略和历史内容保留；当前只保留未来扩展所需的身份与 revision 元数据。
- OSS 等新 source provider。
- 资源市场、发现、推荐与社区分发。
- 新 Agent 类型的具体适配；产品语义沿用本契约，但需在实际纳入时为其能力和安装契约单独设计与验证。

## Open product decisions

None.

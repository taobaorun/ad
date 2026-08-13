# Product Contract: Skill / Plugin 资源中心与项目安装

Authority: 用户于 2026-08-13 在本次对话中确认的产品决策

Product Context: `docs/product-specs/project-agent-workspace.md`

## Actor and observable outcome

AD 用户可以在一个独立的资源中心集中管理来自 Git 仓库或本地目录的 Skill 和 Plugin，然后在具体项目中从资源中心选择资源及目标 Agent（Codex 或 Claude Code）完成安装和卸载。资源中心是 AD 安装 Skill / Plugin 的唯一资源池；未进入资源中心的内容不能通过 AD 安装。

安装后的资源内容始终关联资源中心中的原始或受管 source checkout；中心内容更新后，已安装项目无需重新安装即可使用新内容。AD 屏蔽 Agent 的安装机制差异，但不转换、改写、裁剪或重新打包 Skill / Plugin 原始内容。任何从项目内发起的操作只影响当前项目和用户选择的 Agent。

## Requirements

- **R1 — 独立的已纳管资源库。** AD 提供与“项目”并列的顶级资源中心，以 Skill / Plugin 资源为主体展示已导入内容，负责 source 导入、扫描、更新和删除；它不是资源市场，不承担发现或推荐。
  - Acceptance: 桌面产品中可从顶级导航进入资源中心；默认视图围绕资源而不是 source 配置表；界面中不出现市场搜索、推荐或远程发现承诺。
  - Owner / method: Product / Design 通过真实桌面界面走查验收导航、信息层级和能力边界。
  - Provenance: 用户选择“已纳管资源库”，并确认资源中心为独立顶级入口。

- **R2 — 多来源纳管与可辨识选择。** 本期 source 支持 Git 仓库和本地目录。不同 source 中同名的资源是独立候选项，不在资源中心判定为冲突；用户在安装选择时必须能看到来源类型并辨别具体来源。
  - Acceptance: 可同时导入 Git 和本地 source；两个不同 source 的同名资源可同时出现且可分别选择；项目安装选择中显示 source 类型和足以区分候选项的来源信息。
  - Owner / method: Engineering 通过 source 导入与同名资源端到端场景验收；Product 确认界面可辨识性。
  - Provenance: 用户确认 AD 统一管理多种来源，本期支持 Git / 本地，安装时由用户根据来源选择。

- **R3 — 原始内容不可转换。** AD 只管理资源的身份、来源、更新、兼容性、安装状态和生命周期，不转换、改写、裁剪或补全 Skill / Plugin 内容。同一 source 内如果存在重复安装标识，应在导入或扫描时作为非标资源拦截，不把它交给用户解决。
  - Acceptance: 安装前后可证明资源原始文件字节不变；同 source 内重复安装标识的资源不会进入可安装状态，并给出简洁的不合规原因。
  - Owner / method: Engineering 使用内容摘要对比和非标 fixture 验收。
  - Provenance: 用户明确“Skill/Plugin 原始内容不能转换，AD 只做资源管理”，并要求拦截不符合规范的 Skill。

- **R4 — 项目与 Agent 精确安装。** 安装入口位于具体项目。用户选择资源后，必须选择 Codex 或 Claude Code；安装、启用、禁用和卸载只能改变当前项目与所选 Agent。AD 对用户提供统一产品语义，不要求用户理解 Agent 的安装位置或注册差异。
  - Acceptance: 项目安装候选只来自资源中心；在项目 A 为 Codex 安装资源后，项目 A 的 Claude Code 和项目 B 的两个 Agent 状态与文件均不变；卸载具有同样的隔离性。
  - Owner / method: Engineering 通过两项目 × 两 Agent 的隔离验收和安装/卸载端到端验证。
  - Provenance: 用户确认项目内的操作不影响其他项目，并要求 Skill / Plugin 安装区分 Codex 与 Claude Code。

- **R5 — Agent 兼容性如实呈现。** Skill / Plugin 可以只支持某个 Agent。如果资源依赖目标 Agent 不具备的专有能力，AD 应显示“不支持该 Agent”并阻止安装，不伪造兼容、不修改原始内容。
  - Acceptance: 一个仅支持 Claude Code 的 Plugin 在 Codex 候选中显示不支持且无法执行安装，在 Claude Code 下仍可正常安装。
  - Owner / method: Engineering 使用 Agent 兼容/不兼容 fixture 验收；Product 确认阻塞信息可理解。
  - Provenance: 用户选择“明确不支持并拦截”。

- **R6 — 中心更新自动传播。** Skill 与 Plugin 的原始文件必须通过软链接指向资源中心中的原始或受管 source checkout。如果 Agent 还要求启用、注册或其他必要配置，AD 可以只在当前项目的所选 Agent 上管理这些附加状态，卸载时一并清理。
  - Acceptance: 对中心 source 执行一次已确认的内容更新后，至少两个已安装项目在不重写项目内资源链接、不重新安装的情况下读取到新内容；卸载后当前项目/Agent 的链接与 AD 添加的附加配置均被清理。
  - Owner / method: Engineering 通过共享 source 更新、链接身份和 Agent 注册状态验收。
  - Provenance: 用户确认资源安装使用软链接、中心更新自动传播，并确认 Agent 差异在产品层统一、具体适配由设计方案解决。

- **R7 — 安装目标冲突必须先卸载。** 不同 source 的同名资源可以在中心并存，但安装到当前项目/Agent 时，如果目标位置已被占用，AD 必须阻止新安装。对 AD 管理的旧安装，展示其来源并提供标准卸载；用户卸载后再单独发起新来源的安装，AD 不执行隐式替换或一键切换。
  - Acceptance: 同一项目/Agent 的占位冲突不会改写旧安装；界面指明已安装资源的 source 类型；只有卸载操作完成后，用户才能另行安装新来源。
  - Owner / method: Engineering 通过同目标、异 source 的安装顺序和文件不变性验收。
  - Provenance: 用户明确“如果冲突，提示需要卸载，然后才能安装其他来源”。

- **R8 — 非 AD 托管资源只展示、不接管。** 如果用户通过其他途径在项目/Agent 中安装了 Skill / Plugin，AD 应在项目资源清单中将其标识为“非 AD 托管”。如果它占用了 AD 资源的安装目标，AD 只说明占用状态并允许用户重新检查。AD 不把它自动加入资源中心，不提供安装来源推断，也不删除、卸载、备份、恢复或接管该资源。
  - Acceptance: 项目清单能展示 Agent 中已存在但无 AD ownership 的 Skill / Plugin，并明确标记为“非 AD 托管”；该资源不出现在资源中心的受管资源中；其操作区不提供 AD 卸载、删除、替换、移除并备份、恢复或接管动作；检查前后外部项字节与路径不变。
  - Owner / method: Engineering 使用 external file/directory/symlink 占位 fixture 验收。
  - Provenance: 用户明确外部项处理交给用户，AD 不提供移除、备份或恢复能力。

- **R9 — 项目卸载是可复用的标准动作。** 每个受 AD 管理的项目/Agent 安装都必须有对应卸载动作。该动作只清理当前项目/Agent 中可证明属于 AD 的链接和附加配置，不修改 source，不影响其他项目。
  - Acceptance: 同一资源安装到两个项目后，在项目 A 卸载只清理 A 的链接和 Agent 状态，项目 B 与中心 source 不变。
  - Owner / method: Engineering 通过跨项目卸载隔离验收和 ownership 证明。
  - Provenance: 用户确认“有安装就有卸载”，并重申项目操作不影响其他项目。

- **R10 — 移除单个中心资源前完成受管安装清理。** 用户可以从资源中心移除某一个 Skill 或 Plugin，而不影响同一 source 中的其他资源。该资源仍被项目安装时，移除前必须告知受影响的项目数和 Agent 安装数；用户确认后，AD 对每个受管安装调用与项目页一致的标准卸载动作，并展示整体及逐项进度。所有必要卸载完成后才将该资源移出资源中心，不修改 Git 仓库或本地目录，也不留下悬空链接。
  - Acceptance: 单个资源移除确认清楚呈现影响数量；取消时零变更；确认后各项目/Agent 按标准卸载规则清理并展示进度；任一项失败时逐项呈现失败，不报告整体成功，且资源保持可管理以便重试未完成项；成功后同一 source 中其他资源仍在资源中心，source 原始内容不变。
  - Owner / method: Engineering 通过多项目资源移除、取消和部分失败场景验收；Product 审核破坏性确认的信息充分性。
  - Provenance: 用户要求删除前告知所有已安装项目将失效，并确认删除时按各项目的卸载方式清理。

- **R11 — 已移除资源必须先重新加入资源中心。** 单个 Skill / Plugin 被移出资源中心后，source 刷新、内容更新、应用重启或项目扫描都不得将它自动恢复为受管资源。用户若要再次安装，必须先在资源中心主动重新添加该资源；资源重新进入中心后，才可以从具体项目选择 Agent 发起安装。“重新添加资源”和“安装到项目”是两个明确动作。
  - Acceptance: 移除后的资源不出现在项目的 AD 安装候选中；刷新 source、更新内容、重启 AD 和重新扫描项目后仍不恢复；用户在资源中心主动重新添加后，它重新出现在资源中心和项目安装候选中，但在用户于具体项目发起安装前不会写入任何项目/Agent。
  - Owner / method: Engineering 通过移除、刷新、重启、扫描、重新添加和项目安装的完整生命周期验收；Product 确认入口和两个动作的可理解性。
  - Provenance: 用户明确资源中心是唯一安装来源，“没有就需要先添加到资源里面”，不提供额外安装途径。

- **R12 — 界面是产品化的管理体验。** 资源中心应呈现为有策展感的资源库，项目页聚焦“已安装”、“可安装”和当前最主要的下一步操作。信息通过层级、状态和按需展开呈现，不把机制说明和所有诊断信息平铺在主界面。
  - Acceptance: 正常状态不向用户解释软链接、adapter、ownership 等实现机制；每个页面或主分区有一个清晰主操作；阻塞状态仅提供简洁原因与一个可执行的下一步；空状态、加载、错误、确认和操作反馈均有明确视觉设计；真实桌面界面在代表性窗口尺寸下完成人工视觉走查。
  - Owner / method: Product / Design 以真实桌面产品完成任务走查和视觉评审；Engineering 验收状态覆盖与操作可达性。
  - Provenance: 用户明确 AD 是一个需要设计美感的产品，不能把所有说明与功能平铺到界面。

## In scope

- 一个独立顶级资源中心，管理 Skill 和 Plugin。
- Git 仓库与本地目录 source 的导入、扫描、更新、状态和删除。
- source 内单个 Skill / Plugin 的添加、移除、移除后的持续排除状态，以及用户主动重新添加。
- 项目中针对 Codex 或 Claude Code 的 Skill / Plugin 安装、必要的 Agent 状态管理和卸载。
- 软链接关联、中心内容更新传播、Agent 兼容性、非标资源拦截、安装目标冲突和 external/unowned 占位边界。
- 受管项目卸载，以及移除中心资源时对全部受管安装的明示确认与清理。
- 资源中心和项目安装界面的产品化信息架构、状态与视觉验收。

## Out of scope

- Skill / Plugin 版本管理、项目版本 pin、更新通道或版本回滚。
- 资源市场、资源推荐、搜索公共生态或远程发现。
- OSS 等 Git / 本地之外的具体 source 类型；保留未来扩展可能，但本期不提供占位功能。
- Skill / Plugin 内容转换、跨 Agent 改写、兼容层注入、裁剪或重新打包。
- 为不支持的 Agent 伪造降级版 Plugin / Skill。
- 删除、备份、恢复、接管或自动修复 external/unowned 项。
- 从资源中心发起跨项目安装、绕过资源中心的 AD 安装，以及 user/global Agent 安装。
- Agent 的具体文件位置、注册格式、配置载体与适配算法；这些由后续技术设计确定。

## Constraints and confirmed decisions

- AD 的长期产品定性是以项目为作用域：项目中的操作不得影响其他项目。
- 资源中心是全局管理面，但不提供跨项目安装。“移除中心资源”是用户在明确影响范围后发起的全局生命周期操作，不属于项目操作。
- 资源中心是 AD 安装 Skill / Plugin 的唯一资源池；项目安装不能自行发现或临时引入资源。
- 中心内容更新依靠 source 关联自动传播；本期是全局最新内容语义，不支持版本管理。
- Skill / Plugin 原始内容不可被 AD 改写。
- 不同 source 的同名资源在中心不构成冲突；安装时的实际目标占位仍必须拦截。
- Codex 与 Claude Code 必须具有统一的用户操作模型，但能力不对等时必须如实表达不支持。
- 只对可证明属于 AD 的安装执行卸载与清理。
- 非 AD 安装的 Skill / Plugin 只在项目中标识为“非 AD 托管”，不会自动进入资源中心或变为 AD-owned。
- 正常产品界面不暴露软链接、adapter、ownership 等实现术语。

## Delegated engineering defaults and boundaries

- 后续设计可为 Codex 和 Claude Code 分别实现 Agent adapter，自主决定安装位置、注册载体、启用状态与安全执行步骤，前提是原始内容不变、资源文件保持中心链接、只修改当前项目/Agent，且安装/卸载结果与本契约一致。
- 工程可在不改变产品语义的前提下自主设计 source/resource/install 身份、路径安全、所有权证明、并发控制、失败恢复和重试。
- 产品/设计可自主选择卡片、列表、抽屉、状态标记和文案，前提是保留本契约中的主导航、操作归属、必要来源辨识、阻塞与确认语义，并通过真实桌面视觉走查。
- 移除中心资源时，工程可决定受管卸载的执行顺序和重试机制，但必须提供整体及逐项进度；部分失败必须如实呈现，不得移除仍会造成悬空安装的中心资源。
- 工程可自主选择如何持久表达“source 中该资源已被用户移除”，但该状态必须跨刷新、内容更新、重启和项目扫描保持稳定，并只允许由资源中心的主动重新添加动作解除。

## Repository baseline and superseded semantics

- 当前产品已具备项目 + Agent 作用域、capability/adapter 边界、Skill source catalog 与 Agent 侧 Skill symlink 的部分基础。
- `docs/product-specs/project-agent-workspace.md` 中“项目操作不跨项目”的产品原则继续有效。
- 既有文档中与本契约不一致的 Plugin 复制/转换/独立 cache 语义，以及“仍被引用时直接禁止移除资源”语义，不再作为本请求的产品权威；后续设计必须按 R6、R10 和 R11 重新收敛。
- 代码和既有设计只用于证明当前能力与约束，不扩展本契约的产品范围。

## Deferred adjacent ideas

- 中心 source 与资源的版本管理、项目 pin 和回滚策略。
- OSS 等新 source provider。
- 资源市场、发现、推荐与社区分发。
- 新 Agent 类型的具体适配；产品语义沿用本契约，但需在实际纳入时为其能力和安装契约单独设计与验证。

## Open product decisions

None.

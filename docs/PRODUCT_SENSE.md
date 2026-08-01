# 产品理念

AD 是本地 Coding Agent 的项目配置管理器。它不替代 Claude Code 或 Codex，也不做 chat UI；它负责让用户知道一个项目最终会使用什么配置，并能安全地改变它。

## 目标用户

核心用户是同时维护多个项目、多个 Agent 或多个配置实例的开发者。他们知道项目需要什么，却不想再靠手工追踪 user/project/runtime 层级、Skill source revision、Plugin 安装位置和转换残留。

AD 不面向只需偶尔修改一个 settings 文件的用户，也不把复杂文件语义隐藏成无法核验的“智能完成”。

## 核心价值

| 用户痛点 | AD 的回答 |
|---|---|
| 不知道某功能是否真的完成 | 每项资源显示 provenance、effective state、ownership、management status、coverage 与真实 action |
| 项目配置散落在多个层级和目录 | Project Agent Workspace 统一展示 Settings、Skills、Plugins 和 History |
| Skill source 更新可能影响多个项目 | source acquisition 与项目 pin 分离；项目只引用不可变 artifact revision |
| Plugin 缺少原生项目隔离 | Claude 使用项目覆盖；Codex 使用 AD Managed Project Codex Runtime，并明确这不是 Codex 原生 Project Plugin |
| Claude → Codex 转换看似成功但漏配置 | 先盘点有效项目环境，再按真实 carrier 路由；unsupported、partial、conflict 和 residual 不会消失 |
| 自动写盘让人不放心 | Preview → explicit confirmation → Apply → receipt；rollback 也是新的受保护计划 |

## 产品目标

AD 当前追求的是 **managed-Agent configuration parity**：Claude Code 与 Codex 在配置管理用户任务上具有一致的可理解性、安全保证和项目隔离。

它不追求 **automation-access parity**。本轮没有公共 CLI、MCP、deep-link 或远程 API，也不允许被管理的 Agent 自动批准危险操作。所有风险确认仍由第一方桌面 UI 中的人完成。

## 不做的事

- 不做云同步或团队协作；配置可能包含敏感信息，AD 是单机工具。
- 不嵌入 PTY，不调用 Agent 完成聊天或代码任务。
- 不允许用户动态加载 adapter 或自定义任意转换规则。
- 不把项目级配置隔离宣传成第三方扩展的运行时沙箱或发布者认证。
- 不读取或管理 auth/token/session/history/log/database 内容；受控 auth symlink 只共享路径关系。
- 不伪造平台不存在的能力：Codex User Plugin acquisition、Claude Plugin install 等保持 degraded/external。

## 设计取舍的优先级

1. 数据安全与不丢修改；
2. 能力陈述与实际证据一致；
3. 项目、Agent 与 installation 的隔离；
4. 可发现、可解释、可恢复；
5. macOS 原生体验与低配置上手。

## 证据等级

功能状态必须区分：规格已声明、第一方 UI 可达、自动化通过、真实 macOS `.app` 工作流通过、release bundle 通过。未达到的等级不向上推断。

项目工作区的逐项证据矩阵见 `docs/product-specs/project-agent-workspace.md`。未知 Agent 版本、schema 或位置会主动降低 inventory coverage；“全部”永远需要可验证的 discovery contract。

## 命名

- 产品名 `AD`；
- package / crate 名 `ad`；
- bundle identifier `com.jiaxy.ad`。

# Multi-Agent v1 实施计划

## Overview

按已批准的 `docs/design-docs/multi-agent-architecture.md`，将现有 v0 Agent foundation 迁移为以 AgentContext、capability ports、ResourceSnapshot 和 MutationPlan/Receipt 为核心的 v1 架构。迁移保持旧 Claude profile、IPC 和 UI 可用，先构建只读契约，再逐步接入共享安全执行、Codex 对等能力和 artifact conversion。

## Architecture Decisions

- AgentDefinition、AgentInstallation、AgentContext 分离；installation identity 由 adapter 按有效 config home 计算。
- capability 由可调用 port 推导，descriptor 同时声明 scope、operation、availability 和 limitations。
- common layer 不解析 Agent-specific 配置字段，只处理 resource、snapshot、digest、plan、receipt 和 error。
- adapter 负责 inspect/plan；ExecutionEngine 是唯一用户文件写入入口。
- profile 使用 `(agentId, profileId)` envelope + adapter-owned payload。
- conversion 按 artifact route 工作，source 永远不进入 write-set。

## Dependency Graph

```text
v1 typed contracts
  ├─ canonical discovery + AgentContext IPC/store
  ├─ Claude capability ports
  │    └─ shared ExecutionEngine
  └─ Codex capability ports
       └─ operation-level parity

Claude ports + Codex ports + ExecutionEngine
  └─ profile envelope + artifact conversion
       └─ unified UI + release gate
```

## Task List

### Phase 1: v1 Contract

- [x] Task 1: Typed identities and core context models
- [x] Task 2: Resource, plan, receipt and structured error contracts
- [x] Task 3: Capability ports and derived descriptors
- [ ] Task 4: TypeScript IPC schemas for v1 read models

### Checkpoint A

- [ ] Targeted Rust tests and frontend schema tests pass
- [ ] Existing v0 commands and profiles remain compatible
- [ ] No v1 code writes user files

### Phase 2: Canonical Context

- [ ] Task 5: Adapter-owned canonical installation discovery
- [ ] Task 6: AgentContext IPC and backward-compatible frontend selection

### Checkpoint B

- [ ] Duplicate discovery sources expose one canonical installation
- [ ] Different effective config homes remain separate

### Phase 3: Safe Execution

- [ ] Task 7: Claude capability ports over existing behavior
- [ ] Task 8: Backend-owned plan store and digest validation
- [ ] Task 9: Backup manifest, atomic mutations, receipts and compensation

### Checkpoint C

- [ ] Claude existing behavior has no regression
- [ ] Fault-injection tests prove conflict and partial-failure semantics

### Phase 4: Codex Parity

- [ ] Task 10: Codex resource allowlist and settings port
- [ ] Task 11: Codex skills and plugins ports
- [ ] Task 12: Shared process service and launch recipes
- [ ] Task 13: Claude/Codex operation-level parity contract tests

### Checkpoint D

- [ ] Five required user capabilities are callable for both agents
- [ ] Sensitive/runtime Codex files never enter managed snapshots

### Phase 5: Profiles and Conversion

- [ ] Task 14: AgentProfile envelope and Claude compatibility reader
- [ ] Task 15: Artifact-level Claude-to-Codex conversion route
- [ ] Task 16: Confirmed apply and rollback through ExecutionEngine

### Checkpoint E

- [ ] Source bytes never change
- [ ] Target conflicts and unsupported artifacts are visible before apply
- [ ] Rollback respects post-apply external modifications

### Phase 6: UI and Release

- [ ] Task 17: AgentContext/capability/plan-aware stores and UI
- [ ] Task 18: Conversion preview, confirmation and rollback UI with zh/en i18n
- [ ] Task 19: Full verification, as-built docs and ExecPlan archive

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| v0/v1 双轨期间模型混用 | High | v1 使用独立命名和 additive IPC；每个切片保留兼容测试 |
| capability port 变成空壳 | High | descriptor 从 port 推导，operation-level contract tests 验证行为 |
| 多文件写入中途失败 | High | 写前全量备份、单文件原子写、逆序补偿和 partial receipt |
| Codex 配置表面随版本变化 | High | allowlist 与解析规则只在 Codex adapter；保留未知 TOML 字段 |
| UI 出现散落 agentId 分支 | Medium | 集中 editor registry，store 使用 AgentContext 和 descriptor |

## Open Questions

当前无阻塞性开放问题。任何新增依赖、Tauri 权限变化或用户数据迁移语义变化仍需先征得用户确认。

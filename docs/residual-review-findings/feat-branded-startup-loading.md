# Residual Review Findings

- 来源：`ce-code-review` run `20260720-110338-7e1e261e`
- 审查范围：PR [#3](https://github.com/taobaorun/ad/pull/3) 的本地对齐工作树
- 记录日期：2026-07-20

## Residual Review Findings

- P1 `src/lib/startupSurface.ts:20` — Spotlight timing contradicts the documented contract — [GitHub Issue #4](https://github.com/taobaorun/ad/issues/4)
- P1 `src/main.tsx:59` — Startup adds an artificial 600ms minimum — [GitHub Issue #5](https://github.com/taobaorun/ad/issues/5)
- P1 `src/main.tsx:65` — Unhandled bootstrap rejection can strand startup splash — [GitHub Issue #6](https://github.com/taobaorun/ad/issues/6)
- P2 `index.html:145` — Static splash hardcodes localized UI copy — [GitHub Issue #7](https://github.com/taobaorun/ad/issues/7)
- P2 `src/lib/startup.ts:88` — Profile load still waits past startup deadline — [GitHub Issue #8](https://github.com/taobaorun/ad/issues/8)

这些问题属于现有 branded startup loading 变更，会改变启动时序、错误退化或构建期 i18n 结构，因此未作为本次下拉框视觉修复的机械 review fix 自动应用。

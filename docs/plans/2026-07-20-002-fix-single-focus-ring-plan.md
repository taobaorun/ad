---
title: 转换下拉框焦点环 - Plan
type: fix
date: 2026-07-20
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
---

# 转换下拉框焦点环 - Plan

## Goal Capsule

- **目标：** 将转换对话框中“白色外框 + 主题色内框”的焦点指示改为单个对比度合格的主题焦点环，同时保持 Mocha 与 Latte 下的键盘可见性。
- **依据：** 用户截图定义视觉缺陷；现有全局焦点合同定义该对话框以外的无障碍基线。
- **停止条件：** 如果局部焦点环无法在生产 CSS 级联中覆盖全局两层样式、无法在相邻表面达到 3:1 对比度，或键盘焦点不可见，则停止实施。
- **执行方式：** 一处局部 React 样式修改，使用现有行为测试和 macOS 视觉验证。
- **收尾责任：** LFG 负责实施、审查、Tauri 验证、提交、PR 与 CI；无法自动检查窗口时，以用户截图确认为兜底。

## Product Contract

### Summary

转换对话框的下拉框将只显示一个语义化 Blue 焦点环，不再同时显示主题色与白色线条，其他控件的焦点样式保持不变。

### Problem Frame

全局 `:focus-visible` 规则有意叠加 Sapphire `box-shadow` 和带偏移的 Text `outline`。转换下拉框继承两层样式，因此 Text 外框在 Mocha 中表现为明显的白色边线。若只删除外框并保留 Sapphire，会造成 Latte 回归，因为 Latte Sapphire 与 Base 或 Surface 0 的对比度不足 3:1。

### Requirements

- R1. 通过键盘聚焦转换对话框的下拉框时，只显示一个可见焦点环，不显示第二层偏移白色外框。
- R2. 局部焦点环必须使用现有语义主题角色，并在 Mocha 与 Latte 中对 Base 和 Surface 0 达到至少 3:1 对比度。
- R3. 修改必须保持 `:focus-visible` 行为、原生下拉交互、禁用状态、自定义下拉提示及所有非焦点样式不变。
- R4. 按钮、链接、输入框、文本域、标题栏下拉框及其他原生控件继续使用现有全局双色焦点合同。

### Scope Boundaries

- 修改仅限 `src/components/AgentConversionDialog.tsx` 中对话框局部的 `ConversionSelect` 样式。
- 全局主题 token、共享原生控件焦点规则、组件状态、转换行为、i18n、Rust 代码和设计文档均不修改。

## Planning Contract

### Key Technical Decisions

- KTD1. 仅在 `ConversionSelect` 上覆盖全局两层焦点样式，单焦点环使用现有语义 Blue `link` 角色。Blue 保持产品冷色强调体系，并在两种主题中达到对比度门槛。
- KTD2. 局部外框和焦点环声明使用带 important 的 `focus-visible` Tailwind utility。全局焦点选择器在 Tailwind utilities 之后输出且特异性更高，普通 `focus-visible:outline-none` 无法在生产级联中生效。

### Assumptions

- 对话框局部例外是正确的产品边界，因为截图与请求指向转换下拉框，而其他原生控件的全局双色焦点样式是有意引入的。
- 现有 `:focus-visible` 选择器仍是区分键盘与指针焦点的正确边界。

## Implementation Units

### U1. 为转换下拉框提供单一局部焦点环

- **目标：** 覆盖所有转换对话框下拉框的全局双色焦点层，不改变行为或其他控件。
- **需求：** R1、R2、R3、R4；实现 KTD1 和 KTD2。
- **依赖：** 无。
- **文件：** 修改 `src/components/AgentConversionDialog.tsx`；检查 `tests/components/AgentConversionDialog.test.tsx` 中保持不变的行为覆盖。
- **方法：** 为对话框共享的 select class 增加带 important 的 `focus-visible` utilities，隐藏可见外框，并用 2px 语义 Blue 焦点环替换继承的阴影。保持现有 `appearance-none`、自定义 chevron、尺寸、禁用、值和事件 props 不变，使作用域、权限与高级安装实例下拉框获得一致修复。
- **遵循模式：** 使用语义 Tailwind 颜色；将例外留在拥有该缺陷的组件中；不修改全局无障碍合同。
- **测试场景：** 无新增测试预期——这是 jsdom 无法证明的纯样式级联修改；现有组件测试继续覆盖选择、禁用状态和决策传递。Mocha 与 Latte 下键盘焦点只显示一个 Blue 环；指针焦点不产生持续的键盘专用指示；禁用和下拉行为保持不变。
- **验证：** 现有转换测试、类型检查、lint 和生产构建通过。运行 `pnpm tauri dev`，在两种主题中检查全部转换下拉框；若无法自动检查窗口，则安装生产 bundle，并明确交由用户通过截图确认视觉结果。

## Verification Contract

| 门禁                                                           | 适用范围        | 完成信号                                                                                       |
| -------------------------------------------------------------- | --------------- | ---------------------------------------------------------------------------------------------- |
| `pnpm test -- tests/components/AgentConversionDialog.test.tsx` | R1-R4 行为回归  | 现有作用域、权限、忙碌状态和实例选择测试通过。                                                 |
| `pnpm typecheck`                                               | TypeScript 合同 | 无类型错误。                                                                                   |
| `pnpm lint`                                                    | 代码质量        | 无 lint 错误或警告。                                                                           |
| `pnpm build`                                                   | 生产 CSS 级联   | 生产 bundle 成功生成带 important 的局部外框和焦点环 utilities。                                |
| `pnpm tauri dev` 或已安装 bundle 的人工检查                    | 视觉与交互      | 每个转换下拉框在 Mocha 与 Latte 中只显示一个 Blue 环；指针、禁用、chevron 和下拉行为保持正确。 |

## Definition of Done

- R1-R4 在作用域、权限和高级安装实例下拉框上全部满足。
- U1 验证通过，且不改变转换语义或全局焦点合同。
- 无法自动检查 Tauri 时，交付已安装应用并明确请求截图确认，不将纯浏览器证据当作 WKWebView 证明。
- 分支中不包含本任务引入的废弃样式实验或无关修改。

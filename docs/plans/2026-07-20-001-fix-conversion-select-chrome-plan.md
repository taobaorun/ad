---
title: Conversion Select Chrome - Plan
type: fix
date: 2026-07-20
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
---

# Conversion Select Chrome - Plan

## Goal Capsule

- **Objective:** Remove the native macOS inset and gradient chrome from select controls in the Claude Code to Codex conversion dialog while preserving clear dropdown affordance and existing behavior.
- **Authority:** The user's screenshot and request define the visual defect; `docs/design-docs/theme-system.md` defines the semantic surface, border, focus, and dual-theme rules.
- **Stop conditions:** Stop if removing native appearance prevents keyboard selection, disabled states, or visible focus from working.
- **Execution profile:** One localized React styling change with existing behavior tests and macOS visual verification.
- **Tail ownership:** LFG owns review, browser verification, commit, PR, and CI after implementation.

## Product Contract

### Summary

The conversion dialog will render its scope, permission, and advanced installation selects as flat theme-aware surfaces without the native macOS inner gradient or inset bezel.

### Problem Frame

The conversion dialog applies semantic background and border classes to native `<select>` elements, but WKWebView still paints the macOS menulist appearance above those colors. This produces the highlighted nested dark capsule, gradient, inner shadow, and native double-arrow chrome, making the controls inconsistent with adjacent inputs and the theme system.

### Requirements

- R1. Conversion-dialog selects must not display the native macOS inset, gradient, or inner bezel.
- R2. Scope, permission, and advanced installation selects must retain a visible dropdown affordance, their current option values, change handlers, disabled behavior, and keyboard interaction.
- R3. The controls must continue using the existing semantic background, border, text, disabled, and focus treatment in both Mocha and Latte themes.

### Scope Boundaries

- The change is limited to `src/components/AgentConversionDialog.tsx`.
- The title-bar Agent selector and global native-control styling are out of scope.
- Conversion behavior, Rust code, and i18n resources remain unchanged.

## Planning Contract

### Key Technical Decisions

- KTD1. Disable native select appearance only inside the conversion dialog, then provide a custom `ChevronDown` indicator. This removes WKWebView chrome without changing every select in the application or losing dropdown discoverability.
- KTD2. Reuse the existing semantic Tailwind roles rather than introducing literal colors or new design tokens. The defect is native rendering, not a missing palette role.

### Assumptions

- The screenshot's highlighted effect is the closed select's macOS/WKWebView native menulist chrome, so `appearance-none` is the correct boundary for the visual fix.
- A single downward chevron is the intended replacement affordance for the native double-arrow glyph.

## Implementation Units

### U1. Flatten conversion-dialog selects

- **Goal:** Render every select in the conversion dialog with one flat semantic surface and a consistent custom dropdown indicator.
- **Requirements:** R1, R2, R3; implements KTD1 and KTD2.
- **Dependencies:** None.
- **Files:** Modify `src/components/AgentConversionDialog.tsx`; inspect unchanged behavior coverage in `tests/components/AgentConversionDialog.test.tsx`.
- **Approach:** Add a small dialog-local select wrapper that applies `appearance-none`, reserves space for a non-interactive `ChevronDown`, and preserves the existing select props and option children. Use it for scope, permissions, and advanced installation selection without changing state or handlers. Preserve the native `disabled` attribute while dimming both the control and custom chevron and showing the disabled cursor through existing semantic state classes.
- **Patterns to follow:** Use `lucide-react` icons with `aria-hidden` for decorative UI; keep semantic Tailwind aliases from `docs/design-docs/theme-system.md`; preserve the global focus rule in `src/styles/globals.css`.
- **Test scenarios:** Test expectation: none -- this is a styling-only change that jsdom cannot distinguish from native WKWebView rendering; existing component tests already exercise selection, disabled state, and decision propagation unchanged.
- **Verification:** Existing conversion dialog tests remain green; type checking and lint pass; macOS visual inspection confirms a flat background, correctly positioned chevron, visible focus, disabled styling, and working keyboard selection in Mocha and Latte.

## Verification Contract

| Gate                                                           | Applicability          | Done signal                                                                                                                                               |
| -------------------------------------------------------------- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm test -- tests/components/AgentConversionDialog.test.tsx` | U1 behavior regression | Existing scope, permissions, busy-state, and instance-selection tests pass.                                                                               |
| `pnpm typecheck`                                               | U1 TypeScript contract | No type errors.                                                                                                                                           |
| `pnpm lint`                                                    | U1 code quality        | No lint errors or warnings.                                                                                                                               |
| macOS conversion-dialog inspection                             | R1-R3 visual behavior  | Scope, permissions, and advanced instance selects have no inner native bezel in both themes; mouse, keyboard, focus, and disabled behavior remain usable. |

## Definition of Done

- R1-R3 are satisfied across all conversion-dialog select variants.
- U1 verification passes without changing conversion semantics or global select styling.
- The branch contains no abandoned styling experiments or unrelated edits introduced by this task.

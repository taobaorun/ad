# 002 — Establish motion tokens and correct interactive primitives

- **Status**: TODO
- **Commit**: f6339ed
- **Severity**: MEDIUM
- **Category**: Easing & duration; Physicality; Performance; Accessibility; Cohesion
- **Estimated scope**: 13 files, about 120 changed lines

## Problem

The app has no shared motion vocabulary. Timings are handwritten as 100ms, 120ms, 150ms, 160ms, and 200ms with bare `ease-out`. The shared Button has no press feedback, Tabs uses `transition-all`, Toggle uses entry/exit easing for persistent movement, progress bars animate layout width, and several busy indicators ignore reduced motion.

```tsx
/* src/components/ui/button.tsx:6-8 — current */
'... transition-colors ...';
```

```tsx
/* src/components/ui/tabs.tsx:67 — current */
'... transition-all ...';
```

```tsx
/* src/components/SkillToggle.tsx:32-35 — current */
transform: on ? 'translateX(16px)' : 'translateX(0)',
transition: 'transform 0.12s ease-out',
```

```tsx
/* src/components/ResourceRemovalDialog.tsx:76-80 — current */
className="h-full rounded-full bg-primary transition-[width]"
style={{ width: `${percentage}%` }}
```

## Target

Add these exact tokens to both theme modes through `:root` inheritance in `src/styles/globals.css`:

```css
--motion-ease-out: cubic-bezier(0.23, 1, 0.32, 1);
--motion-ease-in-out: cubic-bezier(0.77, 0, 0.175, 1);
--motion-ease-drawer: cubic-bezier(0.32, 0.72, 0, 1);
--motion-duration-press: 160ms;
--motion-duration-fast: 150ms;
--motion-duration-panel: 200ms;
```

- Enabled non-link shared buttons press to `scale(0.97)` using `transform 160ms var(--motion-ease-out)`; link buttons remain unscaled.
- Project rows and project workspace tab buttons use at most `scale(0.98)` for 100–160ms; no hover transform.
- Toggle thumb moves for 120ms using `var(--motion-ease-in-out)` because it moves between two persistent positions.
- Generic Tabs no longer uses `transition-all`; only intended colors may transition for `150ms ease`.
- Resource/source removal progress uses `transform: scaleX(percentage / 100)` with `transform-origin: left`; never animate width.
- Reduced motion removes press/position movement, makes progress changes immediate, and keeps color/opacity feedback.
- All busy spinners/pulses match `OperationProgress.tsx:31` by adding `motion-reduce:animate-none`.

## Repo conventions to follow

- Semantic tokens live in the first `:root` block of `src/styles/globals.css`.
- `src/components/OperationProgress.tsx:31` is the exemplar for a reduced-motion busy indicator.
- Continue using Tailwind utility composition; use arbitrary values such as `duration-[var(--motion-duration-press)]` and `ease-[var(--motion-ease-out)]` only where Tailwind emits valid CSS.

## Steps

1. Add the six motion tokens to `src/styles/globals.css` beside the semantic UI tokens.
2. Update `src/components/ui/button.tsx`: constrain transitions to intended properties, add enabled press scale, override the `link` variant to remain at scale 1, and add `motion-reduce:transform-none`.
3. Add subtle press feedback to `src/components/ProjectSidebar.tsx` project rows and `src/components/ProjectDetailSupport.tsx` `TabButton`; do not animate selection movement.
4. Replace `transition-all` in `src/components/ui/tabs.tsx` with explicit color transition using `150ms ease`.
5. Update `src/components/SkillToggle.tsx` to use `120ms var(--motion-ease-in-out)` and a reduced-motion no-transition branch.
6. In `ResourceRemovalDialog.tsx` and `SourceRemovalDialog.tsx`, replace percentage width with `transform: scaleX(${percentage / 100})`, `origin-left`, and `transition-transform 150ms var(--motion-ease-in-out)`. Reduced motion uses `transition-none`.
7. Add `motion-reduce:animate-none` to busy icons in `AgentConversionProgress.tsx`, `LegacySkillMigrationCard.tsx`, `ProjectDetailSupport.tsx`, `ResourceCenter.tsx`, and `SkillSources.tsx`. Preserve the surrounding `role=status`/text so a static icon is still understandable.
8. Add/update tests that assert the shared Button/Tabs contract, progress uses transform not width transition, and every listed busy indicator has a reduced-motion class.

## Boundaries

- Do NOT animate hover scale, list selection position, layout width/height, or tab indicator movement.
- Do NOT add bounce or a motion dependency.
- Do NOT alter business behavior or timing of async operations.
- Do NOT change startup animation; it has a separate accepted design and runtime evidence.
- If Tailwind rejects a CSS variable utility, use a small named CSS class in `globals.css`; do not invent different values.

## Verification

- **Mechanical**: focused component/theme tests, then `pnpm typecheck`, `pnpm lint`, `pnpm test`, and `pnpm build`.
- **Feel check**: use buttons, project rows, tabs, and Toggle at normal speed. Feedback must be visible only on press, finish within 160ms, and never make keyboard navigation wait.
- In DevTools at 10% speed, confirm progress bars composite with `transform` and do not change their layout width.
- Emulate `prefers-reduced-motion: reduce`: buttons/toggles/progress must drop movement; status text and color feedback remain.
- **Done when**: no `transition-all` remains in generic Tabs, no removal progress animates width, and all cited busy indicators honor reduced motion.

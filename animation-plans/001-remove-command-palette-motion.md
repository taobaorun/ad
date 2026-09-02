# 001 — Remove Command Palette open and close motion

- **Status**: TODO
- **Commit**: f6339ed
- **Severity**: HIGH
- **Category**: Purpose & frequency; Interruptibility
- **Estimated scope**: 3 files, about 80 changed lines

## Problem

The Command Palette is a keyboard-primary surface that may be opened and closed 100+ times per day. It currently adds authored entry and exit keyframes plus a 140ms delayed unmount. This makes a frequent action wait for decoration, and rapid close/reopen restarts from the keyframe origin instead of retargeting from the current presentation value.

```tsx
/* src/components/CommandPalette.tsx:65-72 — current */
const [present, setPresent] = useState(open);
const [closing, setClosing] = useState(false);
const presentRef = useRef(false);
const closeTimerRef = useRef<number | null>(null);
```

```css
/* src/styles/globals.css:356-384 — current */
@keyframes ad-command-palette-enter {
  /* opacity + translate + scale */
}
@keyframes ad-command-palette-exit {
  /* opacity + translate + scale */
}
.ad-command-palette-enter {
  animation: ad-command-palette-enter 160ms cubic-bezier(0.16, 1, 0.3, 1);
}
.ad-command-palette-exit {
  animation: ad-command-palette-exit 140ms ease-out forwards;
}
```

## Target

- Palette panel and scrim appear in the first rendered frame when `paletteOpen` becomes true.
- Palette panel and scrim unmount immediately when `paletteOpen` becomes false.
- Preserve the selected Quiet layout, combobox/listbox semantics, focus trap, active-option scrolling, and row-level press feedback.
- Preserve focus restoration without a timer. Do not steal focus when executing a command opens another focused surface.
- Remove all `ad-command-palette-*` keyframes/classes and animation-only presence state.

## Repo conventions to follow

- Open state remains owned by `useUiState.paletteOpen` in `src/store/ui.ts`.
- The shared `Dialog` focus implementation at `src/components/ui/dialog.tsx:42-88` demonstrates capture and cleanup without decorative delays.
- High-frequency row feedback remains the existing `active:scale-[0.985] duration-100` in `CommandPalette.tsx`; do not expand motion beyond the pressed row.

## Steps

1. In `src/components/CommandPalette.tsx`, remove `present`, `closing`, `presentRef`, `closeTimerRef`, and all delayed-close branches.
2. Replace the open/focus effect with one effect keyed by `open` and `prefill`: when open, capture `document.activeElement`, reset term/index, and focus the search input on the next task/frame; on cleanup, restore focus only when focus is still in the palette or on `document.body`.
3. Restore the direct `if (!open) return null` rendering gate.
4. Remove `data-closing` and `ad-command-palette-enter/exit` class selection from the panel. Keep every Quiet layout and ARIA class unchanged.
5. Delete the Command Palette keyframes/classes and their reduced-motion block from `src/styles/globals.css`.
6. Update `tests/components/CommandPalette.test.tsx`: Escape should remove the dialog without waiting 140ms, and focus must still return to the trigger. Add a rapid close/reopen test proving one dialog remains and the search input is focused without timers.

## Boundaries

- Do NOT revert the Quiet visual layout or its accessibility semantics.
- Do NOT animate the scrim, result list, active option, or global shortcut.
- Do NOT add a motion library or new dependency.
- If the cited presence state no longer exists, STOP and report drift.

## Verification

- **Mechanical**: `pnpm test -- tests/components/CommandPalette.test.tsx`; `pnpm typecheck`; `pnpm lint`; `pnpm test`; `pnpm build`; all must pass.
- **Feel check**: open/close with `⌘K` ten times rapidly. The palette must be present on the next frame, disappear immediately, never flash from a keyframe origin, and keep focus behavior correct.
- In DevTools Animations, opening and closing the palette must create no animation entry. Row press feedback may still appear.
- **Done when**: no `ad-command-palette-enter`, `ad-command-palette-exit`, palette keyframe, or 140ms close timer remains.

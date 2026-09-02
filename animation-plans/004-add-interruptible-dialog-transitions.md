# 004 — Add interruptible shared Dialog transitions

- **Status**: TODO
- **Commit**: f6339ed
- **Severity**: MEDIUM
- **Category**: Interruptibility; Missed opportunities; Accessibility
- **Estimated scope**: 3 files, about 110 changed lines

## Problem

The shared Dialog primitive immediately mounts and unmounts the entire overlay. Occasional review, install, removal, profile, and detection dialogs therefore teleport into and out of the centered viewport. A naïve keyframe fix would also restart on rapid reopen.

```tsx
/* src/components/ui/dialog.tsx:90-103 — current */
if (!open) return null;
return (
  <div className="fixed inset-0 ... bg-overlay/65 p-4">
    <div className="relative flex ... rounded-lg ... shadow-lg">
```

## Target

Use interruptible CSS transitions, never keyframes:

```css
.ad-dialog-overlay {
  transition: opacity 180ms var(--motion-ease-out);
}
.ad-dialog-panel {
  transition:
    opacity 180ms var(--motion-ease-out),
    transform 180ms var(--motion-ease-out);
}
.ad-dialog-panel[data-open='false'] {
  opacity: 0;
  transform: translateY(-4px) scale(0.97);
}
```

- Center origin is correct for a modal.
- Exit uses the same target/path and 180ms strong ease-out.
- Rapid close/reopen retargets from the current computed transform/opacity.
- Reduced motion removes transform and uses `opacity 200ms ease`.
- Focus trap, Escape, close-disabled behavior, title/description ids, sticky footer, and focus restoration remain correct.

## Repo conventions to follow

- Depend on plan 002 tokens `--motion-ease-out` and `--motion-duration-panel`.
- Follow the existing controlled `open` prop; do not create a second source of truth for product state.
- Use a small internal presence state only to retain the DOM through exit.

## Steps

1. Add internal `present` and `visualOpen` state to `src/components/ui/dialog.tsx`. When `open` becomes true, render closed, then set visual-open on the next animation frame. When false, set visual-open false and retain DOM.
2. Remove retained DOM on `transitionend` from the panel only when still closed. Include a 240ms safety timeout for environments that omit transition events; clear it on reopen/unmount.
3. Move focus capture to the true open edge. Restore focus after exit only if no newly opened surface has already moved focus elsewhere.
4. Add overlay/panel classes and exact transitions in `src/styles/globals.css`, driven by `data-open="true|false"`.
5. Add reduced-motion CSS: opacity `200ms ease`, no transform.
6. Add `tests/components/Dialog.test.tsx` covering first entry state, Escape, close-disabled, focus trap, exit retention, focus restoration, and close/reopen interruption.
7. Run representative existing consumers without changing them: AgentPlanDialog, AgentProfilesDialog, SourceRemovalDialog, ResourceRemovalDialog, DetectedProjectsModal.

## Boundaries

- Do NOT animate dialog height, width, padding, footer, or scroll position.
- Do NOT stagger dialog contents or add blur animation.
- Do NOT convert the component to a third-party library.
- Do NOT modify custom legacy/risk dialogs in this plan.
- If plan 002 tokens are absent, execute 002 first.

## Verification

- **Mechanical**: focused Dialog and representative consumer tests, then `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm build`.
- **Feel check**: open/close five representative dialogs and reverse one before exit completes. The surface must stay centered, never flash, and never restart from scale 0.97 after reversal.
- At 10% speed, overlay and panel must settle together within 180ms.
- With reduced motion, only a 200ms opacity fade remains.
- **Done when**: shared dialogs no longer teleport and rapid reopen is continuous without focus regression.

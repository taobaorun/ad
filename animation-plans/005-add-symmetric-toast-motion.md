# 005 — Add symmetric activation Toast motion

- **Status**: TODO
- **Commit**: f6339ed
- **Severity**: MEDIUM
- **Category**: Spatial consistency; Interruptibility; Missed opportunities
- **Estimated scope**: 3 files, about 100 changed lines

## Problem

Activation toasts are occasionally inserted into and removed from a bottom-right stack with no spatial bridge. Manual dismissal and the eight-second timeout both delete the item immediately.

```tsx
/* src/components/ActivateToast.tsx:15-18 — current */
{
  toasts.map((toast) => <ToastCard key={toast.id} toast={toast} />);
}
```

```tsx
/* src/components/ActivateToast.tsx:27-30 — current */
const t = setTimeout(() => dismiss(toast.id), TOAST_TTL_MS);
return () => clearTimeout(t);
```

## Target

- Each card enters from its own bottom edge with `opacity: 0; transform: translateY(100%)`, then settles to opacity 1 / translateY(0).
- Entry: `220ms var(--motion-ease-out)` where the curve is `cubic-bezier(0.23, 1, 0.32, 1)`.
- Exit: same path to `translateY(100%)`, `160ms var(--motion-ease-out)`.
- Manual close and timeout both start exit, then call store `dismissToast` only after transition completes.
- Rapid manual close during entry retargets the CSS transition from the current presentation state. No keyframes.
- Reduced motion removes translation and uses `opacity 200ms ease`.
- Do not animate stack layout/reflow or stagger multiple toasts.

## Repo conventions to follow

- Reuse plan 002 motion tokens.
- Keep the Zustand store as the source of toast identity and the maximum-three policy.
- Preserve `role=status`, `aria-live=polite`, notification side effect, copy button, and eight-second lifetime.

## Steps

1. Add internal `visible`/`exiting` state to `ToastCard`, initialized false. On mount, set visible true on the next animation frame.
2. Replace direct timeout dismissal with `beginDismiss`: set exiting/visible false, then dismiss on card `transitionend`; add a 240ms safety timeout and clear it on unmount.
3. Route the close button through the same idempotent `beginDismiss` function.
4. Add exact toast transition classes in `src/styles/globals.css`; transition only transform and opacity.
5. Add reduced-motion CSS for 200ms opacity-only feedback.
6. Add/update toast tests covering entry, manual exit, timeout exit, repeated dismissal, timer cleanup, and `aria-live` stability.

## Boundaries

- Do NOT change toast TTL, copy behavior, notification behavior, position, width, or stack cap.
- Do NOT add bounce, drag-to-dismiss, stack FLIP, or a motion dependency.
- Do NOT reuse Command Palette keyframes.
- If plan 002 tokens are absent, execute 002 first.

## Verification

- **Mechanical**: focused toast tests plus `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm build`.
- **Feel check**: produce one toast, then three in a burst; dismiss during entry and after settled. Each card must follow the same bottom-edge path and never restart or bounce. Existing cards may reposition instantly.
- At 10% speed, card opacity and translate must begin/end together; no layout property may animate.
- With reduced motion, translation disappears but a 200ms opacity bridge remains.
- **Done when**: timeout and manual dismissal share one symmetric, interruptible path and the store removes cards only after exit.

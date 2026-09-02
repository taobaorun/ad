# 003 — Repair Profile Drawer first-open and reversible motion

- **Status**: TODO
- **Commit**: f6339ed
- **Severity**: MEDIUM
- **Category**: Easing & duration; Physicality & origin; Accessibility
- **Estimated scope**: 3 files, about 90 changed lines

## Problem

`GatedProfileEditDrawer` mounts `ProfileEditDrawer` only after `editingProfileId` is already non-null. The first rendered drawer therefore starts at `translate-x-0` and has no previous offscreen state to interpolate from. The scrim also appears instantly. On close, `editingId` becomes null immediately, so the editor content disappears before the panel finishes sliding out.

```tsx
/* src/App.tsx:126-134 — current */
const mounted = useHasBeenTrue(editingProfileId != null);
if (!mounted || !profileFeaturesFor(activeAgentId).legacyProjectTemplates) return null;
return <ProfileEditDrawer />;
```

```tsx
/* src/components/ProfileEditDrawer.tsx:41-48 — current */
{open && <div className="fixed inset-0 z-[80] bg-overlay/55" ... />}
'... transition-transform duration-200 ease-out ' +
  (open ? 'translate-x-0' : 'translate-x-full')
```

## Target

- First open renders one closed frame at `translateX(100%)`, then transitions to `translateX(0)`.
- Close transitions back to `translateX(100%)` while preserving the current editor content until the transform transition ends.
- Reopening mid-close retargets the CSS transition from the current transform; no keyframes.
- Drawer transition: `transform 200ms var(--motion-ease-drawer)` where `--motion-ease-drawer` is `cubic-bezier(0.32, 0.72, 0, 1)` from plan 002.
- Scrim transition: `opacity 200ms var(--motion-ease-out)` using `cubic-bezier(0.23, 1, 0.32, 1)`.
- Reduced motion: no translation; use opacity `200ms ease` and retain correct hidden/inert state.

## Repo conventions to follow

- Reuse the motion tokens introduced by plan 002; this plan depends on 002.
- Preserve `useHasBeenTrue` lazy chunk behavior in `src/App.tsx` unless a smaller change in the drawer itself proves sufficient.
- Use CSS transitions, not keyframes, so rapid reversal retargets from the presentation state.

## Steps

1. In `ProfileEditDrawer.tsx`, add a visual-open state initialized false plus a retained `displayedEditingId` initialized from the first non-null id.
2. On a new non-null `editingId`, retain it, render the drawer offscreen, then use `requestAnimationFrame` to set visual-open true. Cancel the frame on interruption.
3. On `editingId === null`, set visual-open false but keep `displayedEditingId`. Clear the retained id only after the transform transition completes while still closed.
4. Render scrim while a displayed id exists; drive opacity and pointer events from visual-open. Render `ProfileEditor` with `displayedEditingId` through exit.
5. Apply the exact drawer/scrim transition tokens and symmetric right-edge path. Add `aria-hidden` plus `inert` while visually closed.
6. Add reduced-motion CSS that removes translation but keeps a 200ms opacity bridge.
7. Add tests for first-open closed frame → open state, content retention during close, transition-end cleanup, and close/reopen interruption.

## Boundaries

- Do NOT change dirty-draft confirmation behavior.
- Do NOT add drag/swipe gestures, bounce, or a motion dependency.
- Do NOT change drawer width, editor content, or command palette behavior.
- If plan 002 tokens are absent, execute plan 002 first rather than duplicating curves.

## Verification

- **Mechanical**: focused drawer tests plus `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm build`.
- **Feel check**: open the drawer for the first time, close it, then reopen halfway through close. It must originate from and return to the right edge, retain content through exit, and never jump to an authored start frame.
- Inspect at 10% speed: the scrim and drawer begin together; exit mirrors entry.
- With reduced motion: no lateral movement; opacity still communicates arrival/removal.
- **Done when**: first open no longer teleports, closing content remains visible until transition end, and rapid reversal is continuous.

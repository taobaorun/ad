# Animation improvement plans

Plans were generated against commit `f6339ed` from a standard full-frontend audit. Production code was read-only during the audit.

| Order | Plan                                                                                                    | Severity | Status | Depends on |
| ----- | ------------------------------------------------------------------------------------------------------- | -------- | ------ | ---------- |
| 1     | [001 — Remove Command Palette open and close motion](001-remove-command-palette-motion.md)              | HIGH     | TODO   | —          |
| 2     | [002 — Establish motion tokens and correct interactive primitives](002-establish-motion-foundations.md) | MEDIUM   | TODO   | —          |
| 3     | [003 — Repair Profile Drawer first-open and reversible motion](003-repair-profile-drawer-motion.md)     | MEDIUM   | TODO   | 002        |
| 4     | [004 — Add interruptible shared Dialog transitions](004-add-interruptible-dialog-transitions.md)        | MEDIUM   | TODO   | 002        |
| 5     | [005 — Add symmetric activation Toast motion](005-add-symmetric-toast-motion.md)                        | MEDIUM   | TODO   | 002        |

## Recommended execution order

1. Remove the high-frequency Command Palette motion first; its selected Quiet layout and accessibility work remain.
2. Establish shared tokens and primitive-level behavior before adding occasional-surface motion.
3. Execute Drawer, Dialog, and Toast plans independently after plan 002. They touch separate components and may be verified in any order.

## Explicitly deferred

- Skill source accordion body motion: high-frequency, information-dense, and already has chevron state indication.
- Sidebar selection and core tab-content animation: high-frequency navigation should remain immediate.
- Operation success-banner motion and Resource Center card entrances: lower leverage than the five selected plans.
- Startup splash motion: governed by existing accepted design and real WKWebView evidence; not re-litigated here.

# Icons

Source artwork: `icon.png` (1024×1024 RGBA). Concept: a scope/crosshair with broken
orbit ring + `</>` brackets in the center. Dark navy background, blue ring,
green code symbol.

To regenerate every variant from a new source PNG:

```bash
# Drop your new 1024×1024 (or larger) PNG at icon.png, then:
pnpm tauri icon src-tauri/icons/icon.png
```

This produces all the macOS / Windows / iOS / Android variants Tauri's bundler
expects. The macOS `.app` and `.dmg` only consume the entries listed in
`tauri.conf.json > bundle.icon`.

## Tray icon

The macOS menubar tray icon is **not** generated from this PNG. It is rendered
procedurally at runtime by `src-tauri/src/tray/icon.rs` so it can pick up the
**active profile's color** dynamically. The `iconPath` in `tauri.conf.json >
app.trayIcon` is only used for the very first frame before any profile is
active.

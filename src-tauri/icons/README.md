# Icon placeholder

Drop your application icon here as `icon.png` (1024×1024 PNG), then run:

```bash
pnpm tauri icon icons/icon.png
```

This generates all required `.icns`, `.ico`, and resized `.png` variants used by `tauri.conf.json > bundle.icon`.

Until then, `pnpm tauri build` will warn about missing icon variants but will not fail.

/**
 * Toolbar button that opens the standalone Settings window.
 *
 * The settings UI lives in `SettingsApp.tsx`, mounted under the same origin
 * via the `#/settings` hash route (see `main.tsx`). We use a Tauri
 * `WebviewWindow` so it gets its own OS window — minimizable, movable
 * independently of the main window, matching the cmux-style UX.
 *
 * If the window already exists, we focus it instead of opening a duplicate.
 */

import { useTranslation } from 'react-i18next';
import { Settings as SettingsIcon } from 'lucide-react';
import { WebviewWindow, getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

const SETTINGS_LABEL = 'settings';

async function openSettingsWindow() {
  // Tauri 2: getByLabel was renamed; iterate getAll on WebviewWindow to find it.
  const existing = await WebviewWindow.getByLabel(SETTINGS_LABEL).catch(() => null);
  if (existing) {
    await existing.show();
    await existing.setFocus();
    return;
  }

  const current = getCurrentWebviewWindow();
  // Reuse the same Vite-built bundle; hash routing selects SettingsApp.
  const url = new URL(window.location.href);
  url.hash = '#/settings';

  const win = new WebviewWindow(SETTINGS_LABEL, {
    url: url.pathname + url.search + url.hash,
    title: '设置',
    width: 720,
    height: 520,
    minWidth: 560,
    minHeight: 400,
    resizable: true,
    parent: current.label,
    titleBarStyle: 'visible',
  });
  win.once('tauri://error', (e) => {
    console.error('Failed to open settings window:', e);
  });
}

export function AdvancedSettingsButton() {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      onClick={() => void openSettingsWindow()}
      title={t('advanced.openTitle')}
      aria-label={t('advanced.openTitle')}
      className="rounded p-1 text-muted-foreground hover:bg-muted"
    >
      <SettingsIcon className="h-4 w-4" />
    </button>
  );
}

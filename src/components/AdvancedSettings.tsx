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
import { tauri } from '@/lib/tauri';

export function AdvancedSettingsButton() {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      onClick={() => void tauri.openSettingsWindow()}
      title={t('advanced.openTitle')}
      aria-label={t('advanced.openTitle')}
      className="rounded p-1 text-muted-foreground hover:bg-muted"
    >
      <SettingsIcon className="h-4 w-4" />
    </button>
  );
}

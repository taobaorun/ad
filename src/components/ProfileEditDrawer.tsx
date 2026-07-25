/**
 * Right-side drawer for editing a template (formerly profile).
 *
 * Hosts ProfileEditor (Settings + Env tabs) at 480px wide. Controlled by
 * `useUiState.editingProfileId`. In v0.4 the drawer is reachable only via
 * the ⌘K command palette ("manage templates" / "edit template"); editing
 * a template never affects the currently active project — it only changes
 * the source-of-truth that future Switch-template / init flows copy from.
 *
 * On close, if the editor reported `dirty`, we confirm with the user
 * before unmounting — otherwise unsaved JSON edits would silently
 * disappear.
 */

import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiState } from '@/store/ui';
import { ProfileEditor } from './ProfileEditor';
import { X } from 'lucide-react';

export function ProfileEditDrawer() {
  const { t } = useTranslation();
  const editingId = useUiState((s) => s.editingProfileId);
  const closeEditDrawer = useUiState((s) => s.closeEditDrawer);
  const dirtyRef = useRef(false);

  // Reset dirty tracker when drawer switches profiles or closes.
  useEffect(() => {
    dirtyRef.current = false;
  }, [editingId]);

  function tryClose() {
    if (dirtyRef.current && !window.confirm(t('drawer.unsavedConfirm'))) return;
    closeEditDrawer();
  }

  const open = editingId != null;

  return (
    <>
      {open && (
        <div className="fixed inset-0 z-[80] bg-overlay/55" onClick={tryClose} aria-hidden />
      )}
      <aside
        className={
          'fixed right-0 top-0 z-[81] flex h-full w-[480px] max-w-full flex-col border-l border-border bg-background shadow-2xl transition-transform duration-200 ease-out ' +
          (open ? 'translate-x-0' : 'translate-x-full')
        }
        aria-hidden={!open}
      >
        <header className="flex items-center justify-between border-b border-border px-4 py-2.5">
          <h2 className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
            {t('drawer.editTemplate')}
          </h2>
          <button
            type="button"
            onClick={tryClose}
            className="rounded p-1 text-muted-foreground hover:bg-muted"
            aria-label={t('drawer.closeAria')}
          >
            <X className="h-4 w-4" />
          </button>
        </header>
        <div className="flex-1 overflow-hidden">
          {editingId && (
            <ProfileEditor
              key={editingId}
              profileId={editingId}
              onDirty={(d) => {
                dirtyRef.current = d;
              }}
            />
          )}
        </div>
      </aside>
    </>
  );
}

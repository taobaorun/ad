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

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiState } from '@/store/ui';
import { ProfileEditor } from './ProfileEditor';
import { X } from 'lucide-react';

export function ProfileEditDrawer() {
  const { t } = useTranslation();
  const editingId = useUiState((s) => s.editingProfileId);
  const closeEditDrawer = useUiState((s) => s.closeEditDrawer);
  const dirtyRef = useRef(false);
  const drawerRef = useRef<HTMLElement>(null);
  const frameRef = useRef<number | null>(null);
  const closeTimerRef = useRef<number | null>(null);
  const [displayedEditingId, setDisplayedEditingId] = useState(editingId);
  const [visualOpen, setVisualOpen] = useState(false);

  // Reset dirty tracker when drawer switches profiles or closes.
  useEffect(() => {
    dirtyRef.current = false;
  }, [editingId]);

  useEffect(() => {
    const drawer = drawerRef.current;
    if (!drawer) return;
    if (visualOpen) drawer.removeAttribute('inert');
    else drawer.setAttribute('inert', '');
  }, [visualOpen]);

  useEffect(() => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    if (closeTimerRef.current !== null) window.clearTimeout(closeTimerRef.current);
    if (editingId) {
      setDisplayedEditingId(editingId);
      frameRef.current = requestAnimationFrame(() => setVisualOpen(true));
      return () => {
        if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
      };
    }
    setVisualOpen(false);
    closeTimerRef.current = window.setTimeout(() => setDisplayedEditingId(null), 240);
    return () => {
      if (closeTimerRef.current !== null) window.clearTimeout(closeTimerRef.current);
    };
  }, [editingId]);

  function tryClose() {
    if (dirtyRef.current && !window.confirm(t('drawer.unsavedConfirm'))) return;
    closeEditDrawer();
  }

  function finishClose() {
    if (visualOpen || editingId) return;
    if (closeTimerRef.current !== null) window.clearTimeout(closeTimerRef.current);
    setDisplayedEditingId(null);
  }

  return (
    <>
      {displayedEditingId && (
        <div
          className={`ad-profile-drawer-scrim fixed inset-0 z-[80] bg-overlay/55 ${
            visualOpen ? 'opacity-100' : 'pointer-events-none opacity-0'
          }`}
          onClick={tryClose}
          aria-hidden
        />
      )}
      <aside
        ref={drawerRef}
        className={`ad-profile-drawer fixed right-0 top-0 z-[81] flex h-full w-[480px] max-w-full flex-col border-l border-border bg-background shadow-2xl ${
          visualOpen ? 'translate-x-0' : 'pointer-events-none translate-x-full'
        }`}
        data-open={visualOpen ? 'true' : 'false'}
        aria-hidden={!visualOpen}
        onTransitionEnd={(event) => {
          if (event.currentTarget === event.target) finishClose();
        }}
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
          {displayedEditingId && (
            <ProfileEditor
              key={displayedEditingId}
              profileId={displayedEditingId}
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

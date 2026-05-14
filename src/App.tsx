import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { ProfileList } from './components/ProfileList';
import { ProfileEditor } from './components/ProfileEditor';
import { HistoryPanel } from './components/HistoryPanel';
import { ImportDialog } from './components/ImportDialog';
import { ActivateToast } from './components/ActivateToast';
import { useProfiles } from './store/profiles';

export function App() {
  const loadAll = useProfiles((s) => s.loadAll);
  const view = useProfiles((s) => s.view);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  // Listen for activations triggered from anywhere (tray menu, future CLI, etc.)
  // and refresh the store so the titlebar + sidebar stay in sync. UI-driven
  // activations also fire this — the state is already correct so loadAll is
  // a fast no-op refresh from disk.
  useEffect(() => {
    const unlisten = listen('profile-activated', () => {
      void useProfiles.getState().loadAll();
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  return (
    <div className="flex h-full w-full flex-col">
      <Titlebar />
      <div className="flex flex-1 overflow-hidden">
        <aside className="w-64 shrink-0 border-r border-border bg-muted/30">
          <ProfileList />
        </aside>
        <main className="flex flex-1 flex-col overflow-hidden">
          {view === 'editor' && <ProfileEditor />}
          {view === 'history' && <HistoryPanel />}
        </main>
      </div>
      <ImportDialog />
      <ActivateToast />
    </div>
  );
}

function Titlebar() {
  const activeId = useProfiles((s) => s.activeId);
  const activeName =
    useProfiles((s) => s.profiles.find((p) => p.id === s.activeId))?.displayName ?? '—';

  return (
    <header className="flex h-9 items-center justify-between border-b border-border bg-background px-3 text-sm">
      <div className="font-semibold">cc-switch</div>
      <div className="flex items-center gap-2 text-muted-foreground">
        <span className="inline-block h-2 w-2 rounded-full bg-green-500" />
        <span>active: {activeId ? activeName : 'none'}</span>
      </div>
    </header>
  );
}

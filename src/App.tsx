import { lazy, Suspense, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { ProjectSidebar } from './components/ProjectSidebar';
import { ProjectDetail } from './components/ProjectDetail';
import { GlobalKeymap } from './components/GlobalKeymap';
import { ActivateToast } from './components/ActivateToast';
import { AdvancedSettingsButton } from './components/AdvancedSettings';
import { useProfiles } from './store/profiles';
import { useProjects } from './store/projects';
import { useUiState } from './store/ui';
import { useUiSettings } from './store/uiSettings';
import { AgentSelector } from './components/AgentSelector';
import { AgentConversionButton } from './components/AgentConversionDialog';
import { AgentProfilesButton } from './components/AgentProfilesDialog';
import { useAgents } from './store/agents';
import { useReloadProfilesOnAgentChange } from './hooks/useReloadProfilesOnAgentChange';
import { profileFeaturesFor } from './lib/profileEditorRegistry';
import { applyDocumentTheme } from './lib/theme';

// Heavy dialogs/drawers are lazy-loaded so they don't sit in the first-paint
// chunk. Each is wrapped in a tiny "gate" below that only mounts the lazy
// component after its open state has been true at least once — afterwards
// the component stays mounted so its open/close animations keep working.
const ProfileEditDrawer = lazy(() =>
  import('./components/ProfileEditDrawer').then((m) => ({ default: m.ProfileEditDrawer })),
);
const CommandPalette = lazy(() =>
  import('./components/CommandPalette').then((m) => ({ default: m.CommandPalette })),
);
const HistoryDialog = lazy(() =>
  import('./components/HistoryDialog').then((m) => ({ default: m.HistoryDialog })),
);
const DetectedProjectsModal = lazy(() =>
  import('./components/DetectedProjectsModal').then((m) => ({ default: m.DetectedProjectsModal })),
);
const ImportDialog = lazy(() =>
  import('./components/ImportDialog').then((m) => ({ default: m.ImportDialog })),
);

function useHasBeenTrue(condition: boolean): boolean {
  const [seen, setSeen] = useState(false);
  if (condition && !seen) setSeen(true);
  return seen;
}

/**
 * A′ main window (cmux-inspired): two-column layout with a compact
 * project sidebar on the left and a detail pane on the right. All
 * overlays (palette / drawer / dialogs) sit on top. Global keymap
 * binds ⌘K, ⌘1-8, ⌘T, ⌘P, ⌘E, ⌘⇧K, esc.
 */
export function App() {
  const sidebarCollapsed = useUiState((s) => s.sidebarCollapsed);
  const openPalette = useUiState((s) => s.openPalette);
  const darkMode = useUiSettings((s) => s.darkMode);
  const setDarkMode = useUiSettings((s) => s.setDarkMode);
  useReloadProfilesOnAgentChange();

  useEffect(() => {
    applyDocumentTheme(darkMode);
  }, [darkMode]);

  // Block WebKit gesture events (two-finger rotate/pinch) that cause page wobble.
  useEffect(() => {
    const block = (e: Event) => e.preventDefault();
    document.addEventListener('gesturestart', block, { passive: false });
    document.addEventListener('gesturechange', block, { passive: false });
    document.addEventListener('gestureend', block, { passive: false });
    return () => {
      document.removeEventListener('gesturestart', block);
      document.removeEventListener('gesturechange', block);
      document.removeEventListener('gestureend', block);
    };
  }, []);

  // Tray (or future CLI) can fire `profile-activated` — refresh so the
  // sidebar's profile color dot stays accurate.
  useEffect(() => {
    const unlisten = listen('profile-activated', () => {
      void useProfiles.getState().loadAll();
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  return (
    <div className="flex h-full w-full min-w-0 flex-col overflow-hidden">
      <Titlebar
        onOpenPalette={() => openPalette()}
        dark={darkMode}
        onToggleDark={() => setDarkMode(!darkMode)}
      />

      <div className="flex min-w-0 flex-1 overflow-hidden">
        {!sidebarCollapsed && (
          <aside className="w-[264px] shrink-0" style={{ background: 'var(--ds-bg-sidebar)' }}>
            <ProjectSidebar />
          </aside>
        )}
        <main className="flex min-w-0 flex-1 overflow-hidden">
          <ProjectDetail />
        </main>
      </div>

      <GatedProfileEditDrawer />
      <GatedCommandPalette />
      <GatedHistoryDialog />
      <GatedDetectedProjectsModal />
      <GatedImportDialog />
      <ActivateToast />
      <GlobalKeymap />
    </div>
  );
}

// Each Gated* below defers mounting its lazy child until the dialog's open
// state has been true at least once. After that the child stays mounted so
// Radix can keep driving open/close transitions via the `open` prop (or via
// the store the child reads internally).

function GatedProfileEditDrawer() {
  const editingProfileId = useUiState((s) => s.editingProfileId);
  const activeAgentId = useAgents((s) => s.activeAgentId);
  const mounted = useHasBeenTrue(editingProfileId != null);
  if (!mounted || !profileFeaturesFor(activeAgentId).legacyProjectTemplates) return null;
  return (
    <Suspense fallback={null}>
      <ProfileEditDrawer />
    </Suspense>
  );
}

function GatedCommandPalette() {
  const paletteOpen = useUiState((s) => s.paletteOpen);
  const mounted = useHasBeenTrue(paletteOpen);
  if (!mounted) return null;
  return (
    <Suspense fallback={null}>
      <CommandPalette />
    </Suspense>
  );
}

function GatedHistoryDialog() {
  const view = useProfiles((s) => s.view);
  const setView = useProfiles((s) => s.setView);
  const mounted = useHasBeenTrue(view === 'history');
  if (!mounted) return null;
  return (
    <Suspense fallback={null}>
      <HistoryDialog open={view === 'history'} onClose={() => setView('editor')} />
    </Suspense>
  );
}

function GatedDetectedProjectsModal() {
  const open = useProjects((s) => s.detectedModalOpen);
  const mounted = useHasBeenTrue(open);
  if (!mounted) return null;
  return (
    <Suspense fallback={null}>
      <DetectedProjectsModal />
    </Suspense>
  );
}

function GatedImportDialog() {
  const open = useProfiles((s) => s.importOpen);
  const activeAgentId = useAgents((s) => s.activeAgentId);
  const mounted = useHasBeenTrue(open);
  if (!mounted || !profileFeaturesFor(activeAgentId).legacyImport) return null;
  return (
    <Suspense fallback={null}>
      <ImportDialog />
    </Suspense>
  );
}

function Titlebar({
  onOpenPalette,
  dark,
  onToggleDark,
}: {
  onOpenPalette: () => void;
  dark: boolean;
  onToggleDark: () => void;
}) {
  const { t } = useTranslation();
  return (
    <header
      className="flex h-9 items-center justify-between px-3 text-sm"
      style={{ borderBottom: '0.5px solid var(--ds-line)', background: 'rgb(var(--background))' }}
    >
      <div className="font-semibold" style={{ fontSize: 14 }}>
        AD
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onOpenPalette}
          className="inline-flex items-center gap-2 rounded-lg px-2.5 py-1 text-xs"
          style={{
            border: '0.5px solid var(--ds-line-strong)',
            background: 'var(--ds-bg-card)',
            color: 'var(--ds-fg-3)',
            minWidth: 200,
          }}
          title={t('app.titlebar.openPaletteTitle')}
        >
          <kbd
            className="font-mono"
            style={{
              fontSize: 10.5,
              padding: '0 4px',
              borderRadius: 4,
              background: 'var(--ds-bg-soft)',
              border: '0.5px solid var(--ds-line)',
              color: 'var(--ds-fg-3)',
            }}
          >
            ⌘K
          </kbd>
          <span>{t('app.titlebar.runAnything')}</span>
          <svg
            width="13"
            height="13"
            viewBox="0 0 13 13"
            fill="none"
            style={{ marginLeft: 'auto' }}
          >
            <circle cx="5.5" cy="5.5" r="4" stroke="currentColor" strokeWidth="1.2" />
            <path d="M8.5 8.5l3 3" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
        </button>
        <AdvancedSettingsButton />
        <AgentProfilesButton />
        <AgentConversionButton />
        <AgentSelector />
        <button
          type="button"
          onClick={onToggleDark}
          title={dark ? t('app.titlebar.switchToLight') : t('app.titlebar.switchToDark')}
          aria-label={dark ? t('app.titlebar.switchToLight') : t('app.titlebar.switchToDark')}
          style={{
            width: 28,
            height: 28,
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            borderRadius: 6,
            background: 'transparent',
            border: 0,
            color: 'var(--ds-fg-3)',
            cursor: 'pointer',
          }}
        >
          {dark ? (
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <path
                d="M14 10.5A6.5 6.5 0 015.5 2a6.5 6.5 0 108.5 8.5z"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinejoin="round"
              />
            </svg>
          ) : (
            <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
              <circle cx="8" cy="8" r="3.5" stroke="currentColor" strokeWidth="1.3" />
              <line
                x1="8"
                y1="1"
                x2="8"
                y2="2.5"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinecap="round"
              />
              <line
                x1="8"
                y1="13.5"
                x2="8"
                y2="15"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinecap="round"
              />
              <line
                x1="1"
                y1="8"
                x2="2.5"
                y2="8"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinecap="round"
              />
              <line
                x1="13.5"
                y1="8"
                x2="15"
                y2="8"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinecap="round"
              />
              <line
                x1="3.05"
                y1="3.05"
                x2="4.1"
                y2="4.1"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinecap="round"
              />
              <line
                x1="11.9"
                y1="11.9"
                x2="12.95"
                y2="12.95"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinecap="round"
              />
              <line
                x1="12.95"
                y1="3.05"
                x2="11.9"
                y2="4.1"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinecap="round"
              />
              <line
                x1="4.1"
                y1="11.9"
                x2="3.05"
                y2="12.95"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinecap="round"
              />
            </svg>
          )}
        </button>
      </div>
    </header>
  );
}

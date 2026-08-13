/**
 * UI state for the A′ redesign: which project is active, whether the
 * command palette / profile-edit drawer are open, sidebar collapsed.
 *
 * `activeProjectPath` and `sidebarCollapsed` persist to localStorage so
 * the layout state survives restarts. Everything else is session-only.
 *
 * Storage decision (see ExecPlan D7): originally planned to live in
 * `~/.ad/state/ui.json` via backend IPC, but localStorage keeps M1
 * scoped to the frontend with no new tauri commands.
 */

import { create } from 'zustand';
import { requestAgentWorkspaceChange } from '@/lib/workspaceDraftGuard';

const STORAGE_KEY = 'ad.ui-state.v1';

interface Persisted {
  activeProjectPath: string | null;
  sidebarCollapsed: boolean;
  workspaceMode: 'projects' | 'resources';
}

const defaults: Persisted = {
  activeProjectPath: null,
  sidebarCollapsed: false,
  workspaceMode: 'projects',
};

function load(): Persisted {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaults;
    const parsed = JSON.parse(raw) as Partial<Persisted>;
    return { ...defaults, ...parsed };
  } catch {
    return defaults;
  }
}

function save(state: Persisted): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // ignore quota / privacy errors
  }
}

interface State extends Persisted {
  /** When the command palette is open, this is the (possibly empty) prefill text. */
  paletteOpen: boolean;
  palettePrefill: string;

  /** When non-null, the profile-edit drawer is open for this template id. */
  editingProfileId: string | null;

  /** When true, the SwitchTemplateDialog is open for the active project. */
  switchTemplateOpen: boolean;

  setActiveProject: (path: string | null) => void;
  setWorkspaceMode: (mode: 'projects' | 'resources') => void;
  openPalette: (prefill?: string) => void;
  closePalette: () => void;
  openEditDrawer: (profileId: string) => void;
  closeEditDrawer: () => void;
  openSwitchTemplate: () => void;
  closeSwitchTemplate: () => void;
  toggleSidebar: () => void;
}

export const useUiState = create<State>((set, get) => ({
  ...load(),
  paletteOpen: false,
  palettePrefill: '',
  editingProfileId: null,
  switchTemplateOpen: false,

  setActiveProject: (path) => {
    if (path !== get().activeProjectPath && !requestAgentWorkspaceChange()) return;
    set({ activeProjectPath: path, workspaceMode: 'projects' });
    save({
      activeProjectPath: path,
      sidebarCollapsed: get().sidebarCollapsed,
      workspaceMode: 'projects',
    });
  },

  setWorkspaceMode: (workspaceMode) => {
    if (workspaceMode !== get().workspaceMode && !requestAgentWorkspaceChange()) return;
    set({ workspaceMode });
    save({
      activeProjectPath: get().activeProjectPath,
      sidebarCollapsed: get().sidebarCollapsed,
      workspaceMode,
    });
  },

  openPalette: (prefill = '') => set({ paletteOpen: true, palettePrefill: prefill }),
  closePalette: () => set({ paletteOpen: false, palettePrefill: '' }),

  openEditDrawer: (profileId) => set({ editingProfileId: profileId }),
  closeEditDrawer: () => set({ editingProfileId: null }),

  openSwitchTemplate: () => set({ switchTemplateOpen: true }),
  closeSwitchTemplate: () => set({ switchTemplateOpen: false }),

  toggleSidebar: () => {
    const next = !get().sidebarCollapsed;
    set({ sidebarCollapsed: next });
    save({
      activeProjectPath: get().activeProjectPath,
      sidebarCollapsed: next,
      workspaceMode: get().workspaceMode,
    });
  },
}));

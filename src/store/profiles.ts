import { create } from 'zustand';
import { emit } from '@tauri-apps/api/event';
import { tauri, type ActivationResult, type ClaudeProcess } from '@/lib/tauri';
import { type ProfileFile, blankProfile } from '@/lib/profileSchema';

/// Frontend → backend event fired after any profile mutation. Tray subscribes
/// to it so the menubar ring color updates live when a user edits the active
/// profile's color in the editor — no re-activation required.
const PROFILES_CHANGED_EVENT = 'profiles-changed';

async function notifyProfilesChanged(): Promise<void> {
  try {
    await emit(PROFILES_CHANGED_EVENT);
  } catch {
    // Backend may not be listening yet during initial mount; that's fine.
  }
}

export type View = 'editor' | 'history';

export interface ToastPayload {
  id: number;
  profileName: string;
  detected: ClaudeProcess[];
}

interface State {
  profiles: ProfileFile[];
  activeId: string | null;
  selectedId: string | null;
  view: View;
  importOpen: boolean;
  toasts: ToastPayload[];
  loading: boolean;

  loadAll: () => Promise<void>;
  select: (id: string | null) => void;
  setView: (view: View) => void;
  setImportOpen: (open: boolean) => void;
  dismissToast: (id: number) => void;

  createNew: () => Promise<ProfileFile>;
  save: (profile: ProfileFile) => Promise<void>;
  remove: (id: string) => Promise<void>;
  activate: (id: string) => Promise<ActivationResult>;
}

function nextUntitledId(profiles: ProfileFile[]): string {
  const taken = new Set(profiles.map((p) => p.id));
  let i = 1;
  while (taken.has(`untitled-${i}`)) i += 1;
  return `untitled-${i}`;
}

// In-flight loadAll dedupe: if a second loadAll arrives while the first is
// pending, we return the same promise so the two callers see consistent state.
let inflightLoadAll: Promise<void> | null = null;

let nextToastId = 1;

export const useProfiles = create<State>((set, get) => ({
  profiles: [],
  activeId: null,
  selectedId: null,
  view: 'editor',
  importOpen: false,
  toasts: [],
  loading: false,

  loadAll: async () => {
    if (inflightLoadAll) return inflightLoadAll;
    const promise = (async () => {
      set({ loading: true });
      try {
        const [profiles, activeId] = await Promise.all([
          tauri.listProfiles(),
          tauri.getActiveProfileId(),
        ]);
        const selectedId = get().selectedId ?? activeId ?? profiles[0]?.id ?? null;
        set({ profiles, activeId, selectedId });
      } finally {
        set({ loading: false });
        inflightLoadAll = null;
      }
    })();
    inflightLoadAll = promise;
    return promise;
  },

  select: (id) => set({ selectedId: id, view: 'editor' }),
  setView: (view) => set({ view }),
  setImportOpen: (open) => set({ importOpen: open }),
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  createNew: async () => {
    const id = nextUntitledId(get().profiles);
    const created = await tauri.saveProfile(blankProfile(id));
    await get().loadAll();
    set({ selectedId: created.id });
    await notifyProfilesChanged();
    return created;
  },

  save: async (profile) => {
    await tauri.saveProfile({ ...profile, updatedAt: new Date().toISOString() });
    await get().loadAll();
    await notifyProfilesChanged();
  },

  remove: async (id) => {
    await tauri.deleteProfile(id);
    const remaining = get().profiles.filter((p) => p.id !== id);
    set({
      profiles: remaining,
      selectedId: remaining[0]?.id ?? null,
    });
    await notifyProfilesChanged();
  },

  activate: async (id) => {
    const res = await tauri.activateProfile(id);
    const profile = get().profiles.find((p) => p.id === res.activatedId);
    const toast: ToastPayload = {
      id: nextToastId++,
      profileName: profile?.displayName ?? res.activatedId,
      detected: res.detectedPids,
    };
    set((s) => ({
      activeId: res.activatedId,
      // Cap to 3 visible toasts; oldest discarded if more arrive in burst.
      toasts: [...s.toasts.slice(-2), toast],
    }));
    return res;
  },
}));

/**
 * Local UI preferences (M5). Persisted to `localStorage` — not synced with
 * the backend, since these are visual toggles per device, not per profile.
 *
 * Cross-window sync: settings open in a standalone Tauri window which shares
 * the same `localStorage` namespace (same origin: tauri://localhost). The
 * `storage` event fires in OTHER windows when one writes, so we subscribe to
 * it and hydrate the store, keeping both windows in lock-step.
 */

import { create } from 'zustand';
import { tauri, type TerminalBackendId } from '../lib/tauri';

const STORAGE_KEY = 'ad.ui-settings.v1';

/** Default binding for the show/hide global shortcut: ⌥⌘A on macOS. */
export const DEFAULT_GLOBAL_SHORTCUT = 'Alt+Cmd+KeyA';

interface TerminalPrefs {
  backend: TerminalBackendId;
  claudeBinPath: string;
  customCommand: string;
}

interface GlobalShortcutPrefs {
  enabled: boolean;
  /** Tauri shortcut string, e.g. "Alt+Cmd+KeyA". */
  binding: string;
}

interface Persisted {
  /** When true, the legacy [Activate] button is shown in ProfileEditor and
   * other legacy global-overwrite entry points. Defaults to false in v0.2. */
  showLegacyActivation: boolean;
  darkMode: boolean;
  terminal: TerminalPrefs;
  globalShortcut: GlobalShortcutPrefs;
}

const defaults: Persisted = {
  showLegacyActivation: false,
  darkMode: true,
  terminal: {
    backend: 'ghostty',
    claudeBinPath: '',
    customCommand: '',
  },
  globalShortcut: {
    enabled: true,
    binding: DEFAULT_GLOBAL_SHORTCUT,
  },
};

function load(): Persisted {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaults;
    const parsed = JSON.parse(raw) as Partial<Persisted>;
    return {
      ...defaults,
      ...parsed,
      terminal: { ...defaults.terminal, ...(parsed.terminal ?? {}) },
      globalShortcut: { ...defaults.globalShortcut, ...(parsed.globalShortcut ?? {}) },
    };
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
  setShowLegacyActivation: (v: boolean) => void;
  setDarkMode: (v: boolean) => void;
  setTerminal: (patch: Partial<TerminalPrefs>) => void;
  /** Update the global shortcut prefs and propagate to the Tauri backend.
   * Returns the error message if backend registration failed (so the UI can
   * surface it); resolves with `null` on success. */
  setGlobalShortcut: (patch: Partial<GlobalShortcutPrefs>) => Promise<string | null>;
  /** Re-sync the current persisted shortcut to the backend (boot-time call). */
  syncGlobalShortcutToBackend: () => Promise<void>;
}

export const useUiSettings = create<State>((set, get) => ({
  ...load(),
  setShowLegacyActivation: (v) => {
    set({ showLegacyActivation: v });
    save({ ...get(), showLegacyActivation: v });
  },
  setDarkMode: (v) => {
    set({ darkMode: v });
    save({ ...get(), darkMode: v });
    tauri.writeThemeHint(v).catch(() => {});
  },
  setTerminal: (patch) => {
    const next = { ...get().terminal, ...patch };
    set({ terminal: next });
    save({ ...get(), terminal: next });
  },
  setGlobalShortcut: async (patch) => {
    const next = { ...get().globalShortcut, ...patch };
    // Persist first so a backend failure doesn't lose the user's intent;
    // they can retry from Settings without their input disappearing.
    set({ globalShortcut: next });
    save({ ...get(), globalShortcut: next });
    try {
      await tauri.setGlobalShortcut(next.enabled ? next.binding : null);
      return null;
    } catch (e) {
      return e instanceof Error ? e.message : String(e);
    }
  },
  syncGlobalShortcutToBackend: async () => {
    const { globalShortcut: gs } = get();
    try {
      await tauri.setGlobalShortcut(gs.enabled ? gs.binding : null);
    } catch (err) {
      // Boot-time failure (e.g. binding conflicts) is non-fatal; the user
      // can change the binding in Settings. Surface to console for debugging.
      console.warn('Failed to register global shortcut on boot:', err);
    }
  },
}));

// Cross-window sync. The `storage` event only fires in OTHER windows of the
// same origin, so a write here will trigger this listener in the sibling
// window (main ↔ settings).
if (typeof window !== 'undefined') {
  window.addEventListener('storage', (e) => {
    if (e.key !== STORAGE_KEY || !e.newValue) return;
    try {
      const parsed = JSON.parse(e.newValue) as Partial<Persisted>;
      useUiSettings.setState({
        ...defaults,
        ...parsed,
        terminal: { ...defaults.terminal, ...(parsed.terminal ?? {}) },
        globalShortcut: { ...defaults.globalShortcut, ...(parsed.globalShortcut ?? {}) },
      });
    } catch {
      // ignore corrupted payload
    }
  });
}

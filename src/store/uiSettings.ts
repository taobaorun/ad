/**
 * Local UI preferences (M5). Persisted to `localStorage` — not synced with
 * the backend, since these are visual toggles per device, not per profile.
 */

import { create } from 'zustand';
import type { TerminalBackendId } from '../lib/tauri';

const STORAGE_KEY = 'ad.ui-settings.v1';

interface TerminalPrefs {
  backend: TerminalBackendId;
  claudeBinPath: string;
  customCommand: string;
}

interface Persisted {
  /** When true, the legacy [Activate] button is shown in ProfileEditor and
   * other legacy global-overwrite entry points. Defaults to false in v0.2. */
  showLegacyActivation: boolean;
  darkMode: boolean;
  terminal: TerminalPrefs;
}

const defaults: Persisted = {
  showLegacyActivation: false,
  darkMode: true,
  terminal: {
    backend: 'ghostty',
    claudeBinPath: '',
    customCommand: '',
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
  },
  setTerminal: (patch) => {
    const next = { ...get().terminal, ...patch };
    set({ terminal: next });
    save({ ...get(), terminal: next });
  },
}));

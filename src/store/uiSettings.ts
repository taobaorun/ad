/**
 * Local UI preferences (M5). Persisted to `localStorage` — not synced with
 * the backend, since these are visual toggles per device, not per profile.
 */

import { create } from 'zustand';

const STORAGE_KEY = 'ad.ui-settings.v1';

interface Persisted {
  /** When true, the legacy [Activate] button is shown in ProfileEditor and
   * other legacy global-overwrite entry points. Defaults to false in v0.2. */
  showLegacyActivation: boolean;
}

const defaults: Persisted = {
  showLegacyActivation: false,
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
  setShowLegacyActivation: (v: boolean) => void;
}

export const useUiSettings = create<State>((set, get) => ({
  ...load(),
  setShowLegacyActivation: (v) => {
    set({ showLegacyActivation: v });
    save({ showLegacyActivation: get().showLegacyActivation });
  },
}));

/**
 * Project + scan-roots state for the M4 UI. Separate store from
 * `useProfiles` because the lifetimes and load points are different
 * (projects load on first render, profiles already do).
 */

import { create } from 'zustand';
import { tauri } from '@/lib/tauri';
import type { Project, ScanRoot, DetectedProject } from '@/lib/projectTypes';

interface State {
  projects: Project[];
  scanRoots: ScanRoot[];
  loading: boolean;

  /** True when the auto-detect modal should be visible. */
  detectedModalOpen: boolean;
  /** Cached scan results. Refreshed on `openDetectedModal`. */
  detected: DetectedProject[];

  loadAll: () => Promise<void>;

  addProject: (path: string) => Promise<Project>;
  removeProject: (path: string) => Promise<void>;
  renameProject: (path: string, displayName: string) => Promise<Project>;

  openDetectedModal: () => Promise<void>;
  closeDetectedModal: () => void;
  refreshDetected: () => Promise<void>;

  addScanRoot: (path: string) => Promise<void>;
  removeScanRoot: (path: string) => Promise<void>;
  setScanRootEnabled: (path: string, enabled: boolean) => Promise<void>;
}

let inflightLoadAll: Promise<void> | null = null;

export const useProjects = create<State>((set, get) => ({
  projects: [],
  scanRoots: [],
  loading: false,
  detectedModalOpen: false,
  detected: [],

  loadAll: async () => {
    if (inflightLoadAll) return inflightLoadAll;
    const promise = (async () => {
      set({ loading: true });
      try {
        const [projects, scanRoots] = await Promise.all([
          tauri.listProjects(),
          tauri.listScanRoots(),
        ]);
        set({ projects, scanRoots });
      } finally {
        set({ loading: false });
        inflightLoadAll = null;
      }
    })();
    inflightLoadAll = promise;
    return promise;
  },

  addProject: async (path) => {
    const project = await tauri.addProject(path);
    await get().loadAll();
    return project;
  },

  removeProject: async (path) => {
    await tauri.removeProject(path);
    await get().loadAll();
  },

  renameProject: async (path, displayName) => {
    const updated = await tauri.renameProject(path, displayName);
    await get().loadAll();
    return updated;
  },

  openDetectedModal: async () => {
    set({ detectedModalOpen: true });
    await get().refreshDetected();
  },

  closeDetectedModal: () => set({ detectedModalOpen: false }),

  refreshDetected: async () => {
    const detected = await tauri.scanForProjects();
    set({ detected });
  },

  addScanRoot: async (path) => {
    const scanRoots = await tauri.addScanRoot(path);
    set({ scanRoots });
    await get().refreshDetected();
  },

  removeScanRoot: async (path) => {
    const scanRoots = await tauri.removeScanRoot(path);
    set({ scanRoots });
    await get().refreshDetected();
  },

  setScanRootEnabled: async (path, enabled) => {
    const scanRoots = await tauri.setScanRootEnabled(path, enabled);
    set({ scanRoots });
    await get().refreshDetected();
  },
}));

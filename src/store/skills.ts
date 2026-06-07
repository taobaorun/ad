import { create } from 'zustand';
import { tauri } from '@/lib/tauri';
import type {
  PluginInfo,
  ProjectSkillConfig,
  SkillEntry,
  SkillSource,
  SkillUpdateResult,
} from '@/lib/skillTypes';

interface SkillsState {
  sources: SkillSource[];
  entries: SkillEntry[];
  plugins: PluginInfo[];
  projectConfig: ProjectSkillConfig | null;
  loading: boolean;

  loadSources: () => Promise<void>;
  addSource: (source: SkillSource) => Promise<SkillSource>;
  removeSource: (id: string) => Promise<void>;
  updateSource: (id: string) => Promise<SkillUpdateResult>;

  scanLibrary: (projectPath?: string) => Promise<void>;

  loadProjectSkills: (projectPath: string) => Promise<void>;
  toggleSkill: (projectPath: string, skillId: string, enabled: boolean) => Promise<void>;
  setSkillScope: (skillId: string, scope: string) => Promise<void>;
  applyProjectSkills: (projectPath: string) => Promise<string[]>;

  loadPlugins: (projectPath?: string) => Promise<void>;
  togglePlugin: (projectPath: string, pluginId: string, enabled: boolean) => Promise<void>;
}

export const useSkills = create<SkillsState>((set, get) => ({
  sources: [],
  entries: [],
  plugins: [],
  projectConfig: null,
  loading: false,

  loadSources: async () => {
    const sources = await tauri.listSkillSources();
    set({ sources });
  },

  addSource: async (source) => {
    const saved = await tauri.addSkillSource(source);
    await get().loadSources();
    return saved;
  },

  removeSource: async (id) => {
    await tauri.removeSkillSource(id);
    await get().loadSources();
    await get().scanLibrary();
  },

  updateSource: async (id) => {
    const result = await tauri.updateSkillSource(id);
    await get().scanLibrary();
    return result;
  },

  scanLibrary: async (projectPath) => {
    set({ loading: true });
    try {
      const entries = await tauri.scanSkillLibrary(projectPath);
      set({ entries });
    } finally {
      set({ loading: false });
    }
  },

  loadProjectSkills: async (projectPath) => {
    const [config] = await Promise.all([
      tauri.getProjectSkills(projectPath),
      get().scanLibrary(projectPath),
      get().loadPlugins(projectPath),
    ]);
    set({ projectConfig: config });
  },

  toggleSkill: async (projectPath, skillId, enabled) => {
    const config = await tauri.toggleSkill(projectPath, skillId, enabled);
    set({ projectConfig: config });
    await get().scanLibrary(projectPath);
  },

  setSkillScope: async (skillId, scope) => {
    await tauri.setSkillScope(skillId, scope);
  },

  applyProjectSkills: async (projectPath) => {
    return tauri.applyProjectSkills(projectPath);
  },

  loadPlugins: async (projectPath) => {
    const plugins = await tauri.listPlugins(projectPath);
    set({ plugins });
  },

  togglePlugin: async (projectPath, pluginId, enabled) => {
    await tauri.togglePlugin(projectPath, pluginId, enabled);
    await get().loadPlugins(projectPath);
  },
}));

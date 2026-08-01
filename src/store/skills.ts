import { create } from 'zustand';
import { tauri } from '@/lib/tauri';
import type { PluginInfo, ProjectSkillConfig, SkillEntry } from '@/lib/skillTypes';
import type {
  SkillCatalogEntry,
  SkillCatalogOperationReport,
  SkillCatalogPlanView,
  SkillSourceRequest,
} from '@/lib/skillCatalogTypes';

interface SkillsState {
  sources: SkillCatalogEntry[];
  entries: SkillEntry[];
  plugins: PluginInfo[];
  projectConfig: ProjectSkillConfig | null;
  loading: boolean;

  loadSources: () => Promise<void>;
  previewAddSource: (source: SkillSourceRequest) => Promise<SkillCatalogPlanView>;
  previewRemoveSource: (sourceId: string) => Promise<SkillCatalogPlanView>;
  previewUpdateSource: (sourceId: string) => Promise<SkillCatalogPlanView>;
  applySourcePlan: (plan: SkillCatalogPlanView) => Promise<SkillCatalogOperationReport>;
  cancelSourcePlan: (planId: string) => Promise<void>;

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
    const catalog = await tauri.listSkillCatalog();
    set({ sources: catalog.entries });
  },

  previewAddSource: (source) => tauri.previewAddSkillCatalogSource(source),

  previewRemoveSource: (sourceId) => tauri.previewRemoveSkillCatalogSource(sourceId),

  previewUpdateSource: (sourceId) => tauri.previewUpdateSkillCatalogSource(sourceId),

  applySourcePlan: async (plan) => {
    const report = await tauri.applySkillCatalogSourcePlan(plan);
    await get().loadSources();
    return report;
  },

  cancelSourcePlan: async (planId) => {
    await tauri.cancelSkillCatalogSourcePlan(planId);
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

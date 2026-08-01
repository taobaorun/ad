import { create } from 'zustand';
import { tauri } from '@/lib/tauri';
import type {
  SkillCatalogEntry,
  SkillCatalogOperationReport,
  SkillCatalogPlanView,
  SkillSourceRequest,
} from '@/lib/skillCatalogTypes';

interface SkillsState {
  sources: SkillCatalogEntry[];

  loadSources: () => Promise<void>;
  previewAddSource: (source: SkillSourceRequest) => Promise<SkillCatalogPlanView>;
  previewRemoveSource: (sourceId: string) => Promise<SkillCatalogPlanView>;
  previewUpdateSource: (sourceId: string) => Promise<SkillCatalogPlanView>;
  applySourcePlan: (plan: SkillCatalogPlanView) => Promise<SkillCatalogOperationReport>;
  cancelSourcePlan: (planId: string) => Promise<void>;
}

export const useSkills = create<SkillsState>((set, get) => ({
  sources: [],

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
}));

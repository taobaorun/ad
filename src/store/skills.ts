import { create } from 'zustand';
import { tauri } from '@/lib/tauri';
import type {
  SkillCatalogEntry,
  SkillCatalogOperationReport,
  SkillCatalogPlanView,
  SkillSourcePreviewProgress,
  SkillSourceRequest,
} from '@/lib/skillCatalogTypes';

interface SkillsState {
  sources: SkillCatalogEntry[];

  loadSources: () => Promise<void>;
  previewAddSource: (
    source: SkillSourceRequest,
    onProgress?: (progress: SkillSourcePreviewProgress) => void,
  ) => Promise<SkillCatalogPlanView>;
  previewRemoveSource: (sourceId: string) => Promise<SkillCatalogPlanView>;
  previewUpdateSource: (sourceId: string) => Promise<SkillCatalogPlanView>;
  previewRollbackSourceUpdate: (receiptId: string) => Promise<SkillCatalogPlanView>;
  applySourcePlan: (plan: SkillCatalogPlanView) => Promise<SkillCatalogOperationReport>;
  cancelSourcePlan: (planId: string) => Promise<void>;
}

export const useSkills = create<SkillsState>((set, get) => ({
  sources: [],

  loadSources: async () => {
    const catalog = await tauri.listSkillCatalog();
    set({ sources: catalog.entries });
  },

  previewAddSource: (source, onProgress) => tauri.previewAddSkillCatalogSource(source, onProgress),

  previewRemoveSource: (sourceId) => tauri.previewRemoveSkillCatalogSource(sourceId),

  previewUpdateSource: (sourceId) => tauri.previewUpdateSkillCatalogSource(sourceId),

  previewRollbackSourceUpdate: (receiptId) =>
    tauri.previewRollbackSkillCatalogSourceUpdate(receiptId),

  applySourcePlan: async (plan) => {
    const report = await tauri.applySkillCatalogSourcePlan(plan);
    await get().loadSources();
    return report;
  },

  cancelSourcePlan: async (planId) => {
    await tauri.cancelSkillCatalogSourcePlan(planId);
  },
}));

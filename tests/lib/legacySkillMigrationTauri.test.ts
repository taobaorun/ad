import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { LegacySkillMigrationPlanView } from '@/lib/legacySkillMigrationTypes';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke,
  Channel: class Channel {},
}));

import { tauri } from '@/lib/tauri';

const plan: LegacySkillMigrationPlanView = {
  schemaVersion: 1,
  id: 'legacy-skill-migration-plan:one',
  projectPath: '/tmp/project',
  canonicalProjectPath: '/private/tmp/project',
  stateId: 'project.json',
  stateDigest: 'sha256:state',
  migrationReceiptIds: ['receipt:workspace'],
  confirmationRequired: true,
  riskFingerprint: 'risk:one',
  expiresAt: '2026-08-01T12:00:00Z',
};

describe('legacy Skill migration Tauri contracts', () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it('submits only the backend-issued plan claim when applying', async () => {
    invoke.mockResolvedValueOnce({ outcome: 'compensated', issues: ['state changed'] });

    await expect(tauri.applyLegacyProjectSkillMigrationPlan(plan)).resolves.toEqual({
      outcome: 'compensated',
      issues: ['state changed'],
    });
    expect(invoke).toHaveBeenCalledWith('apply_legacy_project_skill_migration_plan', {
      claim: {
        planId: plan.id,
        riskFingerprint: plan.riskFingerprint,
        confirmed: true,
      },
    });
  });

  it('binds preview, cancel, and restore to their exact backend identifiers', async () => {
    invoke
      .mockResolvedValueOnce(plan)
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce({ outcome: 'restored', issues: [] });

    await tauri.previewLegacyProjectSkillMigration('/tmp/project');
    await tauri.cancelLegacyProjectSkillMigrationPlan(plan.id);
    await tauri.restoreLegacyProjectSkillState('receipt:archive');

    expect(invoke.mock.calls).toEqual([
      ['preview_legacy_project_skill_migration', { projectPath: '/tmp/project' }],
      ['cancel_legacy_project_skill_migration_plan', { planId: plan.id }],
      ['restore_legacy_project_skill_state', { receiptId: 'receipt:archive' }],
    ]);
  });

  it('rejects malformed inventory responses at the IPC boundary', async () => {
    invoke.mockResolvedValueOnce({
      schemaVersion: 1,
      sources: [],
      projects: [{ migrationStatus: 'ready_to_archive' }],
      archives: [],
      diagnostics: [],
    });

    await expect(tauri.inspectLegacySkillState()).rejects.toThrow();
  });
});

import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { LegacySkillMigrationCard } from '@/components/LegacySkillMigrationCard';
import i18n from '@/i18n';
import type {
  LegacyProjectSkill,
  LegacyProjectSkillArchive,
} from '@/lib/legacySkillMigrationTypes';

const {
  inspectLegacySkillState,
  previewLegacyProjectSkillMigration,
  applyLegacyProjectSkillMigrationPlan,
  cancelLegacyProjectSkillMigrationPlan,
  restoreLegacyProjectSkillState,
} = vi.hoisted(() => ({
  inspectLegacySkillState: vi.fn(),
  previewLegacyProjectSkillMigration: vi.fn(),
  applyLegacyProjectSkillMigrationPlan: vi.fn(),
  cancelLegacyProjectSkillMigrationPlan: vi.fn(),
  restoreLegacyProjectSkillState: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    inspectLegacySkillState,
    previewLegacyProjectSkillMigration,
    applyLegacyProjectSkillMigrationPlan,
    cancelLegacyProjectSkillMigrationPlan,
    restoreLegacyProjectSkillState,
  },
}));

const projectPath = '/tmp/project';
const project: LegacyProjectSkill = {
  stateId: 'tmp-project.json',
  stateDigest: 'sha256:state',
  projectPath,
  canonicalProjectPath: projectPath,
  online: true,
  mode: 'allowlist' as const,
  listedSkills: ['legacy/review'],
  missingSourceIds: [],
  links: [
    {
      logicalId: 'review',
      targetKind: 'managed_artifact' as const,
      migrationReceiptId: 'receipt:workspace',
      health: 'ready' as const,
    },
  ],
  health: 'ready' as const,
  migrationStatus: 'ready_to_archive' as const,
};
const archive: LegacyProjectSkillArchive = {
  schemaVersion: 1 as const,
  archiveId: 'legacy-skill-archive:one',
  originalStateId: project.stateId,
  projectPath,
  canonicalProjectPath: projectPath,
  stateDigest: project.stateDigest,
  receiptId: 'receipt:migration',
  archivedAt: '2026-08-01T10:00:00Z',
  status: 'archived' as const,
};
const inventory = (
  projects: LegacyProjectSkill[] = [project],
  archives: LegacyProjectSkillArchive[] = [],
) => ({
  schemaVersion: 1 as const,
  sources: [],
  projects,
  archives,
  diagnostics: [],
});
const plan = {
  schemaVersion: 1 as const,
  id: 'legacy-skill-migration-plan:one',
  projectPath,
  canonicalProjectPath: projectPath,
  stateId: project.stateId,
  stateDigest: project.stateDigest,
  migrationReceiptIds: ['receipt:workspace'],
  confirmationRequired: true,
  riskFingerprint: 'risk:one',
  expiresAt: '2026-08-01T10:05:00Z',
};

describe('LegacySkillMigrationCard', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    inspectLegacySkillState.mockReset().mockResolvedValue(inventory());
    previewLegacyProjectSkillMigration.mockReset().mockResolvedValue(plan);
    applyLegacyProjectSkillMigrationPlan.mockReset().mockResolvedValue({
      outcome: 'archived',
      receipt: { id: archive.receiptId },
      issues: [],
    });
    cancelLegacyProjectSkillMigrationPlan.mockReset().mockResolvedValue(true);
    restoreLegacyProjectSkillState.mockReset().mockResolvedValue({
      outcome: 'restored',
      issues: [],
    });
  });

  it('requires a preview before archiving and offers receipt-bound restore', async () => {
    inspectLegacySkillState
      .mockResolvedValueOnce(inventory())
      .mockResolvedValueOnce(inventory([], [archive]));
    render(<LegacySkillMigrationCard projectPath={projectPath} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Review archive' }));
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('1 committed workspace receipt')).toBeInTheDocument();
    expect(previewLegacyProjectSkillMigration).toHaveBeenCalledWith(projectPath);

    fireEvent.click(screen.getByRole('button', { name: 'Archive state' }));
    await waitFor(() => expect(applyLegacyProjectSkillMigrationPlan).toHaveBeenCalledWith(plan));
    expect(await screen.findByText(/durable receipt/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Restore state' }));
    await waitFor(() =>
      expect(restoreLegacyProjectSkillState).toHaveBeenCalledWith(archive.receiptId),
    );
  });

  it('keeps blocked legacy state read-only', async () => {
    inspectLegacySkillState.mockResolvedValue(
      inventory([{ ...project, migrationStatus: 'blocked', health: 'blocked' }]),
    );
    render(<LegacySkillMigrationCard projectPath={projectPath} />);

    expect(await screen.findByText(/cleanup is blocked/i)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Review archive' })).not.toBeInTheDocument();
    expect(previewLegacyProjectSkillMigration).not.toHaveBeenCalled();
  });

  it('prevents cancelling a plan while its archive request is in flight', async () => {
    let completeApply!: (value: {
      outcome: 'archived';
      receipt: { id: string };
      issues: string[];
    }) => void;
    applyLegacyProjectSkillMigrationPlan.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          completeApply = resolve;
        }),
    );
    render(<LegacySkillMigrationCard projectPath={projectPath} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Review archive' }));
    const cancel = await screen.findByRole('button', { name: 'Cancel' });
    fireEvent.click(screen.getByRole('button', { name: 'Archive state' }));

    await waitFor(() => expect(cancel).toBeDisabled());
    fireEvent.click(cancel);
    expect(cancelLegacyProjectSkillMigrationPlan).not.toHaveBeenCalled();

    await act(async () => {
      completeApply({
        outcome: 'archived',
        receipt: { id: archive.receiptId },
        issues: [],
      });
    });
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  });
});

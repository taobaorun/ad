import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { SkillSourcesSection } from '@/components/SkillSources';
import i18n from '@/i18n';
import { useSkills } from '@/store/skills';

const {
  listSkillCatalog,
  previewAddSkillCatalogSource,
  previewUpdateSkillCatalogSource,
  previewRemoveSkillCatalogSource,
  applySkillCatalogSourcePlan,
  cancelSkillCatalogSourcePlan,
} = vi.hoisted(() => ({
  listSkillCatalog: vi.fn(),
  previewAddSkillCatalogSource: vi.fn(),
  previewUpdateSkillCatalogSource: vi.fn(),
  previewRemoveSkillCatalogSource: vi.fn(),
  applySkillCatalogSourcePlan: vi.fn(),
  cancelSkillCatalogSourcePlan: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    listSkillCatalog,
    previewAddSkillCatalogSource,
    previewUpdateSkillCatalogSource,
    previewRemoveSkillCatalogSource,
    applySkillCatalogSourcePlan,
    cancelSkillCatalogSourcePlan,
  },
}));

const artifact = {
  schemaVersion: 1 as const,
  artifactId: 'skill-artifact:sha256:tree',
  sourceId: 'skill-source:11111111-1111-4111-8111-111111111111',
  sourceRevision: 'git:1234567890abcdef1234567890abcdef12345678',
  treeDigest: 'sha256:tree',
  manifestDigest: 'sha256:manifest',
  skills: [
    {
      logicalId: 'review',
      subpath: 'review',
      instructionDigest: 'sha256:instruction',
    },
  ],
  activationImpact: {
    instructions: ['review/SKILL.md'],
    hooks: [],
    mcp: [],
    commands: [],
    scripts: ['review/scripts/check.sh'],
    binaries: [],
    executablePaths: ['review/scripts/check.sh'],
    digest: 'sha256:impact',
  },
};

const source = {
  sourceId: artifact.sourceId,
  displayName: 'Review tools',
  sourceType: 'git' as const,
  location: 'git@example.com:team/review.git',
  branch: 'main',
  autoUpdate: false,
  currentArtifact: artifact,
  addedAt: '2026-08-01T08:00:00Z',
  updatedAt: '2026-08-01T08:00:00Z',
};

function plan(action: 'add' | 'update' | 'remove') {
  return {
    schemaVersion: 1 as const,
    id: `skill-catalog-plan:${action}`,
    action,
    expectedCatalogRevision: 'sha256:catalog',
    sourceId: source.sourceId,
    displayName: source.displayName,
    artifact: action === 'remove' ? undefined : artifact,
    currentArtifact: action === 'add' ? undefined : artifact,
    confirmationRequired: true,
    riskFingerprint: `risk:${action}`,
    expiresAt: '2026-08-01T09:00:00Z',
  };
}

describe('SkillSourcesSection', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    useSkills.setState({ sources: [], entries: [], plugins: [], projectConfig: null });
    listSkillCatalog.mockReset().mockResolvedValue({
      schemaVersion: 1,
      revision: 'sha256:catalog',
      entries: [source],
    });
    previewAddSkillCatalogSource.mockReset().mockResolvedValue(plan('add'));
    previewUpdateSkillCatalogSource.mockReset().mockResolvedValue(plan('update'));
    previewRemoveSkillCatalogSource.mockReset().mockResolvedValue(plan('remove'));
    applySkillCatalogSourcePlan.mockReset().mockResolvedValue({
      outcome: 'changed',
      source,
      issues: [],
    });
    cancelSkillCatalogSourcePlan.mockReset().mockResolvedValue(true);
  });

  it('previews and confirms an immutable source refresh', async () => {
    render(<SkillSourcesSection />);
    expect(await screen.findByText('Review tools')).toBeInTheDocument();
    expect(screen.getByText(/1 Skills · revision 1234567890ab/)).toBeInTheDocument();
    expect(screen.getByText(/1 executable or runtime entry points/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Update' }));

    expect(await screen.findByText('Confirm Skill source update')).toBeInTheDocument();
    expect(previewUpdateSkillCatalogSource).toHaveBeenCalledWith(source.sourceId);
    expect(screen.getByText(/projects stay pinned to their prior revision/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Confirm & Apply' }));

    await waitFor(() => expect(applySkillCatalogSourcePlan).toHaveBeenCalledWith(plan('update')));
    await waitFor(() =>
      expect(screen.queryByText('Confirm Skill source update')).not.toBeInTheDocument(),
    );
  });

  it('uses backend-issued identity and cancels a staged add plan', async () => {
    render(<SkillSourcesSection />);
    await screen.findByText('Review tools');
    fireEvent.click(screen.getByRole('button', { name: 'Add Source' }));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Team Skills' } });
    fireEvent.change(screen.getByLabelText('Git URL'), {
      target: { value: 'git@example.com:team/skills.git' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Inspect & Preview' }));

    await waitFor(() =>
      expect(previewAddSkillCatalogSource).toHaveBeenCalledWith({
        displayName: 'Team Skills',
        sourceType: 'git',
        location: 'git@example.com:team/skills.git',
        branch: undefined,
        subdirectory: undefined,
        autoUpdate: false,
      }),
    );
    expect(await screen.findByText('Confirm Skill source addition')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() =>
      expect(cancelSkillCatalogSourcePlan).toHaveBeenCalledWith('skill-catalog-plan:add'),
    );
    expect(applySkillCatalogSourcePlan).not.toHaveBeenCalled();
  });

  it('explains that catalog removal preserves pinned artifacts', async () => {
    render(<SkillSourcesSection />);
    await screen.findByText('Review tools');
    fireEvent.click(screen.getByRole('button', { name: 'Remove source: Review tools' }));

    expect(await screen.findByText('Confirm Skill source removal')).toBeInTheDocument();
    expect(screen.getByText(/does not delete old artifacts/i)).toBeInTheDocument();
    expect(previewRemoveSkillCatalogSource).toHaveBeenCalledWith(source.sourceId);
  });
});

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
  previewRollbackSkillCatalogSourceUpdate,
  applySkillCatalogSourcePlan,
  cancelSkillCatalogSourcePlan,
} = vi.hoisted(() => ({
  listSkillCatalog: vi.fn(),
  previewAddSkillCatalogSource: vi.fn(),
  previewUpdateSkillCatalogSource: vi.fn(),
  previewRemoveSkillCatalogSource: vi.fn(),
  previewRollbackSkillCatalogSourceUpdate: vi.fn(),
  applySkillCatalogSourcePlan: vi.fn(),
  cancelSkillCatalogSourcePlan: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    listSkillCatalog,
    previewAddSkillCatalogSource,
    previewUpdateSkillCatalogSource,
    previewRemoveSkillCatalogSource,
    previewRollbackSkillCatalogSourceUpdate,
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

const binding = {
  schemaVersion: 2 as const,
  bindingId: 'skill-source-binding:1111111111111111',
  sourceId: source.sourceId,
  sourceType: 'git' as const,
  sourceRevision: 'git:2234567890abcdef1234567890abcdef12345678',
  stableRoot: '/tmp/.ad/skill-library/source/current',
  physicalRoot: '/tmp/.ad/skill-library/source/generations/second',
  treeDigest: 'sha256:binding-tree',
  manifestDigest: 'sha256:binding-manifest',
  skills: artifact.skills,
  activationImpact: artifact.activationImpact,
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
    applicability: 'applicable' as const,
    blockingIssues: [],
    affectedResources: [],
    affectedWorkspaces: [],
    confirmationRequired: true,
    riskFingerprint: `risk:${action}`,
    expiresAt: '2026-08-01T09:00:00Z',
  };
}

describe('SkillSourcesSection', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    useSkills.setState({ sources: [] });
    listSkillCatalog.mockReset().mockResolvedValue({
      schemaVersion: 1,
      revision: 'sha256:catalog',
      entries: [source],
    });
    previewAddSkillCatalogSource.mockReset().mockResolvedValue(plan('add'));
    previewUpdateSkillCatalogSource.mockReset().mockResolvedValue(plan('update'));
    previewRemoveSkillCatalogSource.mockReset().mockResolvedValue(plan('remove'));
    previewRollbackSkillCatalogSourceUpdate.mockReset().mockResolvedValue({
      ...plan('update'),
      rollbackOf: 'skill-catalog-receipt:update',
      artifact: undefined,
      currentArtifact: undefined,
      binding: { ...binding, sourceRevision: 'git:1234567890abcdef1234567890abcdef12345678' },
      currentBinding: binding,
    });
    applySkillCatalogSourcePlan.mockReset().mockResolvedValue({
      outcome: 'changed',
      source,
      issues: [],
    });
    cancelSkillCatalogSourcePlan.mockReset().mockResolvedValue(true);
  });

  it('previews and confirms a shared source refresh', async () => {
    render(<SkillSourcesSection />);
    expect(await screen.findByText('Review tools')).toBeInTheDocument();
    expect(screen.getByText(/1 Skills · revision 1234567890ab/)).toBeInTheDocument();
    expect(screen.getByText(/1 executable or runtime entry points/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Update' }));

    expect(await screen.findByText('Confirm Skill source update')).toBeInTheDocument();
    expect(previewUpdateSkillCatalogSource).toHaveBeenCalledWith(source.sourceId);
    expect(screen.getByText(/Update this source and its installed resources/i)).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));

    await waitFor(() =>
      expect(previewAddSkillCatalogSource).toHaveBeenCalledWith(
        {
          displayName: 'Team Skills',
          sourceType: 'git',
          location: 'git@example.com:team/skills.git',
          branch: undefined,
          subdirectory: undefined,
          autoUpdate: false,
        },
        expect.any(Function),
      ),
    );
    expect(await screen.findByText('Confirm Skill source addition')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() =>
      expect(cancelSkillCatalogSourcePlan).toHaveBeenCalledWith('skill-catalog-plan:add'),
    );
    expect(applySkillCatalogSourcePlan).not.toHaveBeenCalled();
  });

  it('offers a confirmed shared rollback from a Git update receipt', async () => {
    applySkillCatalogSourcePlan.mockResolvedValueOnce({
      outcome: 'changed',
      receipt: {
        schemaVersion: 2,
        id: 'skill-catalog-receipt:update',
        planId: 'skill-catalog-plan:update',
        action: 'update',
        sourceId: source.sourceId,
        beforeCatalogRevision: 'sha256:before',
        afterCatalogRevision: 'sha256:after',
        binding,
        previousBinding: {
          ...binding,
          sourceRevision: 'git:1234567890abcdef1234567890abcdef12345678',
          physicalRoot: '/tmp/.ad/skill-library/source/generations/first',
        },
        affectedResources: [],
        affectedWorkspaces: [],
        status: 'complete',
        createdAt: '2026-08-05T08:30:00Z',
      },
      issues: [],
    });
    render(<SkillSourcesSection />);
    await screen.findByText('Review tools');
    fireEvent.click(screen.getByRole('button', { name: 'Update' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Confirm & Apply' }));

    fireEvent.click(await screen.findByRole('button', { name: 'Rollback' }));

    expect(previewRollbackSkillCatalogSourceUpdate).toHaveBeenCalledWith(
      'skill-catalog-receipt:update',
    );
    expect(await screen.findByText('Confirm Skill source rollback')).toBeInTheDocument();
  });

  it('explains that catalog removal requires references to be removed', async () => {
    render(<SkillSourcesSection />);
    await screen.findByText('Review tools');
    fireEvent.click(screen.getByRole('button', { name: 'Remove source: Review tools' }));

    expect(await screen.findByText('Confirm Skill source removal')).toBeInTheDocument();
    expect(screen.getByText(/Uninstall resources from this source first/i)).toBeInTheDocument();
    expect(previewRemoveSkillCatalogSource).toHaveBeenCalledWith(source.sourceId);
  });

  it('blocks source removal while AD-owned project links still reference it', async () => {
    previewRemoveSkillCatalogSource.mockResolvedValueOnce({
      ...plan('remove'),
      applicability: 'blocked' as const,
      blockingIssues: [
        {
          code: 'source_has_installed_references',
          message: 'Remove the Skill from linked projects before removing this source.',
        },
      ],
      affectedResources: ['skill-source:11111111-1111-4111-8111-111111111111/review'],
      affectedWorkspaces: ['workspace:sha256:project'],
    });
    render(<SkillSourcesSection />);
    await screen.findByText('Review tools');
    fireEvent.click(screen.getByRole('button', { name: 'Remove source: Review tools' }));

    expect(await screen.findByText('This operation is blocked')).toBeInTheDocument();
    expect(
      screen.getByText('Remove the Skill from linked projects before removing this source.'),
    ).toBeInTheDocument();
    expect(
      screen.getByText('This source is linked from 1 project workspaces.'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Confirm & Apply' })).toBeDisabled();
  });
});

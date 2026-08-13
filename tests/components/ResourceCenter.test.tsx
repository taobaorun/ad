import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ResourceCenter } from '@/components/ResourceCenter';
import i18n from '@/i18n';

const mocks = vi.hoisted(() => ({
  listResourceCatalog: vi.fn(),
  previewRemoveCatalogResource: vi.fn(),
  applyRemoveCatalogResource: vi.fn(),
  previewAddSkillCatalogSource: vi.fn(),
  applySkillCatalogSourcePlan: vi.fn(),
  readdCatalogResource: vi.fn(),
  listResourceRemovalOperations: vi.fn(),
  retryRemoveCatalogResource: vi.fn(),
  previewUpdateSkillCatalogSource: vi.fn(),
  previewRemoveCatalogSource: vi.fn(),
  applyRemoveCatalogSource: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({ tauri: mocks }));

const sourceId = 'skill-source:11111111-1111-4111-8111-111111111111';
const skillId = 'catalog-resource:sha256:skill';
const pluginId = 'catalog-resource:sha256:plugin';
const catalog = {
  schemaVersion: 1,
  revision: 2,
  migratedFromSkillCatalog: false,
  sources: {
    [sourceId]: {
      id: sourceId,
      displayName: 'Team tools',
      sourceType: 'git',
      location: 'https://example.com/team/tools.git',
      health: 'healthy',
    },
  },
  resources: {
    [skillId]: {
      id: skillId,
      sourceId,
      kind: 'skills',
      installId: 'review',
      displayName: 'Review',
      subpath: 'review',
      descriptorDigest: 'sha256:skill',
      compatibleAgents: ['claude-code', 'codex'],
      present: true,
      lifecycle: 'managed',
    },
    [pluginId]: {
      id: pluginId,
      sourceId,
      kind: 'plugins',
      installId: 'toolbox',
      displayName: 'Toolbox',
      description: 'Native project tools',
      subpath: 'toolbox',
      descriptorDigest: 'sha256:plugin',
      compatibleAgents: ['claude-code'],
      present: true,
      lifecycle: 'managed',
    },
  },
};

const removalPlan = {
  planId: 'remove-plan',
  resourceId: skillId,
  resourceName: 'Review',
  expectedCatalogRevision: 2,
  affectedProjectCount: 2,
  affectedAgentCount: 2,
  installations: [
    {
      installationId: 'installation:a',
      workspaceKey: 'workspace:a',
      agentId: 'codex',
      projectPath: '/work/a',
      state: 'pending',
    },
    {
      installationId: 'installation:b',
      workspaceKey: 'workspace:b',
      agentId: 'claude-code',
      projectPath: '/work/b',
      state: 'pending',
    },
  ],
  riskFingerprint: 'risk:remove',
  expiresAt: '2026-08-13T12:00:00Z',
};

describe('ResourceCenter', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    Object.values(mocks).forEach((mock) => mock.mockReset());
    mocks.listResourceCatalog.mockResolvedValue(catalog);
    mocks.listResourceRemovalOperations.mockResolvedValue([]);
    mocks.previewRemoveCatalogResource.mockResolvedValue(removalPlan);
    mocks.applyRemoveCatalogResource.mockImplementation(async (_plan, onProgress) => {
      onProgress({
        operationId: 'operation:remove',
        sequence: 1,
        phase: 'uninstalling',
        completed: 1,
        total: 2,
        item: { ...removalPlan.installations[0], state: 'succeeded' },
      });
      return {
        operationId: 'operation:remove',
        resourceId: skillId,
        phase: 'complete',
        completed: 2,
        total: 2,
        installations: removalPlan.installations,
      };
    });
  });

  it('distinguishes source and kind while filtering without exposing source paths as primary copy', async () => {
    render(<ResourceCenter />);
    expect(await screen.findByRole('heading', { name: 'Resource Center' })).toBeInTheDocument();
    expect(screen.getByText('Review')).toBeInTheDocument();
    expect(screen.getByText('Toolbox')).toBeInTheDocument();
    expect(screen.getAllByText('Team tools')).toHaveLength(3);
    expect(screen.getAllByText('Claude Code')).toHaveLength(2);
    expect(screen.getByText('Codex')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Plugins' }));
    expect(screen.queryByText('Review')).not.toBeInTheDocument();
    expect(screen.getByText('Toolbox')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('Search resources or sources…'), {
      target: { value: 'missing' },
    });
    expect(screen.getByText('No matching resources')).toBeInTheDocument();
  });

  it('previews impact and applies removal with progress', async () => {
    render(<ResourceCenter />);
    await screen.findByText('Review');
    fireEvent.click(screen.getByRole('button', { name: 'Remove resource Review' }));

    expect(await screen.findByRole('heading', { name: 'Remove “Review”' })).toBeInTheDocument();
    expect(screen.getAllByText('2')).toHaveLength(2);
    fireEvent.click(screen.getByRole('button', { name: 'Remove resource' }));

    await waitFor(() => expect(mocks.applyRemoveCatalogResource).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.queryByRole('heading', { name: 'Remove “Review”' })).not.toBeInTheDocument(),
    );
    expect(mocks.listResourceCatalog).toHaveBeenCalledTimes(2);
  });

  it('keeps removed resources in a secondary section and requires an explicit re-add', async () => {
    const removed = {
      ...catalog,
      resources: {
        ...catalog.resources,
        [skillId]: { ...catalog.resources[skillId], lifecycle: 'suppressed' },
      },
    };
    mocks.listResourceCatalog.mockResolvedValue(removed);
    mocks.readdCatalogResource.mockResolvedValue(catalog);

    render(<ResourceCenter />);
    fireEvent.click(await screen.findByText('Removed resources (1)'));
    expect(screen.getByText('Removed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Re-add resource Review' }));

    await waitFor(() => expect(mocks.readdCatalogResource).toHaveBeenCalledWith(skillId));
    expect(screen.queryByText('Removed')).not.toBeInTheDocument();
  });

  it('shows source lifecycle actions and previews aggregate removal impact', async () => {
    const sourceRemoval = {
      planId: 'source-remove-plan',
      sourceId,
      sourceName: 'Team tools',
      expectedCatalogRevision: 2,
      affectedProjectCount: 2,
      affectedAgentCount: 2,
      resources: [
        {
          resourceId: skillId,
          resourceName: 'Review',
          kind: 'skills',
          affectedProjectCount: 2,
          affectedAgentCount: 2,
          state: 'pending',
        },
        {
          resourceId: pluginId,
          resourceName: 'Toolbox',
          kind: 'plugins',
          affectedProjectCount: 0,
          affectedAgentCount: 0,
          state: 'pending',
        },
      ],
      riskFingerprint: 'risk:source-remove',
      expiresAt: '2026-08-13T12:00:00Z',
    };
    mocks.previewRemoveCatalogSource.mockResolvedValue(sourceRemoval);

    render(<ResourceCenter />);
    await screen.findByText('Review');
    fireEvent.click(screen.getByRole('button', { name: 'Remove source Team tools' }));

    expect(
      await screen.findByRole('heading', { name: 'Remove source “Team tools”' }),
    ).toBeInTheDocument();
    expect(mocks.previewRemoveCatalogSource).toHaveBeenCalledWith(sourceId);
    expect(screen.getByText('Source resources')).toBeInTheDocument();
    expect(screen.getAllByText('2').length).toBeGreaterThanOrEqual(3);
  });
});

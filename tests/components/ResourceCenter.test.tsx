import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
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

  it('shows only sources while keeping source-kind filtering', async () => {
    render(<ResourceCenter />);
    expect(await screen.findByRole('heading', { name: 'Harness', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Skills & Plugins', level: 2 })).toBeInTheDocument();
    expect(screen.getByRole('navigation', { name: 'Harness capabilities' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Skills & Plugins' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('link', { name: 'MCP Coming soon' })).toHaveAttribute(
      'aria-disabled',
      'true',
    );
    expect(screen.getByText('Coming soon')).toBeInTheDocument();
    expect(screen.getByText('Team tools')).toBeInTheDocument();
    expect(screen.getByText('https://example.com/team/tools.git')).toBeInTheDocument();
    expect(screen.queryByText('Review')).not.toBeInTheDocument();
    expect(screen.queryByText('Toolbox')).not.toBeInTheDocument();
    expect(screen.queryByText('Claude Code')).not.toBeInTheDocument();
    expect(screen.queryByText('Codex')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Plugins' }));
    expect(screen.getByText('Team tools')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('Search sources…'), {
      target: { value: 'missing' },
    });
    expect(screen.getByText('No matching sources')).toBeInTheDocument();
  });

  it('updates a source directly from its card', async () => {
    render(<ResourceCenter />);
    await screen.findByText('Team tools');
    fireEvent.click(screen.getByRole('button', { name: 'Update source Team tools' }));
    await waitFor(() =>
      expect(mocks.previewUpdateSkillCatalogSource).toHaveBeenCalledWith(sourceId),
    );
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
    await screen.findByText('Team tools');
    fireEvent.click(screen.getByRole('button', { name: 'Remove source Team tools' }));

    expect(
      await screen.findByRole('heading', { name: 'Remove source “Team tools”' }),
    ).toBeInTheDocument();
    expect(mocks.previewRemoveCatalogSource).toHaveBeenCalledWith(sourceId);
    expect(screen.getByText('Source resources')).toBeInTheDocument();
    expect(screen.getAllByText('2').length).toBeGreaterThanOrEqual(3);
  });

  it('redirects a duplicate add to the existing source card', async () => {
    render(<ResourceCenter />);
    await screen.findByText('Team tools');
    fireEvent.click(screen.getByRole('button', { name: 'Add source' }));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Team tools' } });
    fireEvent.change(screen.getByLabelText('Git URL'), {
      target: { value: 'https://example.com/team/tools.git' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Inspect & Preview' }));

    expect(
      await screen.findByText(
        '“Team tools” is already managed. Update or remove the existing source here.',
      ),
    ).toBeInTheDocument();
    expect(mocks.previewAddSkillCatalogSource).not.toHaveBeenCalled();
    expect(screen.getByDisplayValue('Team tools')).toBeInTheDocument();
  });

  it('shows source clone progress and keeps failures in the dialog for retry', async () => {
    let rejectPreview!: (reason?: unknown) => void;
    const previewResult = new Promise((_resolve, reject) => {
      rejectPreview = reject;
    });
    mocks.previewAddSkillCatalogSource
      .mockImplementationOnce((_request, onProgress) => {
        onProgress({ sequence: 2, phase: 'cloning' });
        return previewResult;
      })
      .mockImplementationOnce(() => new Promise(() => {}));

    render(<ResourceCenter />);
    await screen.findByText('Team tools');
    fireEvent.click(screen.getByRole('button', { name: 'Add source' }));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'New tools' } });
    fireEvent.change(screen.getByLabelText('Git URL'), {
      target: { value: 'https://example.com/new/tools.git' },
    });
    const preview = screen.getByRole('button', { name: 'Inspect & Preview' });
    fireEvent.click(preview);
    fireEvent.click(preview);

    expect(await screen.findByText('Cloning repository…')).toBeInTheDocument();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(mocks.previewAddSkillCatalogSource).toHaveBeenCalledOnce();
    await act(async () => rejectPreview(new Error('Repository unavailable')));
    expect(await screen.findByRole('alert')).toHaveTextContent('Repository unavailable');
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(mocks.previewAddSkillCatalogSource).toHaveBeenCalledTimes(2);
  });
});

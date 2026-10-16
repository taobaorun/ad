import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ResourceCenter } from '@/components/ResourceCenter';
import i18n from '@/i18n';
import { AgentContextSchema, AgentInstallationSchema, AgentMetadataSchema } from '@/lib/agentTypes';
import { useAgents } from '@/store/agents';

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
  agentCollectionPanel: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({ tauri: mocks }));
vi.mock('@/components/AgentCollectionPanel', () => ({
  AgentCollectionPanel: (props: unknown) => mocks.agentCollectionPanel(props),
}));

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

const sourceRemovalPlan = {
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
  expiresAt: '2026-08-31T09:00:00Z',
};

function sourcePlan(action: 'add' | 'update') {
  return {
    schemaVersion: 1,
    id: `skill-catalog-plan:${action}`,
    action,
    expectedCatalogRevision: 'sha256:catalog',
    sourceId,
    displayName: action === 'add' ? 'New tools' : 'Team tools',
    applicability: 'applicable',
    blockingIssues: [],
    warnings: [],
    affectedResources: [],
    affectedWorkspaces: [],
    confirmationRequired: true,
    riskFingerprint: `risk:${action}`,
    expiresAt: '2026-08-31T09:00:00Z',
  };
}

describe('ResourceCenter', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    Object.values(mocks).forEach((mock) => mock.mockReset());
    mocks.agentCollectionPanel.mockImplementation(() => <div>User resource inventory</div>);
    const agents = AgentMetadataSchema.array().parse([
      { id: 'codex', displayName: 'Codex', capabilities: [] },
      { id: 'claude-code', displayName: 'Claude Code', capabilities: [] },
    ]);
    const installations = AgentInstallationSchema.array().parse([
      { id: 'codex:base', agentId: 'codex', rootPath: '/Users/test/.codex' },
      {
        id: 'claude-code:base',
        agentId: 'claude-code',
        rootPath: '/Users/test/.claude',
      },
    ]);
    useAgents.setState({
      agents,
      installations,
      activeContext: AgentContextSchema.parse({ installationId: 'codex:base' }),
      activeAgentId: 'codex',
      capabilitiesByAgent: { codex: [], 'claude-code': [] },
      activeCapabilities: [],
      loading: false,
    });
    mocks.listResourceCatalog.mockResolvedValue(catalog);
    mocks.listResourceRemovalOperations.mockResolvedValue([]);
    mocks.previewRemoveCatalogResource.mockResolvedValue(removalPlan);
    mocks.previewAddSkillCatalogSource.mockResolvedValue(sourcePlan('add'));
    mocks.previewUpdateSkillCatalogSource.mockResolvedValue(sourcePlan('update'));
    mocks.previewRemoveCatalogSource.mockResolvedValue(sourceRemovalPlan);
    mocks.applySkillCatalogSourcePlan.mockResolvedValue({
      outcome: 'changed',
      issues: [],
    });
    mocks.applyRemoveCatalogSource.mockResolvedValue({
      operationId: 'operation:source-remove',
      sourceId,
      phase: 'complete',
      completed: 2,
      total: 2,
      resources: sourceRemovalPlan.resources,
    });
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

  it('opens source-scoped user installation from the existing source card', async () => {
    render(<ResourceCenter />);

    await screen.findByText('Team tools');
    expect(
      screen.queryByRole('heading', { name: 'User-level installation' }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText('User resource inventory')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Install' }));

    expect(
      await screen.findByRole('heading', { name: 'Install from “Team tools”' }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Choose a target Agent/)).toBeInTheDocument();
    expect(screen.getByText('User resource inventory')).toBeInTheDocument();
    expect(mocks.agentCollectionPanel.mock.calls[0]?.[0]).toEqual(
      expect.objectContaining({
        scope: 'user',
        context: { installationId: 'codex:base' },
        sourceFilter: expect.objectContaining({
          kind: 'catalog_git',
          displayName: 'Team tools',
          location: 'https://example.com/team/tools.git',
        }),
      }),
    );

    const agentSelect = screen.getByRole('combobox', { name: 'Target Agent' });
    expect(agentSelect).toHaveValue('codex:base');
    fireEvent.change(agentSelect, { target: { value: 'claude-code:base' } });
    await waitFor(() =>
      expect(mocks.agentCollectionPanel.mock.calls.at(-1)?.[0]).toEqual(
        expect.objectContaining({
          scope: 'user',
          context: { installationId: 'claude-code:base' },
        }),
      ),
    );
  });

  it('shows declared resource versions beside their names and respects the kind filter', async () => {
    mocks.listResourceCatalog.mockResolvedValue({
      ...catalog,
      resources: {
        [skillId]: { ...catalog.resources[skillId], version: '1.8.5' },
        [pluginId]: { ...catalog.resources[pluginId], version: '2.0.0-beta.1' },
      },
    });
    render(<ResourceCenter />);

    expect(await screen.findByText('1.8.5')).toBeInTheDocument();
    expect(screen.getByText('1.8.5').closest('li')).toHaveTextContent('Review');
    expect(screen.getByText('2.0.0-beta.1').closest('li')).toHaveTextContent('Toolbox');
    expect(screen.getByLabelText('Version 1.8.5')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Plugins' }));
    expect(screen.queryByText('1.8.5')).not.toBeInTheDocument();
    expect(screen.getByText('2.0.0-beta.1')).toBeInTheDocument();
  });

  it('replaces changed versions and removes the label when a declaration disappears on refresh', async () => {
    const versionedCatalog = (version: string) => ({
      ...catalog,
      resources: {
        ...catalog.resources,
        [skillId]: { ...catalog.resources[skillId], version },
      },
    });
    mocks.listResourceCatalog
      .mockResolvedValueOnce(versionedCatalog('1.0.0'))
      .mockResolvedValueOnce(versionedCatalog('2.0.0'))
      .mockResolvedValue(catalog);
    render(<ResourceCenter />);
    expect(await screen.findByText('1.0.0')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByText('2.0.0')).toBeInTheDocument();
    expect(screen.queryByText('1.0.0')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(screen.queryByText('2.0.0')).not.toBeInTheDocument());
    expect(screen.queryByRole('list', { name: 'Resource versions' })).not.toBeInTheDocument();
    expect(screen.getByText('Team tools')).toBeInTheDocument();
  });

  it('omits versions for unversioned, removed and missing resources', async () => {
    mocks.listResourceCatalog.mockResolvedValue({
      ...catalog,
      resources: {
        [skillId]: { ...catalog.resources[skillId], version: '1.0.0', present: false },
        [pluginId]: { ...catalog.resources[pluginId], version: '2.0.0', lifecycle: 'suppressed' },
        unversioned: { ...catalog.resources[skillId], id: 'unversioned', version: ' ' },
      },
    });
    render(<ResourceCenter />);
    await screen.findByText('Team tools');
    expect(screen.queryByRole('list', { name: 'Resource versions' })).not.toBeInTheDocument();
    expect(screen.queryByText('1.0.0')).not.toBeInTheDocument();
    expect(screen.queryByText('2.0.0')).not.toBeInTheDocument();
  });

  it('shows sources while keeping source-kind filtering', async () => {
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

  it('shows a concise missing-installation count on the source card', async () => {
    mocks.listResourceCatalog.mockResolvedValue({
      ...catalog,
      missingInstallationsBySource: { [sourceId]: 3 },
    });
    render(<ResourceCenter />);

    expect(await screen.findByText('3 installed resources missing')).toBeInTheDocument();
  });

  it('updates a source directly from its card', async () => {
    render(<ResourceCenter />);
    await screen.findByText('Team tools');
    fireEvent.click(screen.getByRole('button', { name: 'Update source Team tools' }));
    await waitFor(() =>
      expect(mocks.previewUpdateSkillCatalogSource).toHaveBeenCalledWith(sourceId),
    );
  });

  it('shows one concise warning when an update will leave installations missing', async () => {
    mocks.previewUpdateSkillCatalogSource.mockResolvedValue({
      ...sourcePlan('update'),
      warnings: [{ code: 'source_update_missing_installations', count: 2 }],
    });
    render(<ResourceCenter />);
    await screen.findByText('Team tools');
    fireEvent.click(screen.getByRole('button', { name: 'Update source Team tools' }));

    expect(
      await screen.findByText('2 installed resources will be missing from the source.'),
    ).toBeInTheDocument();
  });

  it('shows source lifecycle actions and previews aggregate removal impact', async () => {
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
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));

    expect(
      await screen.findByText(
        '“Team tools” is already managed. Update or remove the existing source here.',
      ),
    ).toBeInTheDocument();
    expect(mocks.previewAddSkillCatalogSource).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText('Search sources…')).toHaveValue('Team tools');
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
    const preview = screen.getByRole('button', { name: 'Continue' });
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

  it.each([
    ['compensated', 'The source operation did not finish. The previous state was restored.'],
    ['partial_failure', 'Some source changes did not finish.'],
  ])(
    'keeps a %s source outcome visible after refresh without reusing its plan',
    async (outcome, message) => {
      mocks.applySkillCatalogSourcePlan.mockResolvedValue({
        outcome,
        issues: ['operation did not finish'],
      });
      render(<ResourceCenter />);
      fireEvent.click(await screen.findByRole('button', { name: 'Update source Team tools' }));
      fireEvent.click(await screen.findByRole('button', { name: 'Confirm & Apply' }));
      await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent(message));
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(mocks.listResourceCatalog).toHaveBeenCalledTimes(2);
      expect(mocks.applySkillCatalogSourcePlan).toHaveBeenCalledTimes(1);
      fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
      await waitFor(() => expect(mocks.listResourceCatalog).toHaveBeenCalledTimes(3));
      expect(mocks.applySkillCatalogSourcePlan).toHaveBeenCalledTimes(1);
    },
  );
});

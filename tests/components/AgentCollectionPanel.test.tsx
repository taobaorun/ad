import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentCollectionPanel } from '@/components/AgentCollectionPanel';
import i18n from '@/i18n';
import { ProjectWorkspaceInventorySchema } from '@/lib/agentResourceInventoryTypes';
import { AgentContextSchema, CapabilityDescriptorSchema } from '@/lib/agentTypes';

const {
  inspectProjectAgentWorkspace,
  previewProjectCollectionAction,
  applyProjectCollectionAction,
} = vi.hoisted(() => ({
  inspectProjectAgentWorkspace: vi.fn(),
  previewProjectCollectionAction: vi.fn(),
  applyProjectCollectionAction: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    inspectProjectAgentWorkspace,
    previewProjectCollectionAction,
    applyProjectCollectionAction,
  },
}));

const context = AgentContextSchema.parse({
  installationId: 'codex:default',
  projectPath: '/Users/test/project',
});
const capabilities = CapabilityDescriptorSchema.array().parse([
  {
    kind: 'skills',
    scopes: ['user', 'project'],
    operations: ['list'],
    availability: 'available',
    limitations: [],
  },
  {
    kind: 'plugins',
    scopes: ['user', 'project'],
    operations: ['list'],
    availability: 'degraded',
    limitations: [
      {
        code: 'marketplace',
        messageKey: 'agents.capabilities.codexPluginInstallRequiresMarketplace',
      },
    ],
  },
]);

function inventory(model = 'current', key = 'workspace:sha256:project') {
  const coverage = {
    status: 'partial',
    observed: 1,
    visible: 1,
    diagnostics: [
      {
        code: 'agent_version_unverified',
        messageKey: 'agents.inventory.agentVersionUnverified',
      },
    ],
  } as const;
  return ProjectWorkspaceInventorySchema.parse({
    schemaVersion: 1,
    workspace: {
      schemaVersion: 1,
      key,
      revision: `workspace-revision:sha256:${model}`,
      agentId: 'codex',
      canonicalProjectPath: '/Users/test/project',
      baseInstallationId: 'codex:default',
      effectiveInstallationId: 'codex:default',
    },
    revision: `inventory-revision:sha256:${model}`,
    discovery: {
      adapterVersion: 1,
      locationSet: 'codex-project-v1',
      schemaVersions: ['codex-config-toml-v1'],
      verifiedAgentVersions: [],
      compatibility: 'unverified',
    },
    settings: {
      workspaceKey: key,
      coverage: { ...coverage, observed: 0, visible: 0 },
      effectiveContent: {},
      fields: [],
      layers: [],
      editableTargets: [],
    },
    skills: {
      workspaceKey: key,
      agentId: 'codex',
      kind: 'skills',
      coverage,
      resources: [
        {
          key: `resource:sha256:review-${model}`,
          kind: 'skills',
          logicalId: `review-${model}`,
          displayName: `Review ${model}`,
          description: 'Reviews the active change',
          effectiveState: 'enabled',
          provenance: {
            declarations: [
              {
                key: 'declaration:sha256:review',
                layer: 'project',
                sourceId: 'catalog:team',
                targetId: 'target:sha256:review',
                scope: 'project',
              },
            ],
            winner: 'declaration:sha256:review',
          },
          ownership: { kind: 'external' },
          health: { status: 'healthy' },
          management: {
            status: 'read_only',
            actions: [{ action: 'inspect', availability: 'available' }],
          },
        },
      ],
    },
    plugins: {
      workspaceKey: key,
      agentId: 'codex',
      kind: 'plugins',
      coverage,
      resources: [
        {
          key: `resource:sha256:plugin-${model}`,
          kind: 'plugins',
          logicalId: `demo@market-${model}`,
          displayName: `Demo ${model}`,
          effectiveState: 'conflict',
          provenance: {
            declarations: [
              {
                key: 'declaration:sha256:plugin-user',
                layer: 'user',
                sourceId: 'agent-plugin:demo',
                targetId: 'target:sha256:plugin-user',
                scope: 'user',
              },
              {
                key: 'declaration:sha256:plugin-project',
                layer: 'runtime',
                sourceId: 'agent-plugin:demo',
                targetId: 'target:sha256:plugin-project',
                scope: 'project',
              },
            ],
            winner: 'declaration:sha256:plugin-project',
          },
          ownership: { kind: 'unknown' },
          health: {
            status: 'degraded',
            diagnostic: {
              code: 'plugin_conflict',
              messageKey: 'agents.inventory.pluginConflict',
            },
          },
          management: { status: 'read_only', actions: [] },
        },
      ],
    },
    diagnostics: [],
  });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function actionableInventory() {
  const next = inventory('available');
  const resource = next.skills.resources[0]!;
  resource.effectiveState = 'unconfigured';
  resource.provenance.declarations = [];
  delete resource.provenance.winner;
  resource.ownership = { kind: 'ad_managed' };
  resource.management = {
    status: 'managed',
    actions: [
      { action: 'inspect', availability: 'available' },
      { action: 'install', availability: 'confirmation_required' },
      {
        action: 'remove',
        availability: 'unavailable',
        limitation: {
          code: 'skill_not_installed',
          messageKey: 'agents.resources.skillNotInstalled',
        },
      },
    ],
  };
  return next;
}

function actionPreview() {
  const current = actionableInventory();
  const resource = current.skills.resources[0]!;
  return {
    workspaceKey: current.workspace.key,
    resourceKey: resource.key,
    action: 'install' as const,
    plan: {
      id: 'plan:install-review',
      agentId: 'codex',
      context,
      changes: [
        {
          resource: {
            installationId: context.installationId,
            projectPath: context.projectPath,
            kind: 'skills' as const,
            scope: 'project' as const,
            logicalId: resource.logicalId,
          },
          kind: 'create' as const,
          target: {
            id: 'target:sha256:review-install',
            kind: 'agent_resource' as const,
            display: 'Project Skill review',
          },
          scope: 'project' as const,
          dependencies: [],
          activationImpact: [],
        },
      ],
      requiredAcknowledgements: [],
      riskFingerprint: 'risk:sha256:install-review',
      expiresAt: '2026-08-01T09:00:00Z',
    },
  };
}

describe('AgentCollectionPanel', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    inspectProjectAgentWorkspace.mockReset().mockResolvedValue(inventory());
    previewProjectCollectionAction.mockReset();
    applyProjectCollectionAction.mockReset();
  });

  it('renders backend-owned effective state, provenance, health, and partial coverage', async () => {
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    expect(await screen.findByText('Review current')).toBeInTheDocument();
    expect(screen.getByText('Demo current')).toBeInTheDocument();
    expect(screen.getByText('Conflict')).toBeInTheDocument();
    expect(screen.getAllByText(/Read-only/)).toHaveLength(2);
    expect(screen.getByLabelText('Degraded resource')).toBeInTheDocument();
    expect(screen.getAllByText(/partial inventory/i)).toHaveLength(2);
    expect(screen.queryByRole('switch')).not.toBeInTheDocument();
    expect(screen.queryByText('/Users/test/project')).not.toBeInTheDocument();
  });

  it('filters typed Skills and Plugins without inspecting raw snapshot content', async () => {
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);
    await screen.findByText('Review current');

    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'demo' } });
    expect(screen.queryByText('Review current')).not.toBeInTheDocument();
    expect(screen.getByText('Demo current')).toBeInTheDocument();
  });

  it('ignores inventory responses from a superseded Agent context', async () => {
    const first = deferred<ReturnType<typeof inventory>>();
    const second = deferred<ReturnType<typeof inventory>>();
    inspectProjectAgentWorkspace
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const nextContext = AgentContextSchema.parse({
      installationId: 'codex:next',
      projectPath: '/Users/test/project',
    });

    const { rerender } = render(
      <AgentCollectionPanel context={context} capabilities={capabilities} />,
    );
    rerender(<AgentCollectionPanel context={nextContext} capabilities={capabilities} />);
    second.resolve(inventory('next', 'workspace:sha256:next'));
    expect(await screen.findByText('Review next')).toBeInTheDocument();
    await act(async () => first.resolve(inventory('stale')));
    expect(screen.queryByText('Review stale')).not.toBeInTheDocument();
  });

  it('shows command failures without fabricating an empty complete inventory', async () => {
    inspectProjectAgentWorkspace.mockRejectedValueOnce(new Error('Inventory failed'));
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    expect(await screen.findByRole('alert')).toHaveTextContent('Inventory failed');
  });

  it('previews and applies only backend-offered project actions', async () => {
    const available = actionableInventory();
    inspectProjectAgentWorkspace.mockResolvedValue(available);
    previewProjectCollectionAction.mockResolvedValue(actionPreview());
    applyProjectCollectionAction.mockResolvedValue({
      workspaceKey: available.workspace.key,
      outcome: 'changed',
      issues: [],
    });
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Install: Review available' }));
    expect(previewProjectCollectionAction).toHaveBeenCalledWith(
      context.installationId,
      context.projectPath,
      {
        workspaceKey: available.workspace.key,
        inventoryRevision: available.revision,
        resourceKey: available.skills.resources[0]!.key,
        action: 'install',
      },
    );
    expect(await screen.findByText('Review changes')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove: Review available' })).toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    expect(applyProjectCollectionAction).toHaveBeenCalledWith(
      'plan:install-review',
      context,
      'risk:sha256:install-review',
    );
    expect(await screen.findByText('Applied')).toBeInTheDocument();
  });
});

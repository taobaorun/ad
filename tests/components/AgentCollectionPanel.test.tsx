import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentCollectionPanel } from '@/components/AgentCollectionPanel';
import i18n from '@/i18n';
import { ProjectWorkspaceInventorySchema } from '@/lib/agentResourceInventoryTypes';
import {
  AgentContextSchema,
  CapabilityDescriptorSchema,
  DeclarationKeySchema,
  PhysicalTargetIdSchema,
  ResourceKeySchema,
} from '@/lib/agentTypes';

const {
  inspectProjectAgentWorkspace,
  previewProjectCollectionAction,
  previewProjectCollectionSourceInstall,
  applyProjectCollectionAction,
} = vi.hoisted(() => ({
  inspectProjectAgentWorkspace: vi.fn(),
  previewProjectCollectionAction: vi.fn(),
  previewProjectCollectionSourceInstall: vi.fn(),
  applyProjectCollectionAction: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    inspectProjectAgentWorkspace,
    previewProjectCollectionAction,
    previewProjectCollectionSourceInstall,
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
            actions: [{ action: 'inspect', intent: 'standard', availability: 'available' }],
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
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, resolve, reject };
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
      { action: 'inspect', intent: 'standard', availability: 'available' },
      { action: 'install', intent: 'standard', availability: 'confirmation_required' },
      {
        action: 'remove',
        intent: 'standard',
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

function conflictingSkillInventory() {
  const next = inventory('conflicting-skills');
  const personal = next.skills.resources[0]!;
  personal.logicalId = 'html-artifact';
  personal.displayName = 'html-artifact';
  personal.description = 'Personal catalog candidate';
  personal.effectiveState = 'conflict';
  personal.provenance.declarations = [];
  personal.provenance.source = {
    kind: 'catalog_git',
    displayName: 'Personal Skill Catalog',
    location:
      'https://github.com/example/a-very-long-personal-skill-catalog-location-that-must-remain-fully-readable.git',
    branch: 'stable',
    subdirectory: 'skills/html-artifact',
  };
  delete personal.provenance.winner;
  personal.management = {
    status: 'read_only',
    actions: [{ action: 'inspect', intent: 'standard', availability: 'available' }],
  };
  const team = structuredClone(personal);
  team.key = ResourceKeySchema.parse('resource:sha256:html-artifact-team');
  team.description = 'Team catalog candidate';
  team.provenance.source = {
    kind: 'catalog_local',
    displayName: 'Team Skill Catalog',
    location: '/Users/test/team/skill-sources',
  };
  next.skills.resources = [personal, team];
  next.skills.coverage.observed = 2;
  next.skills.coverage.visible = 2;
  next.plugins.resources = [];
  next.plugins.coverage.observed = 0;
  next.plugins.coverage.visible = 0;
  return next;
}

function sourceGroupedInventory() {
  const next = actionableInventory();
  const review = next.skills.resources[0]!;
  review.provenance.source = {
    kind: 'catalog_git',
    displayName: 'Team Skills',
    location: 'https://github.com/example/team-skills.git',
    branch: 'main',
  };
  const format = structuredClone(review);
  format.key = ResourceKeySchema.parse('resource:sha256:format-available');
  format.logicalId = 'format-available';
  format.displayName = 'Format available';
  format.description = 'Formats the active change';
  next.skills.resources = [review, format];
  next.skills.coverage.observed = 2;
  next.skills.coverage.visible = 2;
  return next;
}

function singleInstalledSkillInventory() {
  const next = actionableInventory();
  const resource = next.skills.resources[0]!;
  resource.logicalId = 'ad-skill-governance';
  resource.displayName = 'ad-skill-governance';
  resource.description = undefined;
  resource.effectiveState = 'enabled';
  resource.provenance.source = {
    kind: 'installed_path',
    displayName: 'ad-skill-governance',
    location: '/Users/test/project/.agents/skills/ad-skill-governance',
  };
  resource.provenance.declarations = [
    {
      key: DeclarationKeySchema.parse('declaration:sha256:governance'),
      layer: 'project',
      sourceId: 'installed:ad-skill-governance',
      targetId: PhysicalTargetIdSchema.parse('target:sha256:governance'),
      scope: 'project',
    },
  ];
  resource.management = {
    status: 'read_only',
    actions: [
      {
        action: 'remove',
        intent: 'standard',
        availability: 'unavailable',
        limitation: {
          code: 'read_only',
          messageKey: 'agents.resources.readOnly',
        },
      },
    ],
  };
  next.plugins.resources = [];
  next.plugins.coverage.observed = 0;
  next.plugins.coverage.visible = 0;
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
          activationImpact: [
            {
              kind: 'code_execution' as const,
              summaryKey: 'agents.plan.impact.codeExecution',
            },
          ],
        },
      ],
      requiredAcknowledgements: [
        { code: 'project_collection_apply' as const, risk: 'confirmation' as const },
      ],
      riskFingerprint: 'risk:sha256:install-review',
      expiresAt: '2026-08-01T09:00:00Z',
    },
  };
}

function sourceInstallPreview() {
  const current = sourceGroupedInventory();
  const review = current.skills.resources[0]!;
  const format = current.skills.resources[1]!;
  const preview = actionPreview();
  return {
    workspaceKey: current.workspace.key,
    source: review.provenance.source!,
    resourceKeys: [review.key, format.key],
    plan: {
      ...preview.plan,
      id: 'plan:install-team-skills',
      changes: [
        preview.plan.changes[0]!,
        {
          ...preview.plan.changes[0]!,
          resource: {
            ...preview.plan.changes[0]!.resource,
            logicalId: format.logicalId,
          },
          target: {
            ...preview.plan.changes[0]!.target,
            id: 'target:sha256:format-install',
            display: 'Project Skill format',
          },
        },
      ],
      riskFingerprint: 'risk:sha256:install-team-skills',
    },
  };
}

describe('AgentCollectionPanel', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    inspectProjectAgentWorkspace.mockReset().mockResolvedValue(inventory());
    previewProjectCollectionAction.mockReset();
    previewProjectCollectionSourceInstall.mockReset();
    applyProjectCollectionAction.mockReset();
  });

  it('lists complete catalog source addresses and offers conflict recovery actions', async () => {
    inspectProjectAgentWorkspace.mockResolvedValue(conflictingSkillInventory());
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    expect(
      await screen.findByText('“html-artifact” has 2 conflicting sources'),
    ).toBeInTheDocument();
    const guidance = screen.getByRole('alert');
    expect(within(guidance).getByText('Personal Skill Catalog')).toBeInTheDocument();
    expect(within(guidance).getByText('Git source')).toBeInTheDocument();
    const gitAddress = within(guidance).getByText(
      'https://github.com/example/a-very-long-personal-skill-catalog-location-that-must-remain-fully-readable.git',
    );
    expect(gitAddress).toHaveClass('font-mono', 'break-all');
    expect(gitAddress).not.toHaveClass('truncate');
    expect(within(guidance).getByText('Branch: stable')).toBeInTheDocument();
    expect(within(guidance).getByText('Subdirectory: skills/html-artifact')).toBeInTheDocument();
    expect(within(guidance).getByText('Team Skill Catalog')).toBeInTheDocument();
    expect(within(guidance).getByText('Local source')).toBeInTheDocument();
    expect(within(guidance).getByText('/Users/test/team/skill-sources')).toBeInTheDocument();
    expect(
      within(guidance).getByText(/uninstall the current installation first/i),
    ).toBeInTheDocument();
    expect(within(guidance).queryByRole('link')).not.toBeInTheDocument();
    expect(
      within(guidance).queryByRole('button', { name: 'Manage Skill sources' }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    await act(async () => undefined);
    expect(inspectProjectAgentWorkspace).toHaveBeenCalledTimes(2);
  });

  it('labels an installed Skill location and reports unavailable source provenance', async () => {
    const conflicting = conflictingSkillInventory();
    const installed = conflicting.skills.resources[0]!;
    installed.provenance.source = {
      kind: 'installed_path',
      displayName: 'Codex Skill directory',
      location: '/Users/test/.codex/skills/html-artifact',
    };
    const unavailable = conflicting.skills.resources[1]!;
    unavailable.description = 'Untracked Skill';
    delete unavailable.provenance.source;
    inspectProjectAgentWorkspace.mockResolvedValue(conflicting);

    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    const guidance = await screen.findByRole('alert');
    expect(within(guidance).getByText('Codex Skill directory')).toBeInTheDocument();
    expect(within(guidance).getByText('Current installation')).toBeInTheDocument();
    expect(
      within(guidance).getByText('/Users/test/.codex/skills/html-artifact'),
    ).toBeInTheDocument();
    expect(within(guidance).getByText('Untracked Skill')).toBeInTheDocument();
    expect(within(guidance).getByText('Source address unavailable')).toBeInTheDocument();
  });

  it('renders backend-owned effective state, provenance, health, and partial coverage', async () => {
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    expect(await screen.findByText('Review current')).toBeInTheDocument();
    expect(screen.getByText('Demo current')).toBeInTheDocument();
    expect(screen.getByText('Conflict')).toBeInTheDocument();
    expect(screen.getAllByText(/Read-only/)).toHaveLength(2);
    expect(screen.getByLabelText('Degraded resource')).toBeInTheDocument();
    expect(screen.getAllByText(/partial inventory/i)).toHaveLength(2);
    expect(screen.getByText('User layer declaration')).toBeInTheDocument();
    expect(screen.getByText('Local / Runtime layer declaration')).toBeInTheDocument();
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
    expect(
      screen.getByText('Changes code or runtime entry points available to the coding Agent'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove: Review available' })).toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    expect(applyProjectCollectionAction).toHaveBeenCalledWith(
      'plan:install-review',
      context,
      'risk:sha256:install-review',
    );
    expect(await screen.findByText('Applied')).toBeInTheDocument();
  });

  it('groups project Skills by source and installs every eligible Skill with one plan', async () => {
    const available = sourceGroupedInventory();
    inspectProjectAgentWorkspace.mockResolvedValue(available);
    previewProjectCollectionSourceInstall.mockResolvedValue(sourceInstallPreview());
    applyProjectCollectionAction.mockResolvedValue({
      workspaceKey: available.workspace.key,
      outcome: 'changed',
      issues: [],
    });
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    expect(await screen.findByRole('heading', { name: 'Team Skills' })).toBeInTheDocument();
    expect(screen.getByText('https://github.com/example/team-skills.git')).not.toBeVisible();
    expect(screen.getByText('2 Skills')).toBeInTheDocument();
    expect(screen.getByText('Review available')).toBeInTheDocument();
    expect(screen.getByText('Format available')).toBeInTheDocument();
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'review' } });
    expect(screen.queryByText('Format available')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Install all (2)' })).toBeInTheDocument();
    fireEvent.click(screen.getByText('Source details'));
    expect(screen.getByText('https://github.com/example/team-skills.git')).toBeVisible();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Install all (2)' }));
    });

    expect(previewProjectCollectionSourceInstall).toHaveBeenCalledWith(
      context.installationId,
      context.projectPath,
      {
        workspaceKey: available.workspace.key,
        inventoryRevision: available.revision,
        sourceResourceKey: available.skills.resources[0]!.key,
      },
    );
    expect(await screen.findByText('Review changes')).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    });
    expect(applyProjectCollectionAction).toHaveBeenCalledOnce();
    expect(applyProjectCollectionAction).toHaveBeenCalledWith(
      'plan:install-team-skills',
      context,
      'risk:sha256:install-team-skills',
    );
  });

  it('merges a same-named installed Skill into one card and hides its path by default', async () => {
    inspectProjectAgentWorkspace.mockResolvedValue(singleInstalledSkillInventory());
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    expect(await screen.findByText('ad-skill-governance')).toBeInTheDocument();
    expect(screen.getAllByText('ad-skill-governance')).toHaveLength(1);
    expect(screen.getByText('Enabled')).toBeInTheDocument();
    expect(screen.getByText('Current installation')).toBeInTheDocument();
    const location = screen.getByText('/Users/test/project/.agents/skills/ad-skill-governance');
    expect(location).not.toBeVisible();

    fireEvent.click(screen.getByText('Source details'));
    expect(location).toBeVisible();
  });

  it('renders backend-owned relink and repair action intent', async () => {
    const relink = actionableInventory();
    const relinkResource = relink.skills.resources[0]!;
    relinkResource.management.actions = [
      { action: 'update', intent: 'relink', availability: 'confirmation_required' },
    ];
    inspectProjectAgentWorkspace.mockResolvedValueOnce(relink);
    const first = render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    expect(await screen.findByRole('button', { name: 'Relink: Review available' })).toBeEnabled();
    first.unmount();

    const repair = actionableInventory();
    const repairResource = repair.skills.resources[0]!;
    repairResource.management.actions = [
      { action: 'update', intent: 'repair', availability: 'confirmation_required' },
    ];
    inspectProjectAgentWorkspace.mockResolvedValueOnce(repair);
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    expect(await screen.findByRole('button', { name: 'Repair: Review available' })).toBeEnabled();
  });

  it('returns focus on cancel and supports keyboard dismissal', async () => {
    const available = actionableInventory();
    inspectProjectAgentWorkspace.mockResolvedValue(available);
    previewProjectCollectionAction.mockResolvedValue(actionPreview());
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    const install = await screen.findByRole('button', { name: 'Install: Review available' });
    install.focus();
    fireEvent.click(install);
    const cancel = await screen.findByRole('button', { name: 'Cancel' });
    expect(cancel).toHaveFocus();

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(install).toHaveFocus();
  });

  it('keeps the plan dialog open through apply and refresh progress', async () => {
    const available = actionableInventory();
    const apply = deferred<{
      workspaceKey: string;
      outcome: 'changed';
      issues: never[];
    }>();
    const refresh = deferred<ReturnType<typeof inventory>>();
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(available)
      .mockReturnValueOnce(refresh.promise);
    previewProjectCollectionAction.mockResolvedValue(actionPreview());
    applyProjectCollectionAction.mockReturnValue(apply.promise);

    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Install: Review available' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));
    expect(await screen.findByText('Installing project resources…')).toBeInTheDocument();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    await act(async () =>
      apply.resolve({
        workspaceKey: available.workspace.key,
        outcome: 'changed',
        issues: [],
      }),
    );
    expect(await screen.findByText('Refreshing project resources…')).toBeInTheDocument();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    await act(async () => refresh.resolve(inventory('installed')));

    expect(await screen.findByText('Applied')).toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(applyProjectCollectionAction).toHaveBeenCalledOnce();
  });

  it('keeps an install failure in the dialog and retries without duplicate submission', async () => {
    const available = actionableInventory();
    const firstApply = deferred<{
      workspaceKey: string;
      outcome: 'changed';
      issues: never[];
    }>();
    inspectProjectAgentWorkspace.mockResolvedValue(available);
    previewProjectCollectionAction.mockResolvedValue(actionPreview());
    applyProjectCollectionAction.mockReturnValueOnce(firstApply.promise).mockResolvedValueOnce({
      workspaceKey: available.workspace.key,
      outcome: 'changed',
      issues: [],
    });

    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Install: Review available' }));
    const applyButton = await screen.findByRole('button', { name: 'Apply' });
    fireEvent.click(applyButton);
    fireEvent.click(applyButton);
    expect(await screen.findByText('Installing project resources…')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Apply' })).not.toBeInTheDocument();
    expect(applyProjectCollectionAction).toHaveBeenCalledOnce();

    await act(async () => firstApply.reject(new Error('Install failed')));
    expect(await screen.findByText('Install failed')).toBeInTheDocument();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    const retry = screen.getByRole('button', { name: 'Retry' });
    fireEvent.click(retry);

    expect(await screen.findByText('Applied')).toBeInTheDocument();
    expect(applyProjectCollectionAction).toHaveBeenCalledTimes(2);
  });

  it('does not report external or conflicting outcomes as applied', async () => {
    const available = actionableInventory();
    inspectProjectAgentWorkspace.mockResolvedValue(available);
    previewProjectCollectionAction.mockResolvedValue(actionPreview());
    applyProjectCollectionAction.mockResolvedValue({
      workspaceKey: available.workspace.key,
      outcome: 'conflict',
      issues: [],
    });
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Install: Review available' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));

    expect(
      await screen.findByText('The action stopped because the project resource is in conflict.'),
    ).toBeInTheDocument();
    expect(screen.queryByText('Applied')).not.toBeInTheDocument();
  });

  it('distinguishes an empty workspace from an empty filter result', async () => {
    const empty = inventory('empty');
    empty.skills.resources = [];
    empty.plugins.resources = [];
    inspectProjectAgentWorkspace.mockResolvedValueOnce(empty);
    const first = render(<AgentCollectionPanel context={context} capabilities={capabilities} />);
    expect(
      await screen.findByText('No Skills or Plugins were observed in this project workspace.'),
    ).toBeInTheDocument();
    first.unmount();

    const categoryEmpty = inventory('category-empty');
    categoryEmpty.plugins.resources = [];
    inspectProjectAgentWorkspace.mockResolvedValueOnce(categoryEmpty);
    const second = render(<AgentCollectionPanel context={context} capabilities={capabilities} />);
    expect(await screen.findByText('No Plugins were observed.')).toBeInTheDocument();
    second.unmount();

    inspectProjectAgentWorkspace.mockResolvedValueOnce(inventory());
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);
    await screen.findByText('Review current');
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'missing' } });
    expect(screen.getByText('No Skills or Plugins match this filter.')).toBeInTheDocument();
  });
});

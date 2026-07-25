import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentCollectionPanel } from '@/components/AgentCollectionPanel';
import i18n from '@/i18n';
import {
  AgentContextSchema,
  CapabilityDescriptorSchema,
  MutationPlanViewSchema,
  ResourceSnapshotSchema,
} from '@/lib/agentTypes';

const {
  listAgentSkills,
  listAgentPlugins,
  previewAgentCollectionToggle,
  applyAgentPlan,
  rollbackAgentReceipt,
} = vi.hoisted(() => ({
  listAgentSkills: vi.fn(),
  listAgentPlugins: vi.fn(),
  previewAgentCollectionToggle: vi.fn(),
  applyAgentPlan: vi.fn(),
  rollbackAgentReceipt: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    listAgentSkills,
    listAgentPlugins,
    previewAgentCollectionToggle,
    applyAgentPlan,
    rollbackAgentReceipt,
  },
}));

const context = AgentContextSchema.parse({
  installationId: 'codex:default',
  projectPath: '/Users/test/project',
});
const skill = ResourceSnapshotSchema.parse({
  resource: {
    installationId: 'codex:default',
    projectPath: '/Users/test/project',
    kind: 'skills',
    scope: 'project',
    logicalId: 'review',
  },
  location: { path: '/Users/test/project/.agents/skills/review', origin: 'project' },
  mediaType: 'application/vnd.ad.skill+json',
  content: { name: 'Review', enabled: true },
  digest: 'sha256:skill',
  observedAt: '2026-07-15T01:00:00Z',
});
const capabilities = CapabilityDescriptorSchema.array().parse([
  {
    kind: 'skills',
    scopes: ['user', 'project'],
    operations: ['list', 'enable', 'disable', 'preview', 'apply'],
    availability: 'available',
    limitations: [],
  },
  {
    kind: 'plugins',
    scopes: ['user'],
    operations: ['list', 'enable', 'disable', 'preview', 'apply'],
    availability: 'degraded',
    limitations: [
      {
        code: 'marketplace',
        messageKey: 'agents.capabilities.codexPluginInstallRequiresMarketplace',
      },
    ],
  },
]);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe('AgentCollectionPanel', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    listAgentSkills.mockReset().mockResolvedValue([skill]);
    listAgentPlugins.mockReset().mockResolvedValue([]);
    previewAgentCollectionToggle.mockReset().mockResolvedValue({
      id: 'plan-2',
      agentId: 'codex',
      context,
      changes: [{ resource: skill.resource, kind: 'replace' }],
      expiresAt: '2026-07-15T01:05:00Z',
    });
    applyAgentPlan.mockReset().mockResolvedValue({
      id: 'receipt-2',
      planId: 'plan-2',
      status: 'complete',
      appliedResources: [skill.resource],
      backupPaths: [],
      postApplyStates: [],
    });
    rollbackAgentReceipt.mockReset().mockResolvedValue({
      id: 'rollback-2',
      planId: 'rollback-plan',
      status: 'complete',
      appliedResources: [],
      backupPaths: [],
      postApplyStates: [],
    });
    vi.spyOn(window, 'confirm').mockReturnValue(true);
  });

  it('previews a collection toggle before applying it', async () => {
    const runtimeChanged = vi.fn();
    const workspaceChanged = vi.fn();
    window.addEventListener('ad:project-codex-runtime-changed', runtimeChanged);
    window.addEventListener('ad:agent-workspace-changed', workspaceChanged);
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    fireEvent.click(await screen.findByRole('switch', { name: 'Disable Review' }));
    await waitFor(() =>
      expect(previewAgentCollectionToggle).toHaveBeenCalledWith(context, skill.resource, false),
    );
    expect(applyAgentPlan).not.toHaveBeenCalled();

    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(applyAgentPlan).toHaveBeenCalledWith('plan-2'));
    await waitFor(() => expect(runtimeChanged).toHaveBeenCalledTimes(1));
    expect(workspaceChanged).toHaveBeenCalledTimes(1);
    fireEvent.click(await screen.findByRole('button', { name: 'Rollback' }));
    await waitFor(() => expect(rollbackAgentReceipt).toHaveBeenCalledWith('receipt-2', true));
    await waitFor(() => expect(runtimeChanged).toHaveBeenCalledTimes(2));
    expect(workspaceChanged).toHaveBeenCalledTimes(2);
    window.removeEventListener('ad:project-codex-runtime-changed', runtimeChanged);
    window.removeEventListener('ad:agent-workspace-changed', workspaceChanged);
  });

  it('keeps Skills visible when Plugin loading fails', async () => {
    listAgentPlugins.mockRejectedValueOnce(new Error('Plugin metadata failed'));

    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    expect(await screen.findByRole('switch', { name: 'Disable Review' })).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('Plugin metadata failed');
  });

  it('disables toggles that are not allowed by the capability descriptor', async () => {
    const listOnly = CapabilityDescriptorSchema.array().parse([
      {
        kind: 'skills',
        scopes: ['project'],
        operations: ['list'],
        availability: 'available',
        limitations: [],
      },
    ]);

    render(<AgentCollectionPanel context={context} capabilities={listOnly} />);

    expect(await screen.findByRole('switch', { name: 'Disable Review' })).toBeDisabled();
    expect(previewAgentCollectionToggle).not.toHaveBeenCalled();
  });

  it('discards a pending collection preview when the Agent context changes', async () => {
    const pendingPreview = deferred<ReturnType<typeof MutationPlanViewSchema.parse>>();
    const nextContext = AgentContextSchema.parse({
      installationId: 'codex:project',
      projectPath: '/Users/test/project',
    });
    const nextSkill = ResourceSnapshotSchema.parse({
      ...skill,
      resource: {
        ...skill.resource,
        installationId: nextContext.installationId,
        logicalId: 'runtime-review',
      },
      content: { name: 'Runtime Review', enabled: true },
      digest: 'sha256:runtime-skill',
    });
    listAgentSkills.mockResolvedValueOnce([skill]).mockResolvedValueOnce([nextSkill]);
    listAgentPlugins.mockResolvedValueOnce([]).mockResolvedValueOnce([]);
    previewAgentCollectionToggle.mockReturnValueOnce(pendingPreview.promise);

    const { rerender } = render(
      <AgentCollectionPanel context={context} capabilities={capabilities} />,
    );
    fireEvent.click(await screen.findByRole('switch', { name: 'Disable Review' }));
    await waitFor(() => expect(previewAgentCollectionToggle).toHaveBeenCalledTimes(1));

    rerender(<AgentCollectionPanel context={nextContext} capabilities={capabilities} />);
    expect(
      await screen.findByRole('switch', { name: 'Disable Runtime Review' }),
    ).toBeInTheDocument();
    await act(async () => {
      pendingPreview.resolve(
        MutationPlanViewSchema.parse({
          id: 'stale-plan',
          agentId: 'codex',
          context,
          changes: [{ resource: skill.resource, kind: 'replace' }],
          expiresAt: '2026-07-15T01:05:00Z',
        }),
      );
    });

    expect(screen.queryByRole('button', { name: 'Apply' })).not.toBeInTheDocument();
  });

  it('retains a partial receipt so the collection change can be rolled back', async () => {
    applyAgentPlan.mockResolvedValueOnce({
      id: 'partial-receipt',
      planId: 'plan-2',
      status: 'partial_failure',
      appliedResources: [skill.resource],
      backupPaths: ['/tmp/backup'],
      postApplyStates: [],
    });
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    fireEvent.click(await screen.findByRole('switch', { name: 'Disable Review' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));

    expect(
      await screen.findByText('Apply only completed partially; rollback is available.'),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Rollback' }));
    await waitFor(() => expect(rollbackAgentReceipt).toHaveBeenCalledWith('partial-receipt', true));
  });

  it('reports a compensated receipt without offering rollback', async () => {
    applyAgentPlan.mockResolvedValueOnce({
      id: 'compensated-receipt',
      planId: 'plan-2',
      status: 'compensated',
      appliedResources: [skill.resource],
      backupPaths: ['/tmp/backup'],
      postApplyStates: [],
    });
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    fireEvent.click(await screen.findByRole('switch', { name: 'Disable Review' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));

    expect(
      await screen.findByText('Apply failed; all collection changes were restored.'),
    ).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Rollback' })).not.toBeInTheDocument();
  });
});

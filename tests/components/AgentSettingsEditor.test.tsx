import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentSettingsEditor } from '@/components/AgentSettingsEditor';
import i18n from '@/i18n';
import { ProjectWorkspaceInventorySchema } from '@/lib/agentResourceInventoryTypes';
import {
  AgentContextSchema,
  MutationPlanViewSchema,
  OperationReceiptSchema,
} from '@/lib/agentTypes';
import { requestAgentWorkspaceChange } from '@/lib/workspaceDraftGuard';

const {
  inspectProjectAgentWorkspace,
  previewAgentSettingsEdit,
  applyAgentPlan,
  previewAgentRollback,
  applyAgentRollbackPlan,
} = vi.hoisted(() => ({
  inspectProjectAgentWorkspace: vi.fn(),
  previewAgentSettingsEdit: vi.fn(),
  applyAgentPlan: vi.fn(),
  previewAgentRollback: vi.fn(),
  applyAgentRollbackPlan: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    inspectProjectAgentWorkspace,
    previewAgentSettingsEdit,
    applyAgentPlan,
    previewAgentRollback,
    applyAgentRollbackPlan,
  },
}));

vi.mock('@/components/JsonEditor', () => ({
  JsonEditor: ({
    value,
    onChange,
    readOnly,
  }: {
    value: string;
    onChange: (next: string) => void;
    readOnly?: boolean;
  }) => (
    <textarea
      aria-label="Settings content"
      value={value}
      readOnly={readOnly}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const context = AgentContextSchema.parse({
  installationId: 'claude-code:default',
  projectPath: '/Users/test/project',
});

function inventory(
  model = 'project',
  key = 'workspace:sha256:project',
  effectiveInstallationId = 'claude-code:default',
) {
  return ProjectWorkspaceInventorySchema.parse({
    schemaVersion: 1,
    workspace: {
      schemaVersion: 1,
      key,
      revision: `workspace-revision:sha256:${model}`,
      agentId: 'claude-code',
      canonicalProjectPath: '/Users/test/project',
      baseInstallationId: 'claude-code:default',
      effectiveInstallationId,
    },
    revision: `inventory-revision:sha256:${model}`,
    discovery: {
      adapterVersion: 1,
      locationSet: 'claude-project-v1',
      schemaVersions: ['claude-settings-json-v1'],
      verifiedAgentVersions: [],
      compatibility: 'unverified',
    },
    settings: {
      workspaceKey: key,
      coverage: {
        status: 'partial',
        observed: 2,
        visible: 2,
        diagnostics: [
          {
            code: 'agent_version_unverified',
            messageKey: 'agents.inventory.agentVersionUnverified',
          },
        ],
      },
      effectiveContent: { model, apiToken: '••••••••' },
      fields: [],
      layers: [
        {
          declaration: {
            key: 'declaration:sha256:user',
            layer: 'user',
            sourceId: 'user-settings',
            targetId: 'target:sha256:user',
            scope: 'user',
          },
          logicalId: 'user-settings',
          mediaType: 'application/json',
          content: { model: 'user', apiToken: '••••••••' },
          exists: true,
          editable: false,
          preservesUnknownFields: true,
          redactedPaths: ['/apiToken'],
        },
        {
          declaration: {
            key: 'declaration:sha256:project',
            layer: 'project',
            sourceId: 'project-shared',
            targetId: 'target:sha256:project',
            scope: 'project',
          },
          logicalId: 'project-shared',
          mediaType: 'application/json',
          content: { model },
          exists: true,
          editable: true,
          preservesUnknownFields: true,
          redactedPaths: [],
        },
      ],
      editableTargets: [
        {
          declarationKey: 'declaration:sha256:project',
          resource: {
            installationId: 'claude-code:default',
            projectPath: '/Users/test/project',
            kind: 'settings',
            scope: 'project',
            logicalId: 'project-shared',
          },
          mediaType: 'application/json',
          exists: true,
          preservesUnknownFields: true,
          redactedPaths: [],
        },
      ],
    },
    skills: {
      workspaceKey: key,
      agentId: 'claude-code',
      kind: 'skills',
      coverage: { status: 'partial', observed: 0, visible: 0, diagnostics: [] },
      resources: [],
    },
    plugins: {
      workspaceKey: key,
      agentId: 'claude-code',
      kind: 'plugins',
      coverage: { status: 'partial', observed: 0, visible: 0, diagnostics: [] },
      resources: [],
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

describe('AgentSettingsEditor', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    inspectProjectAgentWorkspace.mockReset().mockResolvedValue(inventory());
    previewAgentSettingsEdit.mockReset().mockResolvedValue(
      MutationPlanViewSchema.parse({
        id: 'plan-1',
        agentId: 'claude-code',
        context,
        changes: [
          {
            resource: {
              installationId: 'claude-code:default',
              projectPath: '/Users/test/project',
              kind: 'settings',
              scope: 'project',
              logicalId: 'project-shared',
            },
            kind: 'replace',
            target: {
              id: 'target:sha256:project',
              kind: 'agent_resource',
              display: 'settings/project-shared',
            },
            scope: 'project',
            dependencies: [],
            activationImpact: [
              { kind: 'configuration', summaryKey: 'agents.plan.impact.configuration' },
            ],
          },
        ],
        requiredAcknowledgements: [],
        riskFingerprint: 'risk:settings-plan',
        expiresAt: '2026-08-01T08:00:00Z',
      }),
    );
    applyAgentPlan.mockReset().mockResolvedValue(
      OperationReceiptSchema.parse({
        schemaVersion: 2,
        id: 'receipt-1',
        planId: 'plan-1',
        operationKind: 'apply',
        context,
        status: 'complete',
        appliedResources: [],
        backupPaths: [],
        postApplyStates: [],
        rollback: { available: true },
        createdAt: '2026-08-01T08:00:00Z',
      }),
    );
    previewAgentRollback.mockReset();
    applyAgentRollbackPlan.mockReset();
    vi.spyOn(window, 'confirm').mockReturnValue(true);
  });

  it('shows masked effective and inherited layers as read-only', async () => {
    render(<AgentSettingsEditor context={context} />);

    const editor = await screen.findByRole('textbox', { name: 'Settings content' });
    expect(editor).toHaveAttribute('readonly');
    expect((editor as HTMLTextAreaElement).value).toContain('••••••••');
    fireEvent.click(screen.getByRole('tab', { name: 'User · user-settings' }));
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveAttribute('readonly');
    expect(screen.getByText(/read-only/i)).toBeInTheDocument();
  });

  it('previews and applies only an editable project layer', async () => {
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(
        inventory('project', 'workspace:sha256:project', 'claude-code:project-runtime'),
      )
      .mockResolvedValueOnce(inventory('next'));
    render(<AgentSettingsEditor context={context} />);

    await screen.findByRole('textbox', { name: 'Settings content' });
    fireEvent.click(screen.getByRole('tab', { name: 'Project · project-shared' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: '{\n  "model": "next"\n}' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Preview changes' }));

    await waitFor(() =>
      expect(previewAgentSettingsEdit).toHaveBeenCalledWith(
        {
          installationId: 'claude-code:project-runtime',
          projectPath: '/Users/test/project',
        },
        {
          resource: expect.objectContaining({ logicalId: 'project-shared', scope: 'project' }),
          mediaType: 'application/json',
          content: { model: 'next' },
        },
      ),
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));
    await waitFor(() =>
      expect(applyAgentPlan).toHaveBeenCalledWith('plan-1', context, 'risk:settings-plan'),
    );
    await waitFor(() => expect(inspectProjectAgentWorkspace).toHaveBeenCalledTimes(2));
  });

  it('blocks a workspace switch while a settings draft is dirty', async () => {
    vi.mocked(window.confirm).mockReturnValue(false);
    render(<AgentSettingsEditor context={context} />);

    await screen.findByRole('textbox', { name: 'Settings content' });
    fireEvent.click(screen.getByRole('tab', { name: 'Project · project-shared' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: '{\n  "model": "dirty"\n}' },
    });

    expect(requestAgentWorkspaceChange()).toBe(false);
    expect(window.confirm).toHaveBeenCalledWith(
      'Discard the unsaved Settings draft and switch workspace?',
    );
  });

  it('ignores inventory responses from a superseded Agent context', async () => {
    const first = deferred<ReturnType<typeof inventory>>();
    const second = deferred<ReturnType<typeof inventory>>();
    inspectProjectAgentWorkspace
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const nextContext = AgentContextSchema.parse({
      installationId: 'claude-code:next',
      projectPath: '/Users/test/project',
    });

    const { rerender } = render(<AgentSettingsEditor context={context} />);
    rerender(<AgentSettingsEditor context={nextContext} />);
    second.resolve(inventory('next', 'workspace:sha256:next'));
    expect(
      ((await screen.findByRole('textbox', { name: 'Settings content' })) as HTMLTextAreaElement)
        .value,
    ).toContain('next');
    await act(async () => first.resolve(inventory('stale')));
    expect(
      (screen.getByRole('textbox', { name: 'Settings content' }) as HTMLTextAreaElement).value,
    ).not.toContain('stale');
  });
});

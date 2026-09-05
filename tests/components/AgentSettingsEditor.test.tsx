import { StrictMode } from 'react';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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

function settingsPlan() {
  return MutationPlanViewSchema.parse({
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
    directApplyEligible: true,
    requiredAcknowledgements: [],
    riskFingerprint: 'risk:settings-plan',
    expiresAt: '2026-08-01T08:00:00Z',
  });
}

describe('AgentSettingsEditor', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    inspectProjectAgentWorkspace.mockReset().mockResolvedValue(inventory());
    previewAgentSettingsEdit.mockReset().mockResolvedValue(settingsPlan());
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
    const runtimeContext = {
      ...context,
      installationId: 'claude-code:project-runtime' as typeof context.installationId,
    };
    previewAgentSettingsEdit.mockResolvedValue({ ...settingsPlan(), context: runtimeContext });
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
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

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
    await waitFor(() =>
      expect(applyAgentPlan).toHaveBeenCalledWith('plan-1', runtimeContext, 'risk:settings-plan'),
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

  it('refreshes workspace inventory without discarding a dirty project draft', async () => {
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockResolvedValueOnce(inventory('changed-on-disk'));
    render(<AgentSettingsEditor context={context} />);

    await screen.findByRole('textbox', { name: 'Settings content' });
    fireEvent.click(screen.getByRole('tab', { name: 'Project · project-shared' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: '{\n  "model": "dirty"\n}' },
    });
    act(() => window.dispatchEvent(new Event('ad:agent-workspace-changed')));

    await waitFor(() => expect(inspectProjectAgentWorkspace).toHaveBeenCalledTimes(2));
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(
      '{\n  "model": "dirty"\n}',
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

  async function editAndSave(text = '{"model":"next"}') {
    await screen.findByRole('textbox', { name: 'Settings content' });
    fireEvent.click(screen.getByRole('tab', { name: 'Project · project-shared' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: text },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  }

  it('saves an ordinary edit and undoes it without either confirmation step', async () => {
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockResolvedValueOnce(inventory('next'))
      .mockResolvedValueOnce(inventory());
    previewAgentRollback.mockResolvedValue({
      ...settingsPlan(),
      id: 'undo-plan',
      requiredAcknowledgements: [{ code: 'rollback_apply', risk: 'confirmation' }],
    });
    applyAgentRollbackPlan.mockResolvedValue({
      status: 'complete',
      rollback: { available: false },
    });
    render(
      <StrictMode>
        <AgentSettingsEditor context={context} />
      </StrictMode>,
    );
    // StrictMode repeats the initial read; start the mutation with a stable baseline.
    inspectProjectAgentWorkspace.mockResolvedValue(inventory('next'));
    await editAndSave();
    await screen.findByText('Saved');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(applyAgentPlan).toHaveBeenCalledTimes(1);
    inspectProjectAgentWorkspace.mockResolvedValue(inventory());
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    await screen.findByText('Undone');
    expect(applyAgentRollbackPlan).toHaveBeenCalledWith(
      'undo-plan',
      context,
      'risk:settings-plan',
      true,
    );
    expect(window.confirm).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeInTheDocument();
  });

  it('keeps unknown or risky edits in one review and cancel writes nothing', async () => {
    previewAgentSettingsEdit.mockResolvedValue({
      ...settingsPlan(),
      directApplyEligible: false,
      changedSettingsKeys: ['hooks'],
    });
    render(<AgentSettingsEditor context={context} />);
    await editAndSave('{"model":"project","hooks":{"Stop":[]}}');
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Will change: hooks.')).toBeInTheDocument();
    expect(within(dialog).queryByText('Confirmation required')).not.toBeInTheDocument();
    expect(applyAgentPlan).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(applyAgentPlan).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(
      '{"model":"project","hooks":{"Stop":[]}}',
    );
  });

  it('never skips an explicit risk acknowledgement even with an eligibility hint', async () => {
    previewAgentSettingsEdit.mockResolvedValue({
      ...settingsPlan(),
      requiredAcknowledgements: [{ code: 'dangerous_permission_expansion', risk: 'dangerous' }],
    });
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Dangerous permission change')).toBeInTheDocument();
    expect(applyAgentPlan).not.toHaveBeenCalled();
  });

  it('does not promise undo when the receipt cannot be rolled back', async () => {
    applyAgentPlan.mockResolvedValue({ status: 'complete', rollback: { available: false } });
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockResolvedValue(inventory('next'));
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await screen.findByText('Saved');
    expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeInTheDocument();
  });

  it('guards repeated saves while previewing and keeps edits typed during apply', async () => {
    const pendingPreview = deferred<ReturnType<typeof settingsPlan>>();
    const pendingApply = deferred<{ status: string; rollback: { available: boolean } }>();
    previewAgentSettingsEdit.mockReturnValue(pendingPreview.promise);
    applyAgentPlan.mockReturnValue(pendingApply.promise);
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockResolvedValue(inventory('next'));
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    fireEvent.click(screen.getByRole('button', { name: 'Working…' }));
    expect(previewAgentSettingsEdit).toHaveBeenCalledTimes(1);
    await act(async () => pendingPreview.resolve(settingsPlan()));
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: '{"model":"newer-draft"}' },
    });
    await act(async () =>
      pendingApply.resolve({ status: 'complete', rollback: { available: true } }),
    );
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(
      '{"model":"newer-draft"}',
    );
    expect(screen.getByRole('button', { name: 'Undo' })).toBeDisabled();
    expect(applyAgentPlan).toHaveBeenCalledTimes(1);
  });

  it('refreshes after a successful save without reapplying it', async () => {
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockRejectedValueOnce(new Error('read failed'))
      .mockResolvedValue(inventory('next'));
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await screen.findByText('Saved');
    expect(screen.getByRole('alert')).toHaveTextContent('The change finished');
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Undo' })).toBeEnabled();
    expect(screen.queryByText('Unsaved')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(inspectProjectAgentWorkspace).toHaveBeenCalledTimes(3));
    expect(applyAgentPlan).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.getByRole('button', { name: 'Undo' })).toBeEnabled());
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    expect(screen.queryByText('Unsaved')).not.toBeInTheDocument();
  });

  it('keeps the draft after compensation and does not report success', async () => {
    applyAgentPlan.mockResolvedValue({ status: 'compensated', rollback: { available: false } });
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await screen.findByRole('alert');
    expect(screen.queryByText('Saved')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(
      '{"model":"next"}',
    );
  });

  it('shows partial save and partial undo as incomplete results', async () => {
    applyAgentPlan.mockResolvedValue({
      id: 'partial-save',
      status: 'partial_failure',
      rollback: { available: true },
      context,
    });
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockResolvedValue(inventory('next'));
    previewAgentRollback.mockResolvedValue({ ...settingsPlan(), id: 'undo-plan' });
    applyAgentRollbackPlan.mockResolvedValue({
      status: 'partial_failure',
      rollback: { available: false },
    });
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent('Some settings were not saved'),
    );
    expect(screen.queryByText('Saved')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent('Only some changes were undone'),
    );
    expect(screen.queryByText('Undone')).not.toBeInTheDocument();
  });

  it('leaves changed files alone when rollback preview refuses', async () => {
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockResolvedValue(inventory('next'));
    previewAgentRollback.mockRejectedValue(new Error('Target changed after apply'));
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await screen.findByText('Saved');
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('Could not undo'));
    expect(applyAgentRollbackPlan).not.toHaveBeenCalled();
    expect(screen.queryByText('Undone')).not.toBeInTheDocument();
  });

  it('does not apply a preview after switching to another context', async () => {
    const pending = deferred<ReturnType<typeof settingsPlan>>();
    previewAgentSettingsEdit.mockReturnValue(pending.promise);
    const { rerender } = render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    rerender(
      <AgentSettingsEditor
        context={{
          ...context,
          installationId: 'claude-code:next' as typeof context.installationId,
        }}
      />,
    );
    await act(async () => pending.resolve(settingsPlan()));
    expect(applyAgentPlan).not.toHaveBeenCalled();
  });

  it('defaults older backend views to review and rejects a malformed eligibility flag', async () => {
    const { directApplyEligible: _eligible, changedSettingsKeys: _keys, ...older } = settingsPlan();
    void _eligible;
    void _keys;
    const parsed = MutationPlanViewSchema.parse(older);
    expect(parsed.directApplyEligible).toBe(false);
    expect(parsed.changedSettingsKeys).toEqual([]);
    expect(MutationPlanViewSchema.safeParse({ ...older, changedSettingsKeys: [42] }).success).toBe(
      false,
    );
    expect(
      MutationPlanViewSchema.safeParse({ ...older, directApplyEligible: 'true' }).success,
    ).toBe(false);
    previewAgentSettingsEdit.mockResolvedValue(parsed);
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await screen.findByRole('dialog');
    expect(applyAgentPlan).not.toHaveBeenCalled();
  });

  it('preserves new typing while undo is in flight', async () => {
    const pending = deferred<{ status: string; rollback: { available: boolean } }>();
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockResolvedValueOnce(inventory('next'))
      .mockResolvedValue(inventory());
    previewAgentRollback.mockResolvedValue(settingsPlan());
    applyAgentRollbackPlan.mockReturnValue(pending.promise);
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await screen.findByText('Saved');
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    await waitFor(() => expect(applyAgentRollbackPlan).toHaveBeenCalledTimes(1));
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: '{"model":"typed-during-undo"}' },
    });
    await act(async () => pending.resolve({ status: 'complete', rollback: { available: false } }));
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(
      '{"model":"typed-during-undo"}',
    );
  });

  it('uses the sealed backend changes in review when the editor baseline is stale', async () => {
    previewAgentSettingsEdit.mockResolvedValue({
      ...settingsPlan(),
      directApplyEligible: false,
      changedSettingsKeys: ['model', 'permissions'],
    });
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Will change: model, permissions.')).toBeInTheDocument();
    expect(applyAgentPlan).not.toHaveBeenCalled();
  });

  it('preserves a newer draft when recovering a failed refresh', async () => {
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockRejectedValueOnce(new Error('read failed'))
      .mockResolvedValue(inventory('next'));
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await screen.findByText('Saved');
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: '{"model":"newer"}' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(inspectProjectAgentWorkspace).toHaveBeenCalledTimes(3));
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(
      '{"model":"newer"}',
    );
    expect(screen.getByRole('button', { name: 'Undo' })).toBeDisabled();
    expect(applyAgentPlan).toHaveBeenCalledTimes(1);
  });

  it('preserves a newer draft that returns to the exact original text during save', async () => {
    const pending = deferred<{ status: string; rollback: { available: boolean } }>();
    applyAgentPlan.mockReturnValue(pending.promise);
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockResolvedValue(inventory('next'));
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await waitFor(() => expect(applyAgentPlan).toHaveBeenCalledTimes(1));
    const original = JSON.stringify({ model: 'project' }, null, 2);
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: original },
    });
    await act(async () => pending.resolve({ status: 'complete', rollback: { available: true } }));
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(original);
    expect(screen.getByRole('button', { name: 'Undo' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
  });

  it('preserves an intentional return to the original text across refresh recovery', async () => {
    inspectProjectAgentWorkspace
      .mockResolvedValueOnce(inventory())
      .mockRejectedValueOnce(new Error('read failed'))
      .mockResolvedValue(inventory('next'));
    render(<AgentSettingsEditor context={context} />);
    await editAndSave();
    await screen.findByText('Saved');
    const original = JSON.stringify({ model: 'project' }, null, 2);
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: original },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(inspectProjectAgentWorkspace).toHaveBeenCalledTimes(3));
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(original);
    expect(screen.getByRole('button', { name: 'Undo' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
    expect(applyAgentPlan).toHaveBeenCalledTimes(1);
  });

  it.each([false, true])(
    'tracks a non-undoable save through refresh failure (newer draft: %s)',
    async (newerDraft) => {
      applyAgentPlan.mockResolvedValue({ status: 'complete', rollback: { available: false } });
      inspectProjectAgentWorkspace
        .mockResolvedValueOnce(inventory())
        .mockRejectedValueOnce(new Error('read failed'))
        .mockResolvedValue(inventory('next'));
      render(<AgentSettingsEditor context={context} />);
      await editAndSave();
      await screen.findByText('Saved');
      expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
      expect(screen.queryByText('Unsaved')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeInTheDocument();
      expect(requestAgentWorkspaceChange()).toBe(true);
      expect(window.confirm).not.toHaveBeenCalled();
      const original = JSON.stringify({ model: 'project' }, null, 2);
      if (newerDraft) {
        fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
          target: { value: original },
        });
        expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
      }
      fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
      await waitFor(() => expect(inspectProjectAgentWorkspace).toHaveBeenCalledTimes(3));
      if (newerDraft) {
        expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(original);
        expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
      } else {
        expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
      }
      expect(applyAgentPlan).toHaveBeenCalledTimes(1);
    },
  );
});

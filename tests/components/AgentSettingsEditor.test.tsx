import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import {
  AgentContextSchema,
  MutationPlanViewSchema,
  SettingsDocumentSchema,
} from '@/lib/agentTypes';
import { AgentSettingsEditor } from '@/components/AgentSettingsEditor';

const { listAgentSettingsDocuments, previewAgentSettingsEdit, applyAgentPlan } = vi.hoisted(() => ({
  listAgentSettingsDocuments: vi.fn(),
  previewAgentSettingsEdit: vi.fn(),
  applyAgentPlan: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    listAgentSettingsDocuments,
    previewAgentSettingsEdit,
    applyAgentPlan,
    previewAgentRollback: vi.fn(),
    applyAgentRollbackPlan: vi.fn(),
  },
}));

vi.mock('@/components/JsonEditor', () => ({
  JsonEditor: ({ value, onChange }: { value: string; onChange: (next: string) => void }) => (
    <textarea
      aria-label="Settings content"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const snapshot = SettingsDocumentSchema.parse({
  resource: {
    installationId: 'codex:default',
    projectPath: '/Users/test/project',
    kind: 'settings',
    scope: 'project',
    logicalId: 'project-config',
  },
  location: { path: '/Users/test/project/.codex/config.toml', origin: 'project' },
  mediaType: 'application/toml',
  content: 'model = "gpt-5.4"\n',
  exists: true,
  digest: 'sha256:before',
});
const localSnapshot = SettingsDocumentSchema.parse({
  ...snapshot,
  resource: {
    ...snapshot.resource,
    logicalId: 'project-local',
  },
  location: {
    path: '/Users/test/project/.codex/config.local.toml',
    origin: 'project',
  },
  content: 'approval_policy = "on-request"\n',
  digest: 'sha256:local-before',
});

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
    listAgentSettingsDocuments.mockReset().mockResolvedValue([snapshot]);
    previewAgentSettingsEdit.mockReset().mockResolvedValue({
      id: 'plan-1',
      agentId: 'codex',
      context: {
        installationId: 'codex:default',
        projectPath: '/Users/test/project',
      },
      changes: [{ resource: snapshot.resource, kind: 'replace' }],
      riskFingerprint: 'risk:settings-plan',
      expiresAt: '2026-07-15T01:05:00Z',
    });
    applyAgentPlan.mockReset().mockResolvedValue({
      id: 'receipt-1',
      planId: 'plan-1',
      status: 'complete',
      appliedResources: [snapshot.resource],
      backupPaths: ['/tmp/backup'],
      postApplyStates: [],
    });
  });

  it('previews before applying an edited settings resource', async () => {
    render(
      <AgentSettingsEditor
        context={AgentContextSchema.parse({
          installationId: 'codex:default',
          projectPath: '/Users/test/project',
        })}
      />,
    );

    const editor = await screen.findByRole('textbox', { name: 'Settings content' });
    fireEvent.change(editor, { target: { value: 'model = "gpt-5.5"\n' } });
    fireEvent.click(screen.getByRole('button', { name: 'Preview changes' }));

    await waitFor(() => expect(previewAgentSettingsEdit).toHaveBeenCalledTimes(1));
    expect(applyAgentPlan).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));

    await waitFor(() =>
      expect(applyAgentPlan).toHaveBeenCalledWith(
        'plan-1',
        {
          installationId: 'codex:default',
          projectPath: '/Users/test/project',
        },
        'risk:settings-plan',
      ),
    );
  });

  it('previews creation when an editable settings target is missing', async () => {
    const missingTarget = SettingsDocumentSchema.parse({
      ...snapshot,
      content: '',
      exists: false,
      digest: undefined,
    });
    listAgentSettingsDocuments.mockResolvedValue([missingTarget]);
    previewAgentSettingsEdit.mockResolvedValue({
      id: 'plan-create',
      agentId: 'codex',
      context: {
        installationId: 'codex:default',
        projectPath: '/Users/test/project',
      },
      changes: [{ resource: missingTarget.resource, kind: 'create' }],
      riskFingerprint: 'risk:settings-create',
      expiresAt: '2026-07-15T01:05:00Z',
    });
    const context = AgentContextSchema.parse({
      installationId: 'codex:default',
      projectPath: '/Users/test/project',
    });

    render(<AgentSettingsEditor context={context} />);
    const editor = await screen.findByRole('textbox', { name: 'Settings content' });
    fireEvent.change(editor, { target: { value: 'model = "gpt-5.5"\n' } });
    fireEvent.click(screen.getByRole('button', { name: 'Preview changes' }));

    await waitFor(() =>
      expect(previewAgentSettingsEdit).toHaveBeenCalledWith(context, {
        resource: missingTarget.resource,
        mediaType: 'application/toml',
        content: 'model = "gpt-5.5"\n',
      }),
    );
    expect(await screen.findByText('Create')).toBeInTheDocument();
  });

  it('preserves unsaved drafts in other tabs after applying the selected resource', async () => {
    const updatedSnapshot = SettingsDocumentSchema.parse({
      ...snapshot,
      content: 'model = "gpt-5.5"\n',
      digest: 'sha256:after',
    });
    listAgentSettingsDocuments
      .mockResolvedValueOnce([snapshot, localSnapshot])
      .mockResolvedValueOnce([updatedSnapshot, localSnapshot]);
    render(
      <AgentSettingsEditor
        context={AgentContextSchema.parse({
          installationId: 'codex:default',
          projectPath: '/Users/test/project',
        })}
      />,
    );

    await screen.findByRole('textbox', { name: 'Settings content' });
    fireEvent.click(screen.getByRole('tab', { name: 'Project · project-local' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: 'approval_policy = "never"\n' },
    });
    fireEvent.click(screen.getByRole('tab', { name: 'Project · project-config' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Settings content' }), {
      target: { value: 'model = "gpt-5.5"\n' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Preview changes' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(listAgentSettingsDocuments).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole('tab', { name: 'Project · project-local' }));
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(
      'approval_policy = "never"\n',
    );
    expect(screen.getByText(/unsaved/i)).toBeInTheDocument();
  });

  it('ignores settings responses from a superseded Agent context', async () => {
    const first = deferred<(typeof snapshot)[]>();
    const second = deferred<(typeof snapshot)[]>();
    const nextSnapshot = SettingsDocumentSchema.parse({
      ...snapshot,
      resource: {
        ...snapshot.resource,
        installationId: 'codex:project',
      },
      location: {
        ...snapshot.location,
        path: '/Users/test/project/.codex/project-config.toml',
      },
      content: 'model = "gpt-5.6"\n',
      digest: 'sha256:project',
    });
    listAgentSettingsDocuments
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const initialContext = AgentContextSchema.parse({
      installationId: 'codex:default',
      projectPath: '/Users/test/project',
    });
    const nextContext = AgentContextSchema.parse({
      installationId: 'codex:project',
      projectPath: '/Users/test/project',
    });

    const { rerender } = render(<AgentSettingsEditor context={initialContext} />);
    rerender(<AgentSettingsEditor context={nextContext} />);
    second.resolve([nextSnapshot]);

    expect(await screen.findByRole('textbox', { name: 'Settings content' })).toHaveValue(
      'model = "gpt-5.6"\n',
    );
    await act(async () => {
      first.resolve([snapshot]);
    });

    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(
      'model = "gpt-5.6"\n',
    );
    expect(screen.getByText('/Users/test/project/.codex/project-config.toml')).toBeInTheDocument();
  });

  it('discards a pending preview when the Agent context changes', async () => {
    const pendingPreview = deferred<ReturnType<typeof MutationPlanViewSchema.parse>>();
    const nextSnapshot = SettingsDocumentSchema.parse({
      ...snapshot,
      resource: {
        ...snapshot.resource,
        installationId: 'codex:project',
        logicalId: 'runtime-config',
      },
      location: {
        ...snapshot.location,
        path: '/Users/test/.ad/codex-homes/project/config.toml',
      },
      content: 'model = "gpt-5.6"\n',
      digest: 'sha256:runtime',
    });
    listAgentSettingsDocuments
      .mockResolvedValueOnce([snapshot])
      .mockResolvedValueOnce([nextSnapshot]);
    previewAgentSettingsEdit.mockReturnValueOnce(pendingPreview.promise);
    const initialContext = AgentContextSchema.parse({
      installationId: 'codex:default',
      projectPath: '/Users/test/project',
    });
    const nextContext = AgentContextSchema.parse({
      installationId: 'codex:project',
      projectPath: '/Users/test/project',
    });

    const { rerender } = render(<AgentSettingsEditor context={initialContext} />);
    fireEvent.change(await screen.findByRole('textbox', { name: 'Settings content' }), {
      target: { value: 'model = "gpt-5.5"\n' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Preview changes' }));
    await waitFor(() => expect(previewAgentSettingsEdit).toHaveBeenCalledTimes(1));

    rerender(<AgentSettingsEditor context={nextContext} />);
    expect(await screen.findByRole('textbox', { name: 'Settings content' })).toHaveValue(
      'model = "gpt-5.6"\n',
    );
    await act(async () => {
      pendingPreview.resolve(
        MutationPlanViewSchema.parse({
          id: 'stale-plan',
          agentId: 'codex',
          context: initialContext,
          changes: [
            {
              resource: snapshot.resource,
              kind: 'replace',
              target: {
                id: 'target:stale-settings',
                kind: 'agent_resource',
                display: 'settings/user-config',
              },
              scope: 'user',
              dependencies: [],
              activationImpact: [
                { kind: 'configuration', summaryKey: 'agents.plan.impact.configuration' },
              ],
            },
          ],
          riskFingerprint: 'risk:stale-settings',
          expiresAt: '2026-07-15T01:05:00Z',
        }),
      );
    });

    expect(screen.queryByRole('button', { name: 'Apply' })).not.toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Settings content' })).toHaveValue(
      'model = "gpt-5.6"\n',
    );
  });
});

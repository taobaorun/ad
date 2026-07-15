import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { AgentContextSchema, SettingsDocumentSchema } from '@/lib/agentTypes';
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
    rollbackAgentReceipt: vi.fn(),
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

    await waitFor(() => expect(applyAgentPlan).toHaveBeenCalledWith('plan-1'));
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
});

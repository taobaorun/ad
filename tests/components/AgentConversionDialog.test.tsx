import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentConversionButton } from '@/components/AgentConversionDialog';
import i18n from '@/i18n';
import { AgentInstallationSchema } from '@/lib/agentTypes';
import { useAgents } from '@/store/agents';
import { useUiState } from '@/store/ui';

const { resolveAgentContext, previewClaudeToCodexRoute, applyConversionPlan, rollbackAgentReceipt } =
  vi.hoisted(() => ({
    resolveAgentContext: vi.fn(),
    previewClaudeToCodexRoute: vi.fn(),
    applyConversionPlan: vi.fn(),
    rollbackAgentReceipt: vi.fn(),
  }));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    resolveAgentContext,
    previewClaudeToCodexRoute,
    applyConversionPlan,
    rollbackAgentReceipt,
  },
}));

const installations = AgentInstallationSchema.array().parse([
  {
    id: 'claude-code:default',
    agentId: 'claude-code',
    rootPath: '/Users/test/.claude',
  },
  {
    id: 'codex:default',
    agentId: 'codex',
    rootPath: '/Users/test/.codex',
  },
]);

describe('AgentConversionButton', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    useAgents.setState({ installations });
    useUiState.setState({ activeProjectPath: '/Users/test/project' });
    resolveAgentContext.mockReset().mockImplementation((installationId, projectPath) =>
      Promise.resolve({
        installationId,
        ...(projectPath ? { projectPath } : {}),
      }),
    );
    previewClaudeToCodexRoute.mockReset().mockResolvedValue({
      sourceAgentId: 'claude-code',
      targetAgentId: 'codex',
      artifacts: [
        {
          id: 'settings:model',
          kind: 'settings',
          source: {
            installationId: 'claude-code:default',
            kind: 'settings',
            scope: 'user',
            logicalId: 'user-settings',
          },
          target: {
            installationId: 'codex:default',
            kind: 'settings',
            scope: 'user',
            logicalId: 'user-config',
          },
          disposition: 'mapped',
          message: 'Model maps to Codex',
        },
      ],
      plan: {
        id: 'conversion-plan',
        agentId: 'codex',
        context: { installationId: 'codex:default' },
        changes: [
          {
            resource: {
              installationId: 'codex:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-config',
            },
            kind: 'replace',
          },
        ],
        expiresAt: '2026-07-15T01:05:00Z',
      },
    });
    applyConversionPlan.mockReset().mockResolvedValue({
      id: 'conversion-receipt',
      planId: 'conversion-plan',
      status: 'complete',
      appliedResources: [],
      backupPaths: ['/Users/test/.ad/backups/config.toml'],
      postApplyStates: [],
    });
    rollbackAgentReceipt.mockReset().mockResolvedValue({
      id: 'rollback-receipt',
      planId: 'rollback-plan',
      status: 'complete',
      appliedResources: [],
      backupPaths: [],
      postApplyStates: [],
    });
    vi.spyOn(window, 'confirm').mockReturnValue(true);
  });

  it('previews artifacts before explicitly applying a conversion', async () => {
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    expect(screen.getByText(/The Claude Code source is read-only/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await screen.findByText('Mapped');
    expect(previewClaudeToCodexRoute).toHaveBeenCalledWith(
      { installationId: 'claude-code:default' },
      { installationId: 'codex:default' },
    );
    expect(screen.getByText('Model maps to Codex')).toBeInTheDocument();
    expect(applyConversionPlan).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Apply conversion' }));
    await waitFor(() => expect(applyConversionPlan).toHaveBeenCalledWith('conversion-plan', true));
    expect(await screen.findByText('1 backup created')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Rollback target' }));
    await waitFor(() =>
      expect(rollbackAgentReceipt).toHaveBeenCalledWith('conversion-receipt', true),
    );
  });

  it('previews only the current project scope with canonical Agent contexts', async () => {
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.change(screen.getByRole('combobox', { name: 'Conversion scope' }), {
      target: { value: 'project' },
    });
    expect(screen.getByText('/Users/test/project')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));

    await waitFor(() => {
      expect(resolveAgentContext).toHaveBeenCalledWith(
        'claude-code:default',
        '/Users/test/project',
      );
      expect(resolveAgentContext).toHaveBeenCalledWith('codex:default', '/Users/test/project');
      expect(previewClaudeToCodexRoute).toHaveBeenCalledWith(
        {
          installationId: 'claude-code:default',
          projectPath: '/Users/test/project',
        },
        {
          installationId: 'codex:default',
          projectPath: '/Users/test/project',
        },
      );
    });
  });

  it('locks scope and installations while a preview is in flight', () => {
    previewClaudeToCodexRoute.mockReturnValue(new Promise(() => {}));
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));

    expect(screen.getByRole('combobox', { name: 'Conversion scope' })).toBeDisabled();
    expect(screen.getByRole('combobox', { name: 'Claude Code source' })).toBeDisabled();
    expect(screen.getByRole('combobox', { name: 'Codex target' })).toBeDisabled();
  });
});

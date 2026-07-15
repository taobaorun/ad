import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentConversionButton } from '@/components/AgentConversionDialog';
import i18n from '@/i18n';
import { AgentInstallationSchema } from '@/lib/agentTypes';
import { useAgents } from '@/store/agents';

const { previewClaudeToCodexRoute, applyConversionPlan, rollbackAgentReceipt } = vi.hoisted(() => ({
  previewClaudeToCodexRoute: vi.fn(),
  applyConversionPlan: vi.fn(),
  rollbackAgentReceipt: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
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
    rollbackAgentReceipt.mockReset();
  });

  it('previews artifacts before explicitly applying a conversion', async () => {
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await screen.findByText('Mapped');
    expect(screen.getByText('Model maps to Codex')).toBeInTheDocument();
    expect(applyConversionPlan).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Apply conversion' }));
    await waitFor(() => expect(applyConversionPlan).toHaveBeenCalledWith('conversion-plan', true));
    expect(await screen.findByText('1 backup created')).toBeInTheDocument();
  });
});

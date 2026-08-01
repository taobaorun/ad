import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { HistoryPanel } from '@/components/HistoryPanel';
import i18n from '@/i18n';
import { AgentContextSchema, AgentInstallationSchema } from '@/lib/agentTypes';
import { useAgents } from '@/store/agents';
import { useUiState } from '@/store/ui';

const { listAgentOperationHistory, rollbackAgentReceipt, readHistory } = vi.hoisted(() => ({
  listAgentOperationHistory: vi.fn(),
  rollbackAgentReceipt: vi.fn(),
  readHistory: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    listAgentOperationHistory,
    rollbackAgentReceipt,
    readHistory,
    restoreBackup: vi.fn(),
  },
}));

describe('HistoryPanel', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    useAgents.setState({
      activeAgentId: 'codex',
      activeContext: AgentContextSchema.parse({ installationId: 'codex:default' }),
      installations: AgentInstallationSchema.array().parse([
        {
          id: 'codex:default',
          agentId: 'codex',
          rootPath: '/Users/test/.codex',
        },
      ]),
    });
    useUiState.setState({ activeProjectPath: null });
    listAgentOperationHistory.mockReset().mockResolvedValue([
      {
        createdAt: '2026-07-15T01:00:00Z',
        receipt: {
          schemaVersion: 2,
          id: 'receipt-1',
          planId: 'plan-1',
          operationKind: 'apply',
          context: { installationId: 'codex:default' },
          status: 'complete',
          appliedResources: [
            {
              installationId: 'codex:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-config',
            },
          ],
          backupPaths: ['/tmp/backup'],
          postApplyStates: [],
          rollback: { available: true },
          createdAt: '2026-07-15T01:00:00Z',
        },
      },
    ]);
    rollbackAgentReceipt.mockReset().mockResolvedValue({
      schemaVersion: 2,
      id: 'rollback-1',
      planId: 'rollback-plan',
      operationKind: 'rollback',
      parentReceiptId: 'receipt-1',
      context: { installationId: 'codex:default' },
      status: 'complete',
      appliedResources: [],
      backupPaths: [],
      postApplyStates: [],
      rollback: { available: false, reason: 'rollback_receipt' },
      createdAt: '2026-07-15T01:01:00Z',
    });
    readHistory.mockReset().mockResolvedValue([]);
    vi.spyOn(window, 'confirm').mockReturnValue(true);
  });

  it('shows operation receipts and rolls them back by receipt id', async () => {
    const runtimeChanged = vi.fn();
    const workspaceChanged = vi.fn();
    window.addEventListener('ad:project-codex-runtime-changed', runtimeChanged);
    window.addEventListener('ad:agent-workspace-changed', workspaceChanged);
    render(<HistoryPanel />);

    expect(await screen.findByText('user-config')).toBeInTheDocument();
    expect(readHistory).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Rollback' }));

    await waitFor(() => expect(rollbackAgentReceipt).toHaveBeenCalledWith('receipt-1', true));
    expect(runtimeChanged).toHaveBeenCalledOnce();
    expect(workspaceChanged).toHaveBeenCalledOnce();
    window.removeEventListener('ad:project-codex-runtime-changed', runtimeChanged);
    window.removeEventListener('ad:agent-workspace-changed', workspaceChanged);
  });

  it('does not offer rollback for an already compensated receipt', async () => {
    listAgentOperationHistory.mockResolvedValueOnce([
      {
        createdAt: '2026-07-15T01:00:00Z',
        receipt: {
          schemaVersion: 2,
          id: 'receipt-compensated',
          planId: 'plan-compensated',
          operationKind: 'apply',
          context: { installationId: 'codex:default' },
          status: 'compensated',
          appliedResources: [
            {
              installationId: 'codex:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-config',
            },
          ],
          backupPaths: ['/tmp/backup'],
          postApplyStates: [],
          rollback: { available: false, reason: 'compensated' },
          createdAt: '2026-07-15T01:00:00Z',
        },
      },
    ]);
    render(<HistoryPanel />);

    expect(await screen.findByText('user-config')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Rollback' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Rollback' }));
    expect(rollbackAgentReceipt).not.toHaveBeenCalled();
  });

  it('includes receipts from the effective Project Codex runtime', async () => {
    useAgents.setState({
      installations: AgentInstallationSchema.array().parse([
        {
          id: 'codex:default',
          agentId: 'codex',
          rootPath: '/Users/test/.codex',
        },
        {
          id: 'codex:runtime',
          agentId: 'codex',
          rootPath: '/Users/test/.ad/codex-homes/project',
          projectPath: '/Users/test/project',
          baseInstallationId: 'codex:default',
        },
      ]),
    });
    useUiState.setState({ activeProjectPath: '/Users/test/project' });
    listAgentOperationHistory.mockResolvedValue([
      {
        createdAt: '2026-07-15T02:00:00Z',
        receipt: {
          schemaVersion: 2,
          id: 'runtime-receipt',
          planId: 'runtime-plan',
          operationKind: 'apply',
          context: {
            installationId: 'codex:runtime',
            projectPath: '/Users/test/project',
          },
          status: 'complete',
          appliedResources: [
            {
              installationId: 'codex:runtime',
              projectPath: '/Users/test/project',
              kind: 'settings',
              scope: 'project',
              logicalId: 'runtime-config',
            },
          ],
          backupPaths: [],
          postApplyStates: [],
          rollback: { available: true },
          createdAt: '2026-07-15T02:00:00Z',
        },
      },
    ]);

    render(<HistoryPanel />);

    expect(await screen.findByText('runtime-config')).toBeInTheDocument();
    expect(listAgentOperationHistory).toHaveBeenCalledWith(
      'codex:default',
      50,
      '/Users/test/project',
    );
  });
});

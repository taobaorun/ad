import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { HistoryPanel } from '@/components/HistoryPanel';
import i18n from '@/i18n';
import { AgentContextSchema } from '@/lib/agentTypes';
import { useAgents } from '@/store/agents';

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
    });
    listAgentOperationHistory.mockReset().mockResolvedValue([
      {
        createdAt: '2026-07-15T01:00:00Z',
        receipt: {
          id: 'receipt-1',
          planId: 'plan-1',
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
        },
      },
    ]);
    rollbackAgentReceipt.mockReset().mockResolvedValue({
      id: 'rollback-1',
      planId: 'rollback-plan',
      status: 'complete',
      appliedResources: [],
      backupPaths: [],
      postApplyStates: [],
    });
    readHistory.mockReset().mockResolvedValue([]);
    vi.spyOn(window, 'confirm').mockReturnValue(true);
  });

  it('shows operation receipts and rolls them back by receipt id', async () => {
    render(<HistoryPanel />);

    expect(await screen.findByText('user-config')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Rollback' }));

    await waitFor(() => expect(rollbackAgentReceipt).toHaveBeenCalledWith('receipt-1', true));
  });
});

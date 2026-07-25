import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentProfilesButton } from '@/components/AgentProfilesDialog';
import i18n from '@/i18n';
import { AgentContextSchema } from '@/lib/agentTypes';
import { AgentProfileSchema } from '@/lib/profileSchema';
import { useAgents } from '@/store/agents';

const {
  listProfileEnvelopes,
  saveProfileEnvelope,
  deleteProfileEnvelope,
  previewAgentProfileApply,
  applyAgentPlan,
  rollbackAgentReceipt,
} = vi.hoisted(() => ({
  listProfileEnvelopes: vi.fn(),
  saveProfileEnvelope: vi.fn(),
  deleteProfileEnvelope: vi.fn(),
  previewAgentProfileApply: vi.fn(),
  applyAgentPlan: vi.fn(),
  rollbackAgentReceipt: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    listProfileEnvelopes,
    saveProfileEnvelope,
    deleteProfileEnvelope,
    previewAgentProfileApply,
    applyAgentPlan,
    rollbackAgentReceipt,
  },
}));

vi.mock('@/components/JsonEditor', () => ({
  JsonEditor: ({ value, onChange }: { value: string; onChange: (next: string) => void }) => (
    <textarea
      aria-label="Profile payload"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const profile = AgentProfileSchema.parse({
  schemaVersion: 1,
  key: { agentId: 'codex', profileId: 'default' },
  metadata: {
    displayName: 'Codex Default',
    description: null,
    color: '#7C3AED',
    createdAt: '2026-07-15T01:00:00Z',
    updatedAt: '2026-07-15T01:00:00Z',
  },
  payloadSchema: 'ad.profile/codex.v1',
  payload: { configToml: 'model = "gpt-5.4"\n' },
});

const claudeProfile = AgentProfileSchema.parse({
  ...profile,
  key: { agentId: 'claude-code', profileId: 'default' },
  metadata: { ...profile.metadata, displayName: 'Claude Default' },
  payloadSchema: 'ad.profile/claude-code.v2',
  payload: { layers: { env: {} }, settings: { model: 'opus', env: {} } },
});

describe('AgentProfilesButton', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    useAgents.setState({
      activeAgentId: 'codex',
      activeContext: AgentContextSchema.parse({ installationId: 'codex:default' }),
    });
    listProfileEnvelopes.mockReset().mockResolvedValue([profile]);
    saveProfileEnvelope.mockReset().mockImplementation(async (next) => next);
    deleteProfileEnvelope.mockReset();
    previewAgentProfileApply.mockReset().mockResolvedValue({
      id: 'profile-plan',
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
    });
    applyAgentPlan.mockReset().mockResolvedValue({
      id: 'profile-receipt',
      planId: 'profile-plan',
      status: 'complete',
      appliedResources: [],
      backupPaths: ['/tmp/profile-backup'],
      postApplyStates: [],
    });
    rollbackAgentReceipt.mockReset().mockResolvedValue({
      id: 'profile-rollback-receipt',
      planId: 'profile-plan',
      status: 'compensated',
      appliedResources: [],
      backupPaths: [],
      postApplyStates: [],
    });
  });

  it('edits an adapter-owned profile payload through the registry', async () => {
    render(<AgentProfilesButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Manage profiles' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Codex Default' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Profile payload' }), {
      target: { value: 'model = "gpt-5.5"\n' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save profile' }));

    await waitFor(() => expect(saveProfileEnvelope).toHaveBeenCalledTimes(1));
    expect(saveProfileEnvelope.mock.calls[0]?.[0].payload).toEqual({
      configToml: 'model = "gpt-5.5"\n',
    });
    expect(saveProfileEnvelope.mock.calls[0]?.[0].metadata.updatedAt).toBe('2026-07-15T01:00:00Z');

    fireEvent.click(screen.getByRole('button', { name: 'Preview profile apply' }));
    await waitFor(() =>
      expect(previewAgentProfileApply).toHaveBeenCalledWith(
        { installationId: 'codex:default' },
        'default',
      ),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(applyAgentPlan).toHaveBeenCalledWith('profile-plan'));

    fireEvent.click(await screen.findByRole('button', { name: 'Rollback profile apply' }));
    await waitFor(() => expect(rollbackAgentReceipt).toHaveBeenCalledWith('profile-receipt', true));
  });

  it("deletes the displayed draft through the draft's owning Agent", async () => {
    listProfileEnvelopes.mockResolvedValueOnce([claudeProfile]);
    render(<AgentProfilesButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Manage profiles' }));
    await screen.findByRole('button', { name: 'Claude Default' });
    fireEvent.click(await screen.findByRole('button', { name: 'Delete' }));

    await waitFor(() =>
      expect(deleteProfileEnvelope).toHaveBeenCalledWith('claude-code', 'default'),
    );
  });
});

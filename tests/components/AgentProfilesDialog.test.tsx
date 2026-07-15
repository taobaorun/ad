import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentProfilesButton } from '@/components/AgentProfilesDialog';
import i18n from '@/i18n';
import { AgentProfileSchema } from '@/lib/profileSchema';
import { useAgents } from '@/store/agents';

const { listProfileEnvelopes, saveProfileEnvelope, deleteProfileEnvelope } = vi.hoisted(() => ({
  listProfileEnvelopes: vi.fn(),
  saveProfileEnvelope: vi.fn(),
  deleteProfileEnvelope: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    listProfileEnvelopes,
    saveProfileEnvelope,
    deleteProfileEnvelope,
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

describe('AgentProfilesButton', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    useAgents.setState({ activeAgentId: 'codex' });
    listProfileEnvelopes.mockReset().mockResolvedValue([profile]);
    saveProfileEnvelope.mockReset().mockImplementation(async (next) => next);
    deleteProfileEnvelope.mockReset();
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
  });
});

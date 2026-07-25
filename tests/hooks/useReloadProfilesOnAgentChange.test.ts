import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useReloadProfilesOnAgentChange } from '@/hooks/useReloadProfilesOnAgentChange';
import { useAgents } from '@/store/agents';
import { useProfiles } from '@/store/profiles';

const originalLoadProfiles = useProfiles.getState().loadAll;

describe('useReloadProfilesOnAgentChange', () => {
  beforeEach(() => {
    act(() => {
      useAgents.setState({ activeAgentId: 'claude-code' });
      useProfiles.setState({ agentId: 'claude-code', loadAll: originalLoadProfiles });
    });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    act(() => {
      useAgents.setState({ activeAgentId: 'claude-code' });
      useProfiles.setState({ agentId: 'claude-code', loadAll: originalLoadProfiles });
    });
  });

  it('skips reload when Profiles already match the active Agent', () => {
    const reloadProfiles = vi.fn().mockResolvedValue(undefined);
    useProfiles.setState({ loadAll: reloadProfiles });

    renderHook(() => useReloadProfilesOnAgentChange());

    expect(reloadProfiles).not.toHaveBeenCalled();
  });

  it('reloads Profiles once when the active Agent changes', async () => {
    const reloadProfiles = vi.fn().mockResolvedValue(undefined);
    useProfiles.setState({ loadAll: reloadProfiles });
    const { unmount } = renderHook(() => useReloadProfilesOnAgentChange());

    act(() => useAgents.setState({ activeAgentId: 'codex' }));

    await waitFor(() => expect(reloadProfiles).toHaveBeenCalledTimes(1));
    unmount();
  });

  it('handles a rejected reload without an unhandled promise', async () => {
    const failure = new Error('Profile read failed');
    const reloadProfiles = vi.fn().mockRejectedValue(failure);
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    act(() => {
      useAgents.setState({ activeAgentId: 'codex' });
      useProfiles.setState({ agentId: 'claude-code', loadAll: reloadProfiles });
    });

    renderHook(() => useReloadProfilesOnAgentChange());

    await waitFor(() =>
      expect(consoleError).toHaveBeenCalledWith(
        'Failed to reload profiles after Agent change',
        failure,
      ),
    );
  });
});

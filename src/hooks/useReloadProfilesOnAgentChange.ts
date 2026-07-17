import { useEffect } from 'react';
import { useAgents } from '@/store/agents';
import { useProfiles } from '@/store/profiles';

export function useReloadProfilesOnAgentChange(): void {
  const activeAgentId = useAgents((state) => state.activeAgentId);
  const profilesAgentId = useProfiles((state) => state.agentId);
  const reloadProfiles = useProfiles((state) => state.loadAll);

  useEffect(() => {
    if (activeAgentId === profilesAgentId) return;
    void reloadProfiles().catch((error: unknown) => {
      console.error('Failed to reload profiles after Agent change', error);
    });
  }, [activeAgentId, profilesAgentId, reloadProfiles]);
}

import { create } from 'zustand';
import { tauri } from '@/lib/tauri';
import type { AgentInstallation, AgentMetadata } from '@/lib/agentTypes';

const STORAGE_KEY = 'ad.agent-state.v1';
const DEFAULT_AGENT_ID = 'claude-code';

interface State {
  agents: AgentMetadata[];
  installations: AgentInstallation[];
  activeAgentId: string;
  loading: boolean;
  loadAll: () => Promise<void>;
  select: (agentId: string) => void;
}

function loadSelectedAgent(): string {
  try {
    return window.localStorage.getItem(STORAGE_KEY) || DEFAULT_AGENT_ID;
  } catch {
    return DEFAULT_AGENT_ID;
  }
}

function saveSelectedAgent(agentId: string): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, agentId);
  } catch {
    // Ignore storage failures; the current selection remains in memory.
  }
}

let inflightLoadAll: Promise<void> | null = null;

export const useAgents = create<State>((set, get) => ({
  agents: [],
  installations: [],
  activeAgentId: loadSelectedAgent(),
  loading: false,

  loadAll: async () => {
    if (inflightLoadAll) return inflightLoadAll;
    const promise = (async () => {
      set({ loading: true });
      try {
        const [agents, installations] = await Promise.all([
          tauri.listAgents(),
          tauri.discoverAgents(),
        ]);
        const selected = agents.some((agent) => agent.id === get().activeAgentId)
          ? get().activeAgentId
          : agents[0]?.id ?? DEFAULT_AGENT_ID;
        set({ agents, installations, activeAgentId: selected });
        saveSelectedAgent(selected);
      } finally {
        set({ loading: false });
        inflightLoadAll = null;
      }
    })();
    inflightLoadAll = promise;
    return promise;
  },

  select: (agentId) => {
    if (!get().agents.some((agent) => agent.id === agentId)) return;
    set({ activeAgentId: agentId });
    saveSelectedAgent(agentId);
  },
}));

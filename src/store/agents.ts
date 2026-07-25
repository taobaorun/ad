import { create } from 'zustand';
import { tauri } from '@/lib/tauri';
import {
  AgentContextSchema,
  AgentIdSchema,
  InstallationIdSchema,
  type AgentContext,
  type CapabilityDescriptor,
  type AgentInstallation,
  type AgentMetadata,
} from '@/lib/agentTypes';
import { z } from 'zod';

const LEGACY_STORAGE_KEY = 'ad.agent-state.v1';
const STORAGE_KEY = 'ad.agent-context.v2';
const DEFAULT_AGENT_ID = 'claude-code';

const PersistedContextSchema = z
  .object({
    agentId: AgentIdSchema,
    installationId: InstallationIdSchema,
    projectPath: z.string().min(1).optional(),
  })
  .strict();

type ContextInput = z.input<typeof AgentContextSchema>;

interface State {
  agents: AgentMetadata[];
  installations: AgentInstallation[];
  activeContext: AgentContext | null;
  activeAgentId: string;
  capabilitiesByAgent: Record<string, CapabilityDescriptor[]>;
  activeCapabilities: CapabilityDescriptor[];
  loading: boolean;
  loadAll: () => Promise<void>;
  select: (agentId: string) => void;
  selectContext: (context: ContextInput) => void;
}

function loadPersistedSelection(): { agentId: string; context: AgentContext | null } {
  try {
    const current = window.localStorage.getItem(STORAGE_KEY);
    if (current) {
      const parsed = PersistedContextSchema.safeParse(JSON.parse(current));
      if (parsed.success) {
        return {
          agentId: parsed.data.agentId,
          context: AgentContextSchema.parse({
            installationId: parsed.data.installationId,
            projectPath: parsed.data.projectPath,
          }),
        };
      }
    }
    return {
      agentId: window.localStorage.getItem(LEGACY_STORAGE_KEY) || DEFAULT_AGENT_ID,
      context: null,
    };
  } catch {
    return { agentId: DEFAULT_AGENT_ID, context: null };
  }
}

function saveSelectedContext(agentId: string, context: AgentContext): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ agentId, ...context }));
    window.localStorage.removeItem(LEGACY_STORAGE_KEY);
  } catch {
    // Ignore storage failures; the current selection remains in memory.
  }
}

let inflightLoadAll: Promise<void> | null = null;

export const useAgents = create<State>((set, get) => ({
  agents: [],
  installations: [],
  activeContext: null,
  activeAgentId: DEFAULT_AGENT_ID,
  capabilitiesByAgent: {},
  activeCapabilities: [],
  loading: false,

  loadAll: async () => {
    if (inflightLoadAll) return inflightLoadAll;
    const promise = (async () => {
      set({ loading: true });
      try {
        const agents = await tauri.listAgents();
        const [installations, descriptorEntries] = await Promise.all([
          tauri.discoverAgents(),
          Promise.all(
            agents.map(
              async (agent) => [agent.id, await tauri.listAgentCapabilities(agent.id)] as const,
            ),
          ),
        ]);
        const capabilitiesByAgent = Object.fromEntries(descriptorEntries);
        const persisted = loadPersistedSelection();
        const persistedInstallation = persisted.context
          ? installations.find(
              (installation) =>
                installation.id === persisted.context?.installationId &&
                installation.agentId === persisted.agentId,
            )
          : undefined;
        const selectedInstallation =
          persistedInstallation ??
          installations.find((installation) => installation.agentId === persisted.agentId) ??
          installations[0];
        const activeAgentId = selectedInstallation?.agentId ?? agents[0]?.id ?? DEFAULT_AGENT_ID;
        const activeContext = selectedInstallation
          ? AgentContextSchema.parse({
              installationId: selectedInstallation.id,
              projectPath:
                selectedInstallation.projectPath ??
                (persistedInstallation ? persisted.context?.projectPath : undefined),
            })
          : null;
        set({
          agents,
          installations,
          activeAgentId,
          activeContext,
          capabilitiesByAgent,
          activeCapabilities: capabilitiesByAgent[activeAgentId] ?? [],
        });
        if (activeContext) saveSelectedContext(activeAgentId, activeContext);
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
    const installation = get().installations.find((item) => item.agentId === agentId);
    const activeContext = installation
      ? AgentContextSchema.parse({
          installationId: installation.id,
          projectPath: installation.projectPath,
        })
      : null;
    set({
      activeAgentId: agentId,
      activeContext,
      activeCapabilities: get().capabilitiesByAgent[agentId] ?? [],
    });
    if (activeContext) saveSelectedContext(agentId, activeContext);
  },

  selectContext: (input) => {
    const parsed = AgentContextSchema.safeParse(input);
    if (!parsed.success) return;
    const installation = get().installations.find((item) => item.id === parsed.data.installationId);
    if (!installation) return;
    if (
      installation.projectPath &&
      parsed.data.projectPath &&
      installation.projectPath !== parsed.data.projectPath
    ) {
      return;
    }
    const activeContext = AgentContextSchema.parse({
      installationId: installation.id,
      projectPath: installation.projectPath ?? parsed.data.projectPath,
    });
    set({
      activeAgentId: installation.agentId,
      activeContext,
      activeCapabilities: get().capabilitiesByAgent[installation.agentId] ?? [],
    });
    saveSelectedContext(installation.agentId, activeContext);
  },
}));

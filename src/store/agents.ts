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
import { requestAgentWorkspaceChange } from '@/lib/workspaceDraftGuard';

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
  selectContext: (context: ContextInput) => void;
}

function isBaseInstallation(installation: AgentInstallation): boolean {
  return !installation.projectPath && !installation.baseInstallationId;
}

function resolveBaseInstallation(
  installations: AgentInstallation[],
  installation: AgentInstallation,
): AgentInstallation | undefined {
  if (isBaseInstallation(installation)) return installation;
  if (!installation.baseInstallationId) return undefined;
  return installations.find(
    (candidate) =>
      candidate.id === installation.baseInstallationId && isBaseInstallation(candidate),
  );
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
        const persistedCandidate = persisted.context
          ? installations.find(
              (installation) =>
                installation.id === persisted.context?.installationId &&
                installation.agentId === persisted.agentId,
            )
          : undefined;
        const persistedInstallation = persistedCandidate
          ? resolveBaseInstallation(installations, persistedCandidate)
          : undefined;
        const selectedInstallation =
          persistedInstallation ??
          installations.find(
            (installation) =>
              installation.agentId === persisted.agentId && isBaseInstallation(installation),
          ) ??
          installations.find(isBaseInstallation);
        const activeAgentId = selectedInstallation?.agentId ?? agents[0]?.id ?? DEFAULT_AGENT_ID;
        const activeContext = selectedInstallation
          ? AgentContextSchema.parse({
              installationId: selectedInstallation.id,
              projectPath:
                persistedCandidate?.id === selectedInstallation.id
                  ? persisted.context?.projectPath
                  : undefined,
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

  selectContext: (input) => {
    const parsed = AgentContextSchema.safeParse(input);
    if (!parsed.success) return;
    const candidate = get().installations.find((item) => item.id === parsed.data.installationId);
    if (!candidate) return;
    const installation = resolveBaseInstallation(get().installations, candidate);
    if (!installation) return;
    const activeContext = AgentContextSchema.parse({
      installationId: installation.id,
      projectPath: candidate.id === installation.id ? parsed.data.projectPath : undefined,
    });
    if (
      JSON.stringify(activeContext) !== JSON.stringify(get().activeContext) &&
      !requestAgentWorkspaceChange()
    ) {
      return;
    }
    set({
      activeAgentId: installation.agentId,
      activeContext,
      activeCapabilities: get().capabilitiesByAgent[installation.agentId] ?? [],
    });
    saveSelectedContext(installation.agentId, activeContext);
  },
}));

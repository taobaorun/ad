import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useAgents } from '@/store/agents';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === 'list_agents') {
      return [
        { id: 'claude-code', displayName: 'Claude Code', capabilities: ['settings'] },
        { id: 'codex', displayName: 'Codex', capabilities: ['settings'] },
      ];
    }
    if (cmd === 'discover_agents') {
      return [
        {
          id: 'claude-code:/Users/test/.claude',
          agentId: 'claude-code',
          rootPath: '/Users/test/.claude',
        },
        {
          id: 'codex:/Users/test/.codex',
          agentId: 'codex',
          rootPath: '/Users/test/.codex',
        },
      ];
    }
    if (cmd === 'list_agent_capabilities') {
      return [
        {
          kind: 'settings',
          scopes: ['user', 'project'],
          operations: ['inspect', 'edit', 'preview', 'apply', 'rollback'],
          availability: 'available',
          limitations: [],
        },
      ];
    }
    throw new Error(`unmocked: ${cmd}`);
  }),
}));

describe('useAgents', () => {
  beforeEach(() => {
    window.localStorage.clear();
    useAgents.setState({
      agents: [],
      installations: [],
      activeContext: null,
      activeAgentId: 'claude-code',
      capabilitiesByAgent: {},
      loading: false,
    });
  });

  it('loads built-in agents and canonical installations', async () => {
    await useAgents.getState().loadAll();

    expect(useAgents.getState().agents.map((agent) => agent.id)).toEqual(['claude-code', 'codex']);
    expect(useAgents.getState().installations).toHaveLength(2);
    expect(useAgents.getState().activeContext).toEqual({
      installationId: 'claude-code:/Users/test/.claude',
    });
    expect(useAgents.getState().activeCapabilities).toEqual([
      expect.objectContaining({ kind: 'settings', availability: 'available' }),
    ]);
  });

  it('migrates a legacy Agent id to a canonical installation context', async () => {
    window.localStorage.setItem('ad.agent-state.v1', 'codex');

    await useAgents.getState().loadAll();

    expect(useAgents.getState().activeAgentId).toBe('codex');
    expect(useAgents.getState().activeContext).toEqual({
      installationId: 'codex:/Users/test/.codex',
    });
    expect(JSON.parse(window.localStorage.getItem('ad.agent-context.v2') ?? 'null')).toEqual({
      agentId: 'codex',
      installationId: 'codex:/Users/test/.codex',
    });
  });

  it('persists an installation and optional project context', async () => {
    await useAgents.getState().loadAll();
    useAgents.getState().selectContext({
      installationId: 'codex:/Users/test/.codex',
      projectPath: '/Users/test/project',
    });

    expect(useAgents.getState().activeAgentId).toBe('codex');
    expect(useAgents.getState().activeContext).toEqual({
      installationId: 'codex:/Users/test/.codex',
      projectPath: '/Users/test/project',
    });

    useAgents.getState().selectContext({ installationId: 'unknown' });
    expect(useAgents.getState().activeAgentId).toBe('codex');
  });

  it('restores a persisted v2 context on the next load', async () => {
    window.localStorage.setItem(
      'ad.agent-context.v2',
      JSON.stringify({
        agentId: 'codex',
        installationId: 'codex:/Users/test/.codex',
        projectPath: '/Users/test/project',
      }),
    );

    await useAgents.getState().loadAll();

    expect(useAgents.getState().activeContext).toEqual({
      installationId: 'codex:/Users/test/.codex',
      projectPath: '/Users/test/project',
    });
  });
});

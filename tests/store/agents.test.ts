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
      return [{ agentId: 'codex', rootPath: '/Users/test/.codex' }];
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
      activeAgentId: 'claude-code',
      loading: false,
    });
  });

  it('loads built-in agents and canonical installations', async () => {
    await useAgents.getState().loadAll();

    expect(useAgents.getState().agents.map((agent) => agent.id)).toEqual([
      'claude-code',
      'codex',
    ]);
    expect(useAgents.getState().installations).toHaveLength(1);
  });

  it('persists a valid Agent selection and ignores unknown ids', async () => {
    await useAgents.getState().loadAll();
    useAgents.getState().select('codex');
    expect(useAgents.getState().activeAgentId).toBe('codex');
    expect(window.localStorage.getItem('ad.agent-state.v1')).toBe('codex');

    useAgents.getState().select('unknown');
    expect(useAgents.getState().activeAgentId).toBe('codex');
  });
});

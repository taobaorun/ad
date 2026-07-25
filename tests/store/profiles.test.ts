import { describe, expect, it, vi, beforeEach } from 'vitest';
import { useProfiles } from '@/store/profiles';
import type { ProfileFile } from '@/lib/profileSchema';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    const fixture: ProfileFile = {
      agentId: 'claude-code',
      id: 'homi',
      displayName: 'Homi',
      description: null,
      color: '#7C3AED',
      createdAt: '2026-05-13T22:55:00Z',
      updatedAt: '2026-05-13T22:55:00Z',
      layers: { env: {} },
      settings: { env: { ANTHROPIC_MODEL: 'GLM-5.1' } },
    };
    switch (cmd) {
      case 'list_profiles':
        return [fixture];
      case 'list_agent_profiles':
        return [fixture];
      case 'get_active_profile_id':
        return 'homi';
      case 'save_profile':
        return args?.profile;
      case 'save_agent_profile':
        return args?.profile;
      case 'delete_profile':
        return undefined;
      case 'delete_agent_profile':
        return undefined;
      case 'activate_profile':
        return { activatedId: (args as { id: string }).id, backupPath: null, detectedPids: [] };
      default:
        throw new Error(`unmocked: ${cmd}`);
    }
  }),
}));

describe('useProfiles store', () => {
  beforeEach(() => {
    useProfiles.setState({
      profiles: [],
      activeId: null,
      selectedId: null,
      view: 'editor',
      importOpen: false,
      toasts: [],
      loading: false,
      agentId: 'claude-code',
    });
  });

  it('loads profiles + active id on loadAll', async () => {
    await useProfiles.getState().loadAll();
    const s = useProfiles.getState();
    expect(s.profiles).toHaveLength(1);
    expect(s.activeId).toBe('homi');
    expect(s.selectedId).toBe('homi');
  });

  it('activate updates activeId and emits a toast', async () => {
    await useProfiles.getState().loadAll();
    await useProfiles.getState().activate('homi');
    const s = useProfiles.getState();
    expect(s.activeId).toBe('homi');
    expect(s.toasts).toHaveLength(1);
    expect(s.toasts[0]?.profileName).toBe('Homi');
  });
});

import { describe, expect, it } from 'vitest';
import {
  ProfileFileSchema,
  parseProfileFile,
  blankProfile,
  settingsFromLayers,
  AgentProfileSchema,
  profileFileToAgentProfile,
  type ProfileLayers,
} from '@/lib/profileSchema';

describe('ProfileFileSchema', () => {
  it('accepts a fresh blank profile', () => {
    const p = blankProfile('demo');
    expect(ProfileFileSchema.parse(p)).toEqual(p);
  });

  it('accepts the Rust sample fixture shape', () => {
    const sample = {
      id: 'sample',
      displayName: 'Sample',
      description: 'Schema parity fixture',
      color: '#7C3AED',
      createdAt: '2026-05-13T22:55:00Z',
      updatedAt: '2026-05-13T22:55:00Z',
      settings: {
        env: { ANTHROPIC_BASE_URL: 'https://api.anthropic.com' },
        model: 'claude-opus-4-7',
        theme: 'dark',
      },
    };
    const r = ProfileFileSchema.safeParse(sample);
    expect(r.success).toBe(true);
  });

  it('rejects malformed id', () => {
    const r = parseProfileFile({
      id: '../escape',
      displayName: 'X',
      color: '#000000',
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      settings: { env: {} },
    });
    expect(r.ok).toBe(false);
  });

  it('rejects malformed color', () => {
    const r = parseProfileFile({
      ...blankProfile('x'),
      color: 'red',
    });
    expect(r.ok).toBe(false);
  });

  it('preserves unknown settings keys via passthrough', () => {
    const result = ProfileFileSchema.safeParse({
      ...blankProfile('x'),
      settings: { env: { K: 'V' }, futureKey: { nested: true } },
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect((result.data.settings as Record<string, unknown>).futureKey).toEqual({
        nested: true,
      });
    }
  });

  it('parses v0.1 profiles without layers (backward compat)', () => {
    const v1 = {
      id: 'old',
      displayName: 'Old',
      color: '#7C3AED',
      createdAt: '2026-01-01T00:00:00Z',
      updatedAt: '2026-01-01T00:00:00Z',
      settings: { env: { K: 'V' } },
      // No `layers` field — should default to { env: {} }.
    };
    const r = ProfileFileSchema.safeParse(v1);
    expect(r.success).toBe(true);
    if (r.success) {
      expect(r.data.layers).toEqual({ env: {} });
      expect(r.data.agentId).toBe('claude-code');
    }
  });

  it('keeps profile identity scoped to an Agent', () => {
    const r = ProfileFileSchema.parse({
      ...blankProfile('default'),
      agentId: 'codex',
    });

    expect(r.agentId).toBe('codex');
  });

  it('parses v0.2 profiles with layers populated', () => {
    const v2 = {
      ...blankProfile('layered'),
      layers: {
        shared: { permissions: { allow: ['fs:read'] } },
        local: { model: 'claude-opus-4-7' },
        env: { ANTHROPIC_API_KEY: 'sk-test' },
      },
    };
    const r = ProfileFileSchema.safeParse(v2);
    expect(r.success).toBe(true);
    if (r.success) {
      expect(r.data.layers.local).toEqual({ model: 'claude-opus-4-7' });
      expect(r.data.layers.env).toEqual({ ANTHROPIC_API_KEY: 'sk-test' });
    }
  });
});

describe('AgentProfileSchema', () => {
  it('maps a legacy Claude profile into the stable envelope', () => {
    const legacy = blankProfile('default');
    const profile = profileFileToAgentProfile(legacy);

    expect(AgentProfileSchema.parse(profile)).toEqual(profile);
    expect(profile.key).toEqual({ agentId: 'claude-code', profileId: 'default' });
    expect(profile.payloadSchema).toBe('ad.profile/claude-code.v2');
    expect(profile.payload).toEqual({ layers: legacy.layers, settings: legacy.settings });
  });

  it('accepts a Codex TOML payload without Claude fields', () => {
    const now = new Date().toISOString();
    const profile = AgentProfileSchema.parse({
      schemaVersion: 1,
      key: { agentId: 'codex', profileId: 'default' },
      metadata: {
        displayName: 'Default',
        description: null,
        color: '#7C3AED',
        createdAt: now,
        updatedAt: now,
      },
      payloadSchema: 'ad.profile/codex.v1',
      payload: { configToml: 'model = "gpt-5.4"\n' },
    });

    expect(profile.key.agentId).toBe('codex');
    expect(profile.payload).toEqual({ configToml: 'model = "gpt-5.4"\n' });
    expect(profile.payload).not.toHaveProperty('settings');
    expect(profile.payload).not.toHaveProperty('layers');
  });

  it('rejects an Agent/schema mismatch', () => {
    const legacy = profileFileToAgentProfile(blankProfile('default'));
    const result = AgentProfileSchema.safeParse({
      ...legacy,
      key: { ...legacy.key, agentId: 'codex' },
    });

    expect(result.success).toBe(false);
  });
});

describe('settingsFromLayers', () => {
  it('returns empty env-only settings for empty layers', () => {
    const layers: ProfileLayers = { env: {} };
    expect(settingsFromLayers(layers)).toEqual({ env: {} });
  });

  it('local overrides shared on shared keys', () => {
    const layers: ProfileLayers = {
      shared: { model: 'sonnet', theme: 'dark' },
      local: { model: 'opus' },
      env: {},
    };
    const result = settingsFromLayers(layers);
    expect(result.model).toBe('opus');
    expect((result as { theme?: string }).theme).toBe('dark');
  });

  it('env layer is folded into the env field, layer wins on key conflict', () => {
    const layers: ProfileLayers = {
      local: { env: { A: 'from-local', B: 'from-local' } },
      env: { A: 'from-env-layer', C: 'from-env-layer' },
    };
    const result = settingsFromLayers(layers);
    expect(result.env).toEqual({
      A: 'from-env-layer',
      B: 'from-local',
      C: 'from-env-layer',
    });
  });

  it('preserves unknown keys (enabledPlugins, statusLine, etc.)', () => {
    const layers: ProfileLayers = {
      local: {
        enabledPlugins: { 'a@b': true },
        statusLine: { command: '~/x.sh' },
      },
      env: {},
    };
    const result = settingsFromLayers(layers);
    expect((result as Record<string, unknown>).enabledPlugins).toEqual({ 'a@b': true });
    expect((result as Record<string, unknown>).statusLine).toEqual({ command: '~/x.sh' });
  });
});

import { describe, expect, it } from 'vitest';

import {
  createBlankAgentProfile,
  profileFeaturesFor,
  profileEditorFor,
} from '@/lib/profileEditorRegistry';

describe('profileEditorRegistry', () => {
  it('creates adapter-owned payloads for the same profile id', () => {
    const claude = createBlankAgentProfile('claude-code', 'default');
    const codex = createBlankAgentProfile('codex', 'default');

    expect(claude.key).toEqual({ agentId: 'claude-code', profileId: 'default' });
    expect(claude.payloadSchema).toBe('ad.profile/claude-code.v2');
    expect(codex.key).toEqual({ agentId: 'codex', profileId: 'default' });
    expect(codex.payloadSchema).toBe('ad.profile/codex.v1');
  });

  it('serializes and parses payloads through one registry lookup', () => {
    const profile = createBlankAgentProfile('codex', 'default');
    const editor = profileEditorFor(profile.payloadSchema);

    expect(editor.language).toBe('text');
    expect(editor.serialize(profile)).toBe('');
    expect(editor.update(profile, 'model = "gpt-5.4"\n').payload).toEqual({
      configToml: 'model = "gpt-5.4"\n',
    });
  });

  it('keeps legacy project-template behavior in the centralized registry', () => {
    expect(profileFeaturesFor('claude-code').legacyProjectTemplates).toBe(true);
    expect(profileFeaturesFor('codex').legacyProjectTemplates).toBe(false);
  });
});

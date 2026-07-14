import { describe, expect, it } from 'vitest';
import {
  AgentInstallationSchema,
  AgentProfileRefSchema,
  CapabilitySchema,
  parseAgentInstallation,
} from '@/lib/agentTypes';

describe('Agent schemas', () => {
  it('accepts a canonical installation at the IPC boundary', () => {
    expect(
      AgentInstallationSchema.parse({
        agentId: 'codex',
        rootPath: '/Users/test/.codex',
      }),
    ).toEqual({
      agentId: 'codex',
      rootPath: '/Users/test/.codex',
    });
  });

  it('keeps profile identity composite across Agents', () => {
    const claude = AgentProfileRefSchema.parse({ agentId: 'claude-code', profileId: 'default' });
    const codex = AgentProfileRefSchema.parse({ agentId: 'codex', profileId: 'default' });

    expect(claude).not.toEqual(codex);
  });

  it('uses the same capability names as the Rust boundary', () => {
    expect(CapabilitySchema.parse('process_detection')).toBe('process_detection');
  });

  it('returns validation issues for an empty Agent id', () => {
    const result = parseAgentInstallation({ agentId: '', rootPath: '/tmp/agent' });

    expect(result.ok).toBe(false);
  });
});

import { describe, expect, it } from 'vitest';
import { ProfileFileSchema, parseProfileFile, blankProfile } from '@/lib/profileSchema';

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
});

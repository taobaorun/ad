import { describe, expect, it } from 'vitest';

import { SkillSourcePreviewProgressSchema } from '@/lib/skillCatalogTypes';

describe('Skill source preview progress', () => {
  it('accepts only backend-owned phases with an ordered sequence', () => {
    expect(SkillSourcePreviewProgressSchema.parse({ sequence: 2, phase: 'cloning' })).toEqual({
      sequence: 2,
      phase: 'cloning',
    });
    expect(() =>
      SkillSourcePreviewProgressSchema.parse({
        sequence: 0,
        phase: 'cloning',
        percent: 50,
      }),
    ).toThrow();
  });
});

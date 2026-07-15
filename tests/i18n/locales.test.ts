import { describe, expect, it } from 'vitest';

import en from '@/i18n/locales/en.json';
import zh from '@/i18n/locales/zh.json';

describe('i18n locales', () => {
  it('keeps English and Chinese translation keys in sync', () => {
    expect(leafKeys(en)).toEqual(leafKeys(zh));
  });
});

function leafKeys(value: unknown, prefix = ''): string[] {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return [prefix];
  }

  return Object.entries(value)
    .flatMap(([key, child]) => leafKeys(child, prefix ? `${prefix}.${key}` : key))
    .sort();
}

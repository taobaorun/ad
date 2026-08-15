import { describe, expect, it } from 'vitest';

import en from '@/i18n/locales/en.json';
import zh from '@/i18n/locales/zh.json';

describe('i18n locales', () => {
  it('keeps English and Chinese translation keys in sync', () => {
    expect(leafKeys(en)).toEqual(leafKeys(zh));
  });

  it('keeps the exact startup slogan and localized loading status', () => {
    expect(en.startup.loadingQuote).toBe('Be Water, My Friend');
    expect(zh.startup.loadingQuote).toBe('Be Water, My Friend');
    expect(en.startup.loadingStatus).toBe('AD is loading');
    expect(zh.startup.loadingStatus).toBe('正在加载 AD');
  });

  it('names the agent capability workspace Harness in both locales', () => {
    expect(en.sidebar.harness).toBe('Harness');
    expect(zh.sidebar.harness).toBe('Harness');
    expect(en.resourceCenter.title).toBe('Harness');
    expect(zh.resourceCenter.title).toBe('Harness');
    expect(en.resourceCenter.sectionTitle).toBe('Skills & Plugins');
    expect(zh.resourceCenter.sectionTitle).toBe('Skills / Plugins');
    expect(en.resourceCenter.capability.mcpStatus).toBe('Coming soon');
    expect(zh.resourceCenter.capability.mcpStatus).toBe('即将推出');
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

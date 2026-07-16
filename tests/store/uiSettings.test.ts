import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { applyDocumentTheme, THEME_COLORS } from '@/lib/theme';
import { useUiSettings } from '@/store/uiSettings';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => undefined),
}));

describe('UI theme lifecycle', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    document.documentElement.className = '';
    document.documentElement.removeAttribute('style');
    useUiSettings.setState({ darkMode: true });
  });

  it('applies Mocha and Latte roots to the current document', () => {
    applyDocumentTheme(true);
    expect(document.documentElement).toHaveClass('dark');
    expect(document.documentElement.style.backgroundColor).toBe(normalizeColor(THEME_COLORS.mocha.background));
    expect(document.documentElement.style.color).toBe(normalizeColor(THEME_COLORS.mocha.foreground));

    applyDocumentTheme(false);
    expect(document.documentElement).not.toHaveClass('dark');
    expect(document.documentElement.style.backgroundColor).toBe(normalizeColor(THEME_COLORS.latte.background));
    expect(document.documentElement.style.color).toBe(normalizeColor(THEME_COLORS.latte.foreground));
  });

  it('persists a toggle and updates the backend launch hint', () => {
    useUiSettings.getState().setDarkMode(false);

    expect(JSON.parse(window.localStorage.getItem('ad.ui-settings.v1') ?? '{}')).toMatchObject({
      darkMode: false,
    });
    expect(invoke).toHaveBeenCalledWith('write_theme_hint', { dark: false });
  });

  it('hydrates a sibling-window storage update', () => {
    window.dispatchEvent(
      new StorageEvent('storage', {
        key: 'ad.ui-settings.v1',
        newValue: JSON.stringify({ darkMode: false }),
      }),
    );

    expect(useUiSettings.getState().darkMode).toBe(false);
  });
});

function normalizeColor(value: string): string {
  const probe = document.createElement('div');
  probe.style.color = value;
  return probe.style.color;
}

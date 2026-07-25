import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  applyDocumentTheme: vi.fn(),
  coordinateStartup: vi.fn(),
  createRoot: vi.fn(),
  injectStartupCopy: vi.fn(),
  render: vi.fn(),
  revealStartup: vi.fn().mockResolvedValue(undefined),
  showStartupFailure: vi.fn(),
  startStartupSpotlight: vi.fn(),
  syncGlobalShortcutToBackend: vi.fn().mockResolvedValue(undefined),
  writeThemeHint: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('react-dom/client', () => ({
  default: { createRoot: mocks.createRoot },
}));
vi.mock('@/lib/startup', () => ({ coordinateStartup: mocks.coordinateStartup }));
vi.mock('@/lib/startupSurface', () => ({
  injectStartupCopy: mocks.injectStartupCopy,
  revealStartup: mocks.revealStartup,
  showStartupFailure: mocks.showStartupFailure,
  startStartupSpotlight: mocks.startStartupSpotlight,
}));
vi.mock('@/lib/tauri', () => ({ tauri: { writeThemeHint: mocks.writeThemeHint } }));
vi.mock('@/lib/theme', () => ({ applyDocumentTheme: mocks.applyDocumentTheme }));
vi.mock('@/store/uiSettings', () => ({
  useUiSettings: {
    getState: () => ({
      darkMode: true,
      syncGlobalShortcutToBackend: mocks.syncGlobalShortcutToBackend,
    }),
  },
}));
vi.mock('@/App', () => ({ App: () => null }));
vi.mock('@/SettingsApp', () => ({ SettingsApp: () => null }));

describe('application bootstrap', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.createRoot.mockReturnValue({ render: mocks.render });
    mocks.coordinateStartup.mockResolvedValue({ timedOut: false, failures: [] });
    mocks.revealStartup.mockResolvedValue(undefined);
    window.location.hash = '';
    localStorage.setItem('ad.lang.v1', 'en');
    document.body.innerHTML = '<div id="root"></div>';
  });

  it('waits for main-window startup before rendering and revealing', async () => {
    let resolveStartup!: () => void;
    mocks.coordinateStartup.mockReturnValue(
      new Promise((resolve) => {
        resolveStartup = () => resolve({ timedOut: false, failures: [] });
      }),
    );

    await import('@/main');
    await vi.waitFor(() => expect(mocks.coordinateStartup).toHaveBeenCalledTimes(1));
    expect(mocks.render).not.toHaveBeenCalled();

    resolveStartup();

    await vi.waitFor(() => expect(mocks.render).toHaveBeenCalledTimes(1));
    expect(mocks.revealStartup).toHaveBeenCalledWith(document);
    expect(mocks.syncGlobalShortcutToBackend).toHaveBeenCalledTimes(1);
  });

  it('renders Settings without running main-window startup', async () => {
    window.location.hash = '#/settings';

    await import('@/main');

    await vi.waitFor(() => expect(mocks.render).toHaveBeenCalledTimes(1));
    expect(mocks.coordinateStartup).not.toHaveBeenCalled();
    expect(mocks.syncGlobalShortcutToBackend).not.toHaveBeenCalled();
    expect(mocks.revealStartup).toHaveBeenCalledWith(document);
  });

  it('reveals a localized failure surface when bootstrap rejects', async () => {
    const failure = new Error('Application chunk is unavailable');
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    mocks.coordinateStartup.mockRejectedValue(failure);

    await import('@/main');

    await vi.waitFor(() =>
      expect(mocks.showStartupFailure).toHaveBeenCalledWith(document, {
        title: 'AD could not start',
        message:
          'A required application component could not be loaded. Retry after checking that the installation is complete.',
        retry: 'Retry',
      }),
    );
    expect(mocks.createRoot).not.toHaveBeenCalled();
    expect(consoleError).toHaveBeenCalledWith('AD bootstrap failed', failure);
    consoleError.mockRestore();
  });
});

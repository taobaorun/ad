import React from 'react';
import ReactDOM from 'react-dom/client';
import i18n from './i18n';
import { injectStartupCopy, revealStartup, showStartupFailure } from './lib/startupSurface';
import { tauri } from './lib/tauri';
import { applyDocumentTheme } from './lib/theme';
import { useUiSettings } from './store/uiSettings';
import './styles/globals.css';

// Hash routing: the settings window opens the same Vite bundle with #/settings.
// We dynamic-import the top-level component so Vite emits separate chunks for
// App and SettingsApp — the settings window then never downloads the main
// app's editor/codemirror code, and vice versa.
const isSettings = window.location.hash.startsWith('#/settings');

applyDocumentTheme(useUiSettings.getState().darkMode);

// Main window owns global-shortcut registration. The settings window shares
// localStorage and would otherwise double-register on its own boot.
if (!isSettings) {
  void useUiSettings.getState().syncGlobalShortcutToBackend();
}

// Persist theme hint so Rust can read it at next launch and set the correct
// native window background color before the WebView loads.
tauri.writeThemeHint(useUiSettings.getState().darkMode).catch(() => {});

async function bootstrap() {
  injectStartupCopy(document, {
    quote: i18n.t('startup.loadingQuote'),
    status: i18n.t('startup.loadingStatus'),
  });
  if (isSettings) {
    const { SettingsApp } = await import('./SettingsApp');
    const root = ReactDOM.createRoot(document.getElementById('root')!);
    root.render(
      <React.StrictMode>
        <SettingsApp />
      </React.StrictMode>,
    );
    await revealStartup(document);
  } else {
    const [{ App }, startupResult] = await Promise.all([
      import('./App'),
      import('./lib/startup').then(({ coordinateStartup }) => coordinateStartup()),
    ]);
    for (const failure of startupResult.failures) {
      console.error('AD startup task failed', failure);
    }
    const root = ReactDOM.createRoot(document.getElementById('root')!);
    root.render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
    await revealStartup(document);
  }
}

void bootstrap().catch((error: unknown) => {
  console.error('AD bootstrap failed', error);
  showStartupFailure(document, {
    title: i18n.t('startup.failureTitle'),
    message: i18n.t('startup.failureMessage'),
    retry: i18n.t('startup.retry'),
  });
});

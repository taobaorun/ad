import React from 'react';
import ReactDOM from 'react-dom/client';
import './i18n';
import { App } from './App';
import { SettingsApp } from './SettingsApp';
import { useUiSettings } from './store/uiSettings';
import './styles/globals.css';
import './lib/monacoSetup';

const isSettings = window.location.hash.startsWith('#/settings');

// Main window owns global-shortcut registration. The settings window shares
// localStorage and would otherwise double-register on its own boot.
if (!isSettings) {
  void useUiSettings.getState().syncGlobalShortcutToBackend();
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>{isSettings ? <SettingsApp /> : <App />}</React.StrictMode>,
);

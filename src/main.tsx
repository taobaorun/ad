import React from 'react';
import ReactDOM from 'react-dom/client';
import './i18n';
import { App } from './App';
import './styles/globals.css';
import './lib/monacoSetup';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

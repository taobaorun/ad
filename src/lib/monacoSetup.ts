// Self-host Monaco so the strict CSP in tauri.conf.json doesn't block it.
//
// `@monaco-editor/react` defaults to fetching Monaco from a CDN at runtime.
// That breaks under our `connect-src 'self' ipc:` policy. By calling
// `loader.config({ monaco })` we tell the wrapper to use the bundled monaco
// module instead.
//
// Web workers (json/editor) are wired via Vite's `?worker` syntax so Vite
// emits them as URL-imported Worker modules, which respect `worker-src 'self' blob:`.

import { loader } from '@monaco-editor/react';
import * as monaco from 'monaco-editor';
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker';

// MonacoEnvironment must be set on `self` before any Monaco code runs.
self.MonacoEnvironment = {
  getWorker(_workerId, label) {
    if (label === 'json') return new jsonWorker();
    return new editorWorker();
  },
};

loader.config({ monaco });

// Eager-init the loader so the first Editor render doesn't show a "Loading..."
// flash. Returning the promise lets callers `await monacoReady` if needed.
export const monacoReady = loader.init();

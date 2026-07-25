/**
 * Controlled JSON editor backed by CodeMirror 6.
 *
 * Author: taobaorun
 *
 * Wraps CodeMirror's imperative EditorView in a React-friendly { value,
 * onChange } interface so callers (e.g. LayeredSettingsEditor) stay agnostic
 * about the underlying editor. Theme and readOnly use Compartments so we can
 * reconfigure without rebuilding the view.
 *
 * Why not Monaco: the full Monaco bundle is ~3MB and dominates first paint;
 * CodeMirror covers the JSON-editing surface we actually need (highlight,
 * line numbers, history, search, bracket matching) at ~80KB gzip.
 */

import { useEffect, useRef } from 'react';
import { EditorView } from '@codemirror/view';
import { EditorState, Compartment } from '@codemirror/state';
import { json } from '@codemirror/lang-json';
import { codeFolding, foldService } from '@codemirror/language';
import { basicSetup } from 'codemirror';
import { editorThemeFor } from '@/lib/editorTheme';

export interface JsonEditorProps {
  value: string;
  onChange: (next: string) => void;
  dark?: boolean;
  readOnly?: boolean;
  language?: 'json' | 'text';
  className?: string;
}

const bracketFold = foldService.of((state, from, _to) => {
  const line = state.doc.lineAt(from);
  const text = line.text.trimEnd();
  const lastChar = text[text.length - 1];
  if (lastChar !== '{' && lastChar !== '[') return null;
  const close = lastChar === '{' ? '}' : ']';
  let depth = 1;
  const startPos = line.from + text.length;
  for (let pos = startPos; pos < state.doc.length; pos++) {
    const ch = state.doc.sliceString(pos, pos + 1);
    if (ch === lastChar) depth++;
    else if (ch === close) {
      depth--;
      if (depth === 0) return { from: startPos, to: pos };
    }
  }
  return null;
});

const jsonFoldConfig = codeFolding({
  placeholderDOM(_view, onclick) {
    const el = document.createElement('span');
    el.className = 'cm-foldPlaceholder';
    el.textContent = '…';
    el.onclick = onclick;
    el.title = 'Click to unfold';
    return el;
  },
});

const heightTheme = EditorView.theme({
  '&': { height: '100%', fontSize: '13px' },
  '.cm-scroller': {
    overflow: 'auto',
    fontFamily:
      "ui-monospace, 'SF Mono', Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace",
  },
  '.cm-content': { padding: '8px 0' },
});

export function JsonEditor({
  value,
  onChange,
  dark,
  readOnly,
  language = 'json',
  className,
}: JsonEditorProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const themeCompartment = useRef(new Compartment());
  const readOnlyCompartment = useRef(new Compartment());
  // Keep latest onChange in a ref so the EditorView outlives prop identity churn.
  const onChangeRef = useRef(onChange);
  // Snapshot initial dark/readOnly so the mount effect can seed compartments
  // without listing them as deps (which would tear the view down on toggle).
  const initialDarkRef = useRef(!!dark);
  const initialReadOnlyRef = useRef(!!readOnly);
  const initialLanguageRef = useRef(language);

  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    if (!hostRef.current) return;
    const state = EditorState.create({
      doc: value,
      extensions: [
        basicSetup,
        ...(initialLanguageRef.current === 'json' ? [json(), bracketFold, jsonFoldConfig] : []),
        heightTheme,
        themeCompartment.current.of(editorThemeFor(initialDarkRef.current)),
        readOnlyCompartment.current.of(EditorState.readOnly.of(initialReadOnlyRef.current)),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            onChangeRef.current(update.state.doc.toString());
          }
        }),
      ],
    });
    const view = new EditorView({ state, parent: hostRef.current });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // Mount once. value/dark/readOnly are reconciled by the dedicated effects
    // below — re-running this would destroy the view on every keystroke.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // External value -> doc sync. Guard against echoing our own updates back in.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const current = view.state.doc.toString();
    if (current === value) return;
    view.dispatch({
      changes: { from: 0, to: current.length, insert: value },
    });
  }, [value]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: themeCompartment.current.reconfigure(editorThemeFor(!!dark)),
    });
  }, [dark]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: readOnlyCompartment.current.reconfigure(EditorState.readOnly.of(!!readOnly)),
    });
  }, [readOnly]);

  return <div ref={hostRef} className={className ?? 'h-full w-full overflow-hidden'} />;
}

/**
 * Global keymap hook. Subscribes a document-level keydown listener and
 * dispatches matching shortcuts to the registered handlers.
 *
 * Shortcut keys are strings like `mod+k`, `mod+shift+k`, `mod+1`, `esc`.
 * `mod` matches `metaKey` (Cmd) on macOS — AD is macOS only so we don't
 * also map `ctrlKey`.
 *
 * Default behavior when focus is inside an editable field (`<input>`,
 * `<textarea>`, contenteditable, monaco): single-key shortcuts (`esc`)
 * are still delivered, but `mod+1..9` and `mod+letter` shortcuts are
 * suppressed to avoid stealing typing intent. `mod+k` is *always*
 * delivered because the command palette is the primary action and
 * mirrors macOS Spotlight semantics.
 */

import { useEffect } from 'react';

export type Shortcut = string;
export type Handler = (e: KeyboardEvent) => void;
export type ShortcutMap = Record<Shortcut, Handler>;

function parseShortcut(s: Shortcut): { mod: boolean; shift: boolean; key: string } {
  const parts = s.toLowerCase().split('+').map((x) => x.trim());
  const mod = parts.includes('mod');
  const shift = parts.includes('shift');
  const key = parts[parts.length - 1] ?? '';
  return { mod, shift, key };
}

function matches(s: Shortcut, e: KeyboardEvent): boolean {
  const { mod, shift, key } = parseShortcut(s);
  if (mod !== e.metaKey) return false;
  if (shift !== e.shiftKey) return false;
  const ek = e.key.toLowerCase();
  if (key === 'enter') return ek === 'enter';
  if (key === 'esc') return ek === 'escape';
  return ek === key;
}

function isEditableTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  const tag = t.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
  if (t.isContentEditable) return true;
  // monaco renders into a contenteditable textarea; the check above catches it.
  return false;
}

// A few shortcuts should fire regardless of focus location.
const ALWAYS_ACTIVE = new Set(['mod+k', 'esc', 'mod+shift+k']);

export function useGlobalKeymap(map: ShortcutMap): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      const editable = isEditableTarget(e.target);
      for (const s of Object.keys(map)) {
        if (!matches(s, e)) continue;
        if (editable && !ALWAYS_ACTIVE.has(s.toLowerCase())) continue;
        e.preventDefault();
        map[s]?.(e);
        return;
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [map]);
}

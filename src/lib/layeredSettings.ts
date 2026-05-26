/**
 * Pure helpers for the layered settings editor.
 *
 * Author: taobaorun
 *
 * Lives outside the component file so the editor module exports only React
 * components — keeps Vite fast-refresh happy.
 */

export type LayerParse =
  | { ok: true; value: unknown | undefined }
  | { ok: false; message: string };

export function parseLayer(text: string): LayerParse {
  const trimmed = text.trim();
  if (trimmed === '') return { ok: true, value: undefined };
  try {
    const value = JSON.parse(trimmed);
    if (value !== null && typeof value !== 'object') {
      return { ok: false, message: 'must be a JSON object (or empty)' };
    }
    return { ok: true, value };
  } catch (e) {
    return { ok: false, message: e instanceof Error ? e.message : String(e) };
  }
}

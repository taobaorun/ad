import { useEffect, useMemo, useRef, useState } from 'react';
import Editor from '@monaco-editor/react';
import { useProfiles } from '@/store/profiles';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Label } from './ui/label';
import { Tabs, TabsList, TabsTrigger, TabsContent } from './ui/tabs';
import {
  ProfileFileSchema,
  type ProfileFile,
  type ProfileLayers,
  settingsFromLayers,
} from '@/lib/profileSchema';
import { useUiSettings } from '@/store/uiSettings';
import { Play, Save, Plus, X, Copy } from 'lucide-react';

type LayerName = 'shared' | 'local' | 'env';

/**
 * Three-tab layered profile editor (M3).
 *
 * - Shared tab: monaco JSON editor for `layers.shared` (writes to
 *   `<project>/.claude/settings.json` — committed to git, team-visible).
 * - Local tab: monaco JSON editor for `layers.local` (writes to
 *   `<project>/.claude/settings.local.json` — gitignored).
 * - Env tab: key-value table for `layers.env` (presented as `export`
 *   snippets — never written to a file).
 *
 * On save, the legacy `settings` field is kept in sync with `layers` via
 * `settingsFromLayers` so the v0.1 global-overwrite activation path
 * (deprecated, sunset in M5) keeps working.
 */
export function ProfileEditor() {
  const selectedId = useProfiles((s) => s.selectedId);
  const profile = useProfiles((s) => s.profiles.find((p) => p.id === selectedId)) ?? null;
  const save = useProfiles((s) => s.save);
  const activate = useProfiles((s) => s.activate);
  const select = useProfiles((s) => s.select);
  const showLegacy = useUiSettings((s) => s.showLegacyActivation);

  // Backend may omit empty layer fields via skip_serializing_if; fill defaults
  // through the zod schema so the editor always sees a fully-shaped profile.
  const normalize = (p: ProfileFile | null): ProfileFile | null =>
    p ? ProfileFileSchema.parse(p) : null;

  const [draft, setDraft] = useState<ProfileFile | null>(normalize(profile));
  const [activeLayer, setActiveLayer] = useState<LayerName>('local');

  // Per-layer monaco buffers. We mirror layers.<layer> as text so edits show
  // up immediately even before the JSON parses cleanly.
  const [sharedText, setSharedText] = useState<string>('');
  const [localText, setLocalText] = useState<string>('');

  const [saveError, setSaveError] = useState<string | null>(null);
  const [activateError, setActivateError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [envCopied, setEnvCopied] = useState(false);

  const lastLoadedIdRef = useRef<string | null>(profile?.id ?? null);

  useEffect(() => {
    // Selection changed — confirm if there are unsaved edits.
    if (lastLoadedIdRef.current !== (profile?.id ?? null) && dirty) {
      const ok = window.confirm('You have unsaved edits. Discard them and switch profiles?');
      if (!ok) {
        if (lastLoadedIdRef.current) select(lastLoadedIdRef.current);
        return;
      }
    }
    const normalized = normalize(profile);
    setDraft(normalized);
    setSharedText(layerText(normalized?.layers.shared));
    setLocalText(layerText(normalized?.layers.local));
    setSaveError(null);
    setActivateError(null);
    setDirty(false);
    lastLoadedIdRef.current = profile?.id ?? null;
    // We deliberately depend only on the profile id; deep equality on the
    // object would re-run after every save's `loadAll`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profile?.id]);

  // Validate the active text-based layer (shared/local). Env is structurally
  // valid by construction.
  const sharedValidation = useMemo(() => parseLayer(sharedText), [sharedText]);
  const localValidation = useMemo(() => parseLayer(localText), [localText]);
  const allValid = sharedValidation.ok && localValidation.ok;

  if (!draft) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        Select a profile from the sidebar.
      </div>
    );
  }

  function onEditMeta(next: Partial<ProfileFile>) {
    if (!draft) return;
    setDraft({ ...draft, ...next });
    setDirty(true);
  }

  function onEditEnv(next: Record<string, string>) {
    if (!draft) return;
    setDraft({ ...draft, layers: { ...draft.layers, env: next } });
    setDirty(true);
  }

  async function onSave(): Promise<boolean> {
    if (!draft || !allValid) return false;
    setBusy(true);
    setSaveError(null);
    try {
      const layers: ProfileLayers = {
        shared: sharedValidation.ok ? sharedValidation.value : undefined,
        local: localValidation.ok ? localValidation.value : undefined,
        env: draft.layers.env,
      };
      // Keep the v0.1 `settings` field synced for the legacy activation path.
      const settings = settingsFromLayers(layers);
      await save({ ...draft, layers, settings });
      setDirty(false);
      return true;
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function onActivate() {
    if (!draft || !allValid) return;
    const saved = await onSave();
    if (!saved) return;
    setBusy(true);
    setActivateError(null);
    try {
      await activate(draft.id);
    } catch (e) {
      setActivateError(
        `Saved, but activation failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    } finally {
      setBusy(false);
    }
  }

  async function onCopyExport() {
    if (!draft) return;
    const lines = Object.entries(draft.layers.env).map(
      ([k, v]) => `export ${k}=${shellQuote(v)}`,
    );
    if (lines.length === 0) return;
    await navigator.clipboard.writeText(lines.join('\n') + '\n');
    setEnvCopied(true);
    window.setTimeout(() => setEnvCopied(false), 1500);
  }

  return (
    <div className="flex h-full flex-col">
      {/* Header: color + name + save/activate */}
      <div className="flex items-center gap-2 border-b border-border p-3">
        <div className="flex flex-1 items-center gap-2">
          <input
            type="color"
            value={draft.color}
            onChange={(e) => onEditMeta({ color: e.target.value })}
            className="h-7 w-9 cursor-pointer rounded border border-input bg-transparent"
            aria-label="Profile color"
          />
          <Input
            value={draft.displayName}
            onChange={(e) => onEditMeta({ displayName: e.target.value })}
            className="max-w-md"
            aria-label="Display name"
          />
          {dirty && (
            <span className="text-xs text-muted-foreground" aria-live="polite">
              · unsaved
            </span>
          )}
        </div>
        <Button
          onClick={() => void onSave()}
          disabled={!allValid || busy}
          size="sm"
        >
          <Save className="h-4 w-4" />
          Save
        </Button>
        {showLegacy && (
          <Button
            variant="outline"
            onClick={() => void onActivate()}
            disabled={!allValid || busy}
            title="Legacy v0.1: overwrite ~/.claude/settings.json directly. Use the per-project Apply flow on the right instead."
            size="sm"
          >
            <Play className="h-4 w-4" />
            Activate (legacy)
          </Button>
        )}
      </div>

      {/* Sub-header: id + description */}
      <div className="flex items-center gap-2 px-3 py-2">
        <Label>id:</Label>
        <code className="rounded bg-muted px-1.5 py-0.5 text-xs">{draft.id}</code>
        <Label className="ml-3">description:</Label>
        <Input
          value={draft.description ?? ''}
          onChange={(e) => onEditMeta({ description: e.target.value })}
          className="flex-1"
          placeholder="Optional one-liner"
        />
      </div>

      {/* Errors */}
      {!allValid && (
        <div className="border-y border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
          {!sharedValidation.ok && <div>Shared: {sharedValidation.message}</div>}
          {!localValidation.ok && <div>Local: {localValidation.message}</div>}
        </div>
      )}
      {saveError && (
        <div className="border-y border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
          Save failed: {saveError}
        </div>
      )}
      {activateError && (
        <div className="border-y border-amber-500/40 bg-amber-500/10 px-3 py-1.5 text-xs text-amber-700 dark:text-amber-300">
          {activateError}
        </div>
      )}

      {/* Tabs */}
      <Tabs
        value={activeLayer}
        onValueChange={(v) => setActiveLayer(v as LayerName)}
        className="flex flex-1 flex-col overflow-hidden"
      >
        <div className="px-3 pt-2">
          <TabsList>
            <TabsTrigger value="shared">
              Shared {tabBadge(sharedValidation, draft.layers.shared)}
            </TabsTrigger>
            <TabsTrigger value="local">
              Local {tabBadge(localValidation, draft.layers.local)}
            </TabsTrigger>
            <TabsTrigger value="env">
              Env ({Object.keys(draft.layers.env).length})
            </TabsTrigger>
          </TabsList>
        </div>

        <TabsContent value="shared" className="flex flex-1 flex-col overflow-hidden">
          <div className="border-b border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
            ⚠ This layer writes to{' '}
            <code className="rounded bg-muted px-1 text-[11px]">
              &lt;project&gt;/.claude/settings.json
            </code>{' '}
            — committed to git, visible to your team. Don't put API keys here.
          </div>
          <div className="flex-1 overflow-hidden">
            <Editor
              height="100%"
              defaultLanguage="json"
              value={sharedText}
              onChange={(v) => {
                setSharedText(v ?? '');
                setDirty(true);
              }}
              options={editorOptions}
            />
          </div>
        </TabsContent>

        <TabsContent value="local" className="flex flex-1 flex-col overflow-hidden">
          <div className="border-b border-emerald-500/40 bg-emerald-500/10 px-3 py-1.5 text-xs text-muted-foreground">
            ✓ This layer writes to{' '}
            <code className="rounded bg-muted px-1 text-[11px]">
              &lt;project&gt;/.claude/settings.local.json
            </code>{' '}
            — gitignored, personal.
          </div>
          <div className="flex-1 overflow-hidden">
            <Editor
              height="100%"
              defaultLanguage="json"
              value={localText}
              onChange={(v) => {
                setLocalText(v ?? '');
                setDirty(true);
              }}
              options={editorOptions}
            />
          </div>
        </TabsContent>

        <TabsContent value="env" className="flex flex-1 flex-col overflow-auto">
          <EnvLayerEditor env={draft.layers.env} onChange={onEditEnv} />
          <div className="border-t border-border px-3 py-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void onCopyExport()}
              disabled={Object.keys(draft.layers.env).length === 0}
            >
              <Copy className="h-4 w-4" />
              {envCopied ? 'Copied' : 'Copy export commands'}
            </Button>
            <span className="ml-3 text-xs text-muted-foreground">
              env vars are never written to a file — paste these into your shell when needed.
            </span>
          </div>
        </TabsContent>
      </Tabs>
    </div>
  );
}

function EnvLayerEditor({
  env,
  onChange,
}: {
  env: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
}) {
  const entries = Object.entries(env);

  function setKey(oldKey: string, newKey: string) {
    if (oldKey === newKey) return;
    const next: Record<string, string> = {};
    for (const [k, v] of entries) {
      next[k === oldKey ? newKey : k] = v;
    }
    onChange(next);
  }

  function setValue(key: string, value: string) {
    onChange({ ...env, [key]: value });
  }

  function remove(key: string) {
    const next = { ...env };
    delete next[key];
    onChange(next);
  }

  function addRow() {
    const next = { ...env };
    let i = 1;
    while (`KEY_${i}` in next) i += 1;
    next[`KEY_${i}`] = '';
    onChange(next);
  }

  if (entries.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-sm text-muted-foreground">
        <div>No env vars yet.</div>
        <Button variant="outline" size="sm" onClick={addRow}>
          <Plus className="h-4 w-4" />
          Add row
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-1.5 p-3">
      <div className="grid grid-cols-[1fr_2fr_auto] items-center gap-2 text-xs uppercase tracking-wider text-muted-foreground">
        <div>Key</div>
        <div>Value</div>
        <div></div>
      </div>
      {entries.map(([k, v]) => (
        <div key={k} className="grid grid-cols-[1fr_2fr_auto] items-center gap-2">
          <Input
            value={k}
            onChange={(e) => setKey(k, e.target.value)}
            className="font-mono text-xs"
          />
          <Input
            value={v}
            onChange={(e) => setValue(k, e.target.value)}
            className="font-mono text-xs"
            placeholder="value"
          />
          <Button variant="ghost" size="sm" onClick={() => remove(k)} aria-label={`Remove ${k}`}>
            <X className="h-4 w-4" />
          </Button>
        </div>
      ))}
      <Button variant="outline" size="sm" onClick={addRow} className="mt-2 self-start">
        <Plus className="h-4 w-4" />
        Add row
      </Button>
    </div>
  );
}

const editorOptions = {
  fontSize: 13,
  minimap: { enabled: false },
  scrollBeyondLastLine: false,
  automaticLayout: true,
  tabSize: 2,
} as const;

function layerText(value: unknown): string {
  if (value === undefined || value === null) return '';
  return JSON.stringify(value, null, 2);
}

type LayerParse =
  | { ok: true; value: unknown | undefined }
  | { ok: false; message: string };

function parseLayer(text: string): LayerParse {
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

function tabBadge(parse: LayerParse, value: unknown): string {
  if (!parse.ok) return '⚠';
  if (value === undefined || value === null) return '(empty)';
  const keys = Object.keys(value as Record<string, unknown>).length;
  return `(${keys})`;
}

function shellQuote(s: string): string {
  // POSIX-safe single-quote escape: 'foo' → 'foo', "O'Brien" → 'O'\''Brien'.
  return `'${s.replace(/'/g, "'\\''")}'`;
}

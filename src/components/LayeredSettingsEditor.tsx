/**
 * Reusable layered settings editor — Monaco JSON tabs (shared / local) + KV env tab.
 *
 * Author: taobaorun
 *
 * Used by both ProfileEditor (template editing — typically shared + env) and
 * ProjectConfigEditor (per-project — shared + local + env). Tabs whose layer
 * prop is omitted are hidden.
 *
 * The component is purely controlled: callers own the underlying text / entry
 * state. JSON validity is computed internally and surfaced via
 * `onValidityChange(allValid)`; per-layer parse errors render inline above
 * each tab's editor.
 */

import { useEffect, useMemo, useState, type ReactNode } from 'react';
import Editor from '@monaco-editor/react';
import { Tabs, TabsList, TabsTrigger, TabsContent } from './ui/tabs';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Plus, X } from 'lucide-react';
import { parseLayer, type LayerParse } from '@/lib/layeredSettings';
import { useUiSettings } from '@/store/uiSettings';

export type LayerKey = 'shared' | 'local' | 'env';

export interface TextLayer {
  text: string;
  onChange: (next: string) => void;
  /** Optional caption rendered above the Monaco editor. */
  caption?: string;
  /** Optional tab label override. Defaults to a capitalized layer key. */
  label?: string;
}

export interface EnvLayer {
  entries: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
  caption?: string;
  label?: string;
  /** Optional footer below the KV table — used by ProfileEditor for "Copy export commands". */
  footer?: ReactNode;
}

export interface LayeredSettingsEditorProps {
  shared?: TextLayer;
  local?: TextLayer;
  env?: EnvLayer;
  /** Fires when overall validity (all visible JSON tabs parse) flips. */
  onValidityChange?: (allValid: boolean) => void;
  /** Initial tab. Falls back to the first visible layer. */
  defaultTab?: LayerKey;
}

export function LayeredSettingsEditor({
  shared,
  local,
  env,
  onValidityChange,
  defaultTab,
}: LayeredSettingsEditorProps) {
  const visibleLayers = useMemo<LayerKey[]>(() => {
    const out: LayerKey[] = [];
    if (shared) out.push('shared');
    if (local) out.push('local');
    if (env) out.push('env');
    return out;
  }, [shared, local, env]);

  const [activeTab, setActiveTab] = useState<LayerKey>(
    defaultTab && visibleLayers.includes(defaultTab) ? defaultTab : (visibleLayers[0] ?? 'shared'),
  );

  // If the active tab's layer disappears (e.g. host toggles a layer off),
  // fall back to the first remaining one.
  useEffect(() => {
    const first = visibleLayers[0];
    if (first && !visibleLayers.includes(activeTab)) {
      setActiveTab(first);
    }
  }, [visibleLayers, activeTab]);

  const sharedParse = useMemo<LayerParse>(
    () => (shared ? parseLayer(shared.text) : { ok: true, value: undefined }),
    [shared],
  );
  const localParse = useMemo<LayerParse>(
    () => (local ? parseLayer(local.text) : { ok: true, value: undefined }),
    [local],
  );

  const allValid = sharedParse.ok && localParse.ok;

  useEffect(() => {
    onValidityChange?.(allValid);
  }, [allValid, onValidityChange]);

  if (visibleLayers.length === 0) {
    return null;
  }

  return (
    <Tabs
      value={activeTab}
      onValueChange={(v) => setActiveTab(v as LayerKey)}
      className="flex flex-1 flex-col overflow-hidden"
    >
      <div className="px-3 pt-2">
        <TabsList>
          {shared && (
            <TabsTrigger value="shared">
              {shared.label ?? 'Shared'} {tabBadge(sharedParse)}
            </TabsTrigger>
          )}
          {local && (
            <TabsTrigger value="local">
              {local.label ?? 'Local'} {tabBadge(localParse)}
            </TabsTrigger>
          )}
          {env && (
            <TabsTrigger value="env">
              {env.label ?? 'Env'} ({Object.keys(env.entries).length})
            </TabsTrigger>
          )}
        </TabsList>
      </div>

      {shared && (
        <TabsContent value="shared" className="flex flex-1 flex-col overflow-hidden">
          <TextLayerPanel layer={shared} parse={sharedParse} />
        </TabsContent>
      )}

      {local && (
        <TabsContent value="local" className="flex flex-1 flex-col overflow-hidden">
          <TextLayerPanel layer={local} parse={localParse} />
        </TabsContent>
      )}

      {env && (
        <TabsContent value="env" className="flex flex-1 flex-col overflow-auto">
          {env.caption && (
            <div className="border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
              {env.caption}
            </div>
          )}
          <EnvLayerEditor entries={env.entries} onChange={env.onChange} />
          {env.footer}
        </TabsContent>
      )}
    </Tabs>
  );
}

function TextLayerPanel({ layer, parse }: { layer: TextLayer; parse: LayerParse }) {
  const darkMode = useUiSettings((s) => s.darkMode);
  return (
    <>
      {layer.caption && (
        <div className="border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
          {layer.caption}
        </div>
      )}
      {!parse.ok && (
        <div className="border-b border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
          {parse.message}
        </div>
      )}
      <div className="flex-1 overflow-hidden">
        <Editor
          height="100%"
          defaultLanguage="json"
          value={layer.text}
          onChange={(v) => layer.onChange(v ?? '')}
          theme={darkMode ? 'vs-dark' : 'light'}
          options={MONACO_OPTIONS}
        />
      </div>
    </>
  );
}

function EnvLayerEditor({
  entries: env,
  onChange,
}: {
  entries: Record<string, string>;
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

const MONACO_OPTIONS = {
  fontSize: 13,
  minimap: { enabled: false },
  scrollBeyondLastLine: false,
  automaticLayout: true,
  tabSize: 2,
} as const;

function tabBadge(parse: LayerParse): string {
  if (!parse.ok) return '⚠';
  if (parse.value === undefined || parse.value === null) return '(empty)';
  const keys = Object.keys(parse.value as Record<string, unknown>).length;
  return `(${keys})`;
}

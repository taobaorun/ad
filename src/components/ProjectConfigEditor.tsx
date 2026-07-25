/**
 * Per-project inline settings editor — reads / writes
 * `<project>/.claude/settings.json` (shared) and
 * `<project>/.claude/settings.local.json` (local + top-level env).
 *
 * Author: taobaorun
 *
 * Layout: three tabs via LayeredSettingsEditor.
 *   - Shared → settings.json text (committed to git, team-visible)
 *   - Local  → settings.local.json text minus the top-level `env` key
 *   - Env    → settings.local.json's top-level `env` field as KV entries
 *
 * Save persists every dirty layer atomically. settings.local.json is
 * recomposed as `{ ...localObj, env: envEntries }` on write — if the user
 * pasted an `env` key into the Local JSON tab, it loses to the Env tab.
 */

import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Save, CheckCircle2 } from 'lucide-react';
import { Button } from './ui/button';
import { LayeredSettingsEditor } from './LayeredSettingsEditor';
import { parseLayer } from '@/lib/layeredSettings';
import { tauri } from '@/lib/tauri';

export interface ProjectConfigEditorProps {
  projectPath: string;
}

export function ProjectConfigEditor({ projectPath }: ProjectConfigEditorProps) {
  const { t } = useTranslation();

  const [sharedText, setSharedText] = useState('{}');
  const [localText, setLocalText] = useState('{}');
  const [envEntries, setEnvEntries] = useState<Record<string, string>>({});

  const [sharedDirty, setSharedDirty] = useState(false);
  const [localDirty, setLocalDirty] = useState(false);
  const [envDirty, setEnvDirty] = useState(false);

  const [busy, setBusy] = useState(false);
  const [allValid, setAllValid] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const dirty = sharedDirty || localDirty || envDirty;

  const load = useCallback(async () => {
    setError(null);
    try {
      const [sharedRaw, localRaw] = await Promise.all([
        tauri.readProjectSettings(projectPath, 'shared'),
        tauri.readProjectSettings(projectPath, 'local'),
      ]);
      setSharedText(prettyOrRaw(sharedRaw));

      // Split local into env + rest. If env is missing or not an object,
      // treat as empty entries; remaining keys feed the Local JSON tab.
      const { rest, env } = splitLocal(localRaw);
      setLocalText(prettyOrRaw(JSON.stringify(rest)));
      setEnvEntries(env);

      setSharedDirty(false);
      setLocalDirty(false);
      setEnvDirty(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [projectPath]);

  useEffect(() => {
    void load();
  }, [load]);

  async function handleSave() {
    if (!allValid || !dirty) return;
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      if (sharedDirty) {
        await tauri.writeProjectSettings(projectPath, 'shared', normalizeJsonText(sharedText));
      }
      if (localDirty || envDirty) {
        const localParse = parseLayer(localText);
        const localObj = (
          localParse.ok && localParse.value && typeof localParse.value === 'object'
            ? (localParse.value as Record<string, unknown>)
            : {}
        ) as Record<string, unknown>;
        // Strip any inline `env` from localObj — Env tab is the source of truth.
        const { env: _drop, ...rest } = localObj;
        void _drop;
        const merged: Record<string, unknown> = { ...rest };
        if (Object.keys(envEntries).length > 0) {
          merged.env = envEntries;
        }
        await tauri.writeProjectSettings(projectPath, 'local', JSON.stringify(merged, null, 2));
      }
      setSharedDirty(false);
      setLocalDirty(false);
      setEnvDirty(false);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2500);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="flex h-full flex-col"
      style={{
        borderRadius: 10,
        border: '0.5px solid var(--ds-line)',
        background: 'var(--ds-bg-card)',
        overflow: 'hidden',
      }}
    >
      <div
        className="flex flex-shrink-0 items-center gap-3 border-b"
        style={{ borderColor: 'var(--ds-line)', padding: '10px 14px' }}
      >
        <span
          className="font-mono text-[11px] uppercase"
          style={{ letterSpacing: '0.10em', color: 'var(--ds-fg-4)' }}
        >
          {t('projectEditor.title')}
        </span>
        {dirty && (
          <span className="text-xs" style={{ color: 'var(--ds-fg-3)' }} aria-live="polite">
            · {t('projectEditor.unsaved')}
          </span>
        )}
        <div className="ml-auto flex items-center gap-2">
          {saved && (
            <span className="flex items-center gap-1 text-xs" style={{ color: 'var(--ds-ok)' }}>
              <CheckCircle2 className="h-3 w-3" />
              {t('projectEditor.saved')}
            </span>
          )}
          <Button
            onClick={() => void handleSave()}
            disabled={!dirty || !allValid || busy}
            size="sm"
          >
            <Save className="h-4 w-4" />
            {busy ? t('projectEditor.saving') : t('projectEditor.save')}
          </Button>
        </div>
      </div>
      {error && (
        <div className="border-b border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
          {error}
        </div>
      )}
      <div className="flex min-h-0 flex-1 flex-col" style={{ minHeight: 280 }}>
        <LayeredSettingsEditor
          shared={{
            text: sharedText,
            label: t('projectEditor.tabs.shared'),
            caption: t('projectEditor.captions.shared'),
            onChange: (next) => {
              setSharedText(next);
              setSharedDirty(true);
            },
          }}
          local={{
            text: localText,
            label: t('projectEditor.tabs.local'),
            caption: t('projectEditor.captions.local'),
            onChange: (next) => {
              setLocalText(next);
              setLocalDirty(true);
            },
          }}
          env={{
            entries: envEntries,
            label: t('projectEditor.tabs.env'),
            caption: t('projectEditor.captions.env'),
            onChange: (next) => {
              setEnvEntries(next);
              setEnvDirty(true);
            },
          }}
          onValidityChange={setAllValid}
          defaultTab="shared"
        />
      </div>
    </div>
  );
}

function prettyOrRaw(raw: string): string {
  try {
    const parsed = JSON.parse(raw);
    return JSON.stringify(parsed, null, 2);
  } catch {
    // Surface raw content even when it doesn't parse, so the user can fix it.
    return raw;
  }
}

function normalizeJsonText(text: string): string {
  // Re-pretty-print so saved files have stable formatting; if user typed
  // invalid JSON the Save button is disabled, so we shouldn't reach here.
  const trimmed = text.trim();
  if (trimmed === '') return '{}';
  return JSON.stringify(JSON.parse(trimmed), null, 2);
}

function splitLocal(raw: string): { rest: Record<string, unknown>; env: Record<string, string> } {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { rest: {}, env: {} };
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return { rest: {}, env: {} };
  }
  const obj = parsed as Record<string, unknown>;
  const env: Record<string, string> = {};
  if (obj.env && typeof obj.env === 'object' && !Array.isArray(obj.env)) {
    for (const [k, v] of Object.entries(obj.env as Record<string, unknown>)) {
      if (typeof v === 'string') env[k] = v;
    }
  }
  const { env: _e, ...rest } = obj;
  void _e;
  return { rest, env };
}

import { useEffect, useMemo, useRef, useState } from 'react';
import Editor from '@monaco-editor/react';
import { useProfiles } from '@/store/profiles';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Label } from './ui/label';
import { ClaudeSettingsSchema, type ClaudeSettings, type ProfileFile } from '@/lib/profileSchema';
import { DiffView } from './DiffView';
import { Play, Save, GitCompare } from 'lucide-react';

export function ProfileEditor() {
  const selectedId = useProfiles((s) => s.selectedId);
  const profile = useProfiles((s) => s.profiles.find((p) => p.id === selectedId)) ?? null;
  const save = useProfiles((s) => s.save);
  const activate = useProfiles((s) => s.activate);
  const select = useProfiles((s) => s.select);

  const [draft, setDraft] = useState<ProfileFile | null>(profile);
  const [bodyText, setBodyText] = useState<string>(
    profile ? JSON.stringify(profile.settings, null, 2) : '{}',
  );
  const [showDiff, setShowDiff] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [activateError, setActivateError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Track whether the current selection has unsaved edits.
  const [dirty, setDirty] = useState(false);
  const lastLoadedIdRef = useRef<string | null>(profile?.id ?? null);

  useEffect(() => {
    // Selection changed — confirm if there are unsaved edits.
    if (lastLoadedIdRef.current !== (profile?.id ?? null) && dirty) {
      const ok = window.confirm('You have unsaved edits. Discard them and switch profiles?');
      if (!ok) {
        // Re-select the previous id; this Effect will fire again with the
        // unchanged ref so we won't loop.
        if (lastLoadedIdRef.current) select(lastLoadedIdRef.current);
        return;
      }
    }
    setDraft(profile);
    setBodyText(profile ? JSON.stringify(profile.settings, null, 2) : '{}');
    setSaveError(null);
    setActivateError(null);
    setDirty(false);
    lastLoadedIdRef.current = profile?.id ?? null;
    // We deliberately depend only on the profile id; deep-equality on the
    // object would re-run after every save's `loadAll`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profile?.id]);

  const validation = useMemo(() => {
    try {
      const parsed = JSON.parse(bodyText);
      const result = ClaudeSettingsSchema.safeParse(parsed);
      if (result.success) return { ok: true as const, value: result.data };
      return {
        ok: false as const,
        message: result.error.issues
          .map((i) => `${i.path.join('.') || '<root>'}: ${i.message}`)
          .join('; '),
      };
    } catch (e) {
      return { ok: false as const, message: e instanceof Error ? e.message : String(e) };
    }
  }, [bodyText]);

  if (!draft) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        Select a profile from the sidebar.
      </div>
    );
  }

  function onEdit(next: ProfileFile) {
    setDraft(next);
    setDirty(true);
  }

  function onBodyChange(v: string) {
    setBodyText(v);
    setDirty(true);
  }

  async function onSave(): Promise<boolean> {
    if (!draft || !validation.ok) return false;
    setBusy(true);
    setSaveError(null);
    try {
      await save({ ...draft, settings: validation.value as ClaudeSettings });
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
    if (!draft || !validation.ok) return;
    const saved = await onSave();
    if (!saved) return; // save error is already surfaced; do not proceed
    setBusy(true);
    setActivateError(null);
    try {
      await activate(draft.id);
    } catch (e) {
      // Save succeeded; activate failed. Tell the user explicitly.
      setActivateError(
        `Saved, but activation failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border p-3">
        <div className="flex flex-1 items-center gap-2">
          <input
            type="color"
            value={draft.color}
            onChange={(e) => onEdit({ ...draft, color: e.target.value })}
            className="h-7 w-9 cursor-pointer rounded border border-input bg-transparent"
            aria-label="Profile color"
          />
          <Input
            value={draft.displayName}
            onChange={(e) => onEdit({ ...draft, displayName: e.target.value })}
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
          variant="outline"
          size="sm"
          onClick={() => setShowDiff((v) => !v)}
          aria-pressed={showDiff}
        >
          <GitCompare className="h-4 w-4" />
          Diff
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void onSave()}
          disabled={!validation.ok || busy}
        >
          <Save className="h-4 w-4" />
          Save
        </Button>
        <Button onClick={() => void onActivate()} disabled={!validation.ok || busy}>
          <Play className="h-4 w-4" />
          Activate
        </Button>
      </div>

      <div className="flex items-center gap-2 px-3 py-2">
        <Label>id:</Label>
        <code className="rounded bg-muted px-1.5 py-0.5 text-xs">{draft.id}</code>
        <Label className="ml-3">description:</Label>
        <Input
          value={draft.description ?? ''}
          onChange={(e) => onEdit({ ...draft, description: e.target.value })}
          className="flex-1"
          placeholder="Optional one-liner"
        />
      </div>

      {!validation.ok && (
        <div className="border-y border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
          {validation.message}
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

      <div className="flex-1 overflow-hidden">
        {showDiff ? (
          <DiffView candidateText={bodyText} />
        ) : (
          <Editor
            height="100%"
            defaultLanguage="json"
            value={bodyText}
            onChange={(v) => onBodyChange(v ?? '')}
            options={{
              fontSize: 13,
              minimap: { enabled: false },
              scrollBeyondLastLine: false,
              automaticLayout: true,
              tabSize: 2,
            }}
          />
        )}
      </div>
    </div>
  );
}

import { useEffect, useState } from 'react';
import { tauri } from '@/lib/tauri';
import { useProfiles } from '@/store/profiles';
import { useProjects } from '@/store/projects';
import { Button } from './ui/button';
import { Dialog } from './ui/dialog';
import type { Project, ProjectStatus, ApplyOptions, ApplyOutcome } from '@/lib/projectTypes';
import { AlertTriangle, CheckCircle2, Copy } from 'lucide-react';

/**
 * Apply dialog (M4.4).
 *
 * Three steps stitched together inline:
 * 1. Pick layers + git-dirty acknowledgement (if shared layer requested)
 * 2. Show diff/preview (skipped for now — surfaced as a dry-run later)
 * 3. Resolve any conflicts the backend reports
 *
 * For M4.4 first cut we collapse 1 & 2: pick layers + status banner, then
 * apply, surfacing conflicts as step 3 if the backend pushes back.
 */
export function ApplyDialog({ project, onClose }: { project: Project; onClose: () => void }) {
  const profiles = useProfiles((s) => s.profiles);
  const reload = useProjects((s) => s.loadAll);

  const [profileId, setProfileId] = useState<string>(
    project.currentProfileId ?? project.lastApplied?.profileId ?? profiles[0]?.id ?? '',
  );
  const [layers, setLayers] = useState<{ shared: boolean; local: boolean; env: boolean }>({
    shared: false,
    local: true,
    env: false,
  });
  const [status, setStatus] = useState<ProjectStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [dirtyAcked, setDirtyAcked] = useState(false);
  const [outcome, setOutcome] = useState<ApplyOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        setStatus(await tauri.getProjectStatus(project.path));
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    })();
  }, [project.path]);

  const selectedLayers: ('shared' | 'local' | 'env')[] = [
    layers.shared && 'shared',
    layers.local && 'local',
    layers.env && 'env',
  ].filter((x): x is 'shared' | 'local' | 'env' => Boolean(x));

  const sharedAndDirty = layers.shared && status?.gitDirty === true;
  const blocked = sharedAndDirty && !dirtyAcked;

  async function onApply() {
    if (!profileId || selectedLayers.length === 0) return;
    setBusy(true);
    setError(null);
    setOutcome(null);
    try {
      const opts: ApplyOptions = {
        layers: selectedLayers,
        resolutions: {},
        overwriteDirtyWarningAcked: dirtyAcked,
      };
      const result = await tauri.applyProfileToProject(profileId, project.path, opts);
      setOutcome(result);
      if (result.kind === 'applied') {
        await reload();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={`Apply to ${project.displayName}`}
      size="lg"
    >
      <div className="flex flex-col gap-4 text-sm">
          <div className="flex items-center gap-2">
            <label className="w-20 text-muted-foreground">Profile</label>
            <select
              value={profileId}
              onChange={(e) => setProfileId(e.target.value)}
              className="flex-1 rounded border border-input bg-background px-2 py-1"
            >
              {profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.displayName}
                </option>
              ))}
            </select>
          </div>

          <div>
            <div className="mb-1 text-muted-foreground">Layers</div>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={layers.shared}
                onChange={(e) => setLayers({ ...layers, shared: e.target.checked })}
              />
              <span>Shared</span>
              <span className="text-xs text-destructive">⚠ writes to .claude/settings.json (git)</span>
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={layers.local}
                onChange={(e) => setLayers({ ...layers, local: e.target.checked })}
              />
              <span>Local</span>
              <span className="text-xs text-muted-foreground">.claude/settings.local.json (gitignored)</span>
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={layers.env}
                onChange={(e) => setLayers({ ...layers, env: e.target.checked })}
              />
              <span>Env</span>
              <span className="text-xs text-muted-foreground">copy export snippet (no file write)</span>
            </label>
          </div>

          {sharedAndDirty && (
            <div className="rounded border border-rust/40 bg-rust/10 p-3 text-xs text-rust">
              <div className="mb-2 flex items-center gap-2 font-semibold">
                <AlertTriangle className="h-4 w-4" />
                Working tree is dirty
              </div>
              <p className="mb-2">
                The shared layer writes to <code>.claude/settings.json</code> (git-tracked). Your project
                has uncommitted changes — applying may collide with them.
              </p>
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={dirtyAcked}
                  onChange={(e) => setDirtyAcked(e.target.checked)}
                />
                <span>Apply anyway, I'll review the diff in git</span>
              </label>
            </div>
          )}

          {status && (
            <div className="rounded border border-border bg-muted/30 p-2 text-xs text-muted-foreground">
              <div>git: {status.isGitRepo ? (status.gitDirty ? 'dirty' : 'clean') : 'not a repo'}</div>
              <div>
                .claude/: {status.claudeDirExists ? 'present' : 'absent'} ·{' '}
                settings.json: {status.hasSettingsJson ? 'present' : 'absent'} ·{' '}
                settings.local.json: {status.hasSettingsLocalJson ? 'present' : 'absent'}
              </div>
              {status.isGitRepo && status.gitignoreExcludesSettingsLocal === false && (
                <div className="mt-1 text-rust">
                  ⚠ settings.local.json is not in .gitignore — consider adding it.
                </div>
              )}
            </div>
          )}

          {error && (
            <div className="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {error}
            </div>
          )}

          {outcome && <OutcomeView outcome={outcome} onRetry={() => setOutcome(null)} />}
      </div>

      <div className="mt-4 flex justify-end gap-2">
        <Button variant="outline" onClick={onClose} disabled={busy}>
          {outcome?.kind === 'applied' ? 'Close' : 'Cancel'}
        </Button>
        {outcome?.kind !== 'applied' && (
          <Button
            onClick={() => void onApply()}
            disabled={busy || blocked || !profileId || selectedLayers.length === 0}
          >
            {busy ? 'Applying…' : 'Apply'}
          </Button>
        )}
      </div>
    </Dialog>
  );
}

function OutcomeView({ outcome, onRetry: _onRetry }: { outcome: ApplyOutcome; onRetry: () => void }) {
  if (outcome.kind === 'applied') {
    return (
      <div className="rounded border border-emerald-500/40 bg-emerald-500/10 p-3 text-xs">
        <div className="mb-2 flex items-center gap-2 font-semibold text-emerald-700 dark:text-emerald-300">
          <CheckCircle2 className="h-4 w-4" />
          Applied
        </div>
        {outcome.writtenFiles.length > 0 && (
          <div className="mb-2">
            <div className="text-muted-foreground">Wrote:</div>
            <ul className="ml-4 list-disc">
              {outcome.writtenFiles.map((f) => (
                <li key={f} className="font-mono">{f}</li>
              ))}
            </ul>
          </div>
        )}
        {outcome.backupPaths.length > 0 && (
          <div className="mb-2 text-muted-foreground">
            Backed up {outcome.backupPaths.length} file(s) under ~/.ad/backups/
          </div>
        )}
        {outcome.envExportSnippet && (
          <div className="mt-2">
            <div className="mb-1 flex items-center gap-2">
              <span className="text-muted-foreground">env exports:</span>
              <Button
                size="sm"
                variant="outline"
                onClick={() => navigator.clipboard.writeText(outcome.envExportSnippet ?? '')}
              >
                <Copy className="h-3 w-3" />
                Copy
              </Button>
            </div>
            <pre className="overflow-auto rounded bg-muted p-2 font-mono text-[11px]">
              {outcome.envExportSnippet}
            </pre>
          </div>
        )}
        {outcome.warnings.length > 0 && (
          <ul className="mt-2 ml-4 list-disc text-amber-700 dark:text-amber-300">
            {outcome.warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        )}
      </div>
    );
  }
  if (outcome.kind === 'gitDirtyBlocked') {
    return (
      <div className="rounded border border-rust/40 bg-rust/10 p-3 text-xs text-rust">
        Git dirty — {outcome.message}
      </div>
    );
  }
  if (outcome.kind === 'needsResolution') {
    return (
      <div className="rounded border border-clay/40 bg-clay/10 p-3 text-xs">
        <div className="mb-2 font-semibold text-clay">
          {outcome.conflicts.length} conflict(s) in {outcome.layer} layer
        </div>
        <p className="mb-2 text-muted-foreground">
          The conflict-resolution UI is M4.4 work-in-progress. For now, edit the target file
          manually or pick a different layer mix.
        </p>
        <ul className="ml-4 list-disc font-mono">
          {outcome.conflicts.map((c) => (
            <li key={c.keyPath}>
              <code>{c.keyPath}</code>: existing={JSON.stringify(c.existing)}, incoming=
              {JSON.stringify(c.incoming)}
            </li>
          ))}
        </ul>
      </div>
    );
  }
  return null;
}

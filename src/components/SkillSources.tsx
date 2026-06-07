import { useEffect, useMemo, useState } from 'react';
import { useSkills } from '@/store/skills';
import type { SkillEntry } from '@/lib/skillTypes';
import { Plus, RefreshCw, Trash2, ChevronDown, ChevronRight } from 'lucide-react';
import { Toggle } from './SkillToggle';

export function SkillSourcesSection() {
  const sources = useSkills((s) => s.sources);
  const entries = useSkills((s) => s.entries);
  const loadSources = useSkills((s) => s.loadSources);
  const scanLibrary = useSkills((s) => s.scanLibrary);
  const removeSource = useSkills((s) => s.removeSource);
  const updateSource = useSkills((s) => s.updateSource);
  const setSkillScope = useSkills((s) => s.setSkillScope);

  const [addOpen, setAddOpen] = useState(false);
  const [updating, setUpdating] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  useEffect(() => {
    void loadSources();
    void scanLibrary();
  }, [loadSources, scanLibrary]);

  const managedBySource = useMemo(() => {
    const managed = entries.filter((e) => e.source === 'managed');
    const groups: Record<string, SkillEntry[]> = {};
    for (const e of managed) {
      const key = e.sourceId ?? 'unknown';
      (groups[key] ??= []).push(e);
    }
    return Object.entries(groups).sort(([a], [b]) => a.localeCompare(b));
  }, [entries]);

  async function handleUpdate(id: string) {
    setUpdating(id);
    try {
      await updateSource(id);
    } finally {
      setUpdating(null);
    }
  }

  async function handleRemove(id: string) {
    if (!confirm(`Remove source "${id}" and all its installed skills?`)) return;
    await removeSource(id);
  }

  return (
    <>
      <header className="mb-5">
        <h1 className="text-xl font-semibold leading-tight">Skill Sources</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Manage skill repositories and control which skills are globally available.
        </p>
      </header>

      <div className="mb-4 flex items-center justify-between">
        <h2 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Sources
        </h2>
        <button
          type="button"
          onClick={() => setAddOpen(true)}
          className="inline-flex items-center gap-1.5 rounded-md border border-clay bg-clay/10 px-2.5 py-1 text-xs text-clay hover:bg-clay/20"
        >
          <Plus className="h-3 w-3" /> Add Source
        </button>
      </div>

      {sources.length === 0 && (
        <div className="rounded-lg border border-dashed border-border p-6 text-center text-xs text-muted-foreground">
          No skill sources configured. Add a git repository or local directory.
        </div>
      )}

      {sources.map((s) => (
        <div key={s.id} className="mb-2 rounded-lg border border-border p-3">
          <div className="flex items-start justify-between">
            <div>
              <div className="text-sm font-medium">{s.id}</div>
              <div className="mt-0.5 font-mono text-[11px] text-muted-foreground">{s.url}</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">
                {s.sourceType === 'git' ? `branch: ${s.branch ?? 'default'}` : 'Local directory'}
                {s.subdirectory ? ` · subdir: ${s.subdirectory}` : ''}
                {s.autoUpdate ? ' · Auto-update: on' : ''}
              </div>
            </div>
            <div className="flex gap-1.5">
              {s.sourceType === 'git' && (
                <SmallBtn
                  onClick={() => void handleUpdate(s.id)}
                  disabled={updating === s.id}
                >
                  <RefreshCw className={`h-3 w-3 ${updating === s.id ? 'animate-spin' : ''}`} />
                  {updating === s.id ? 'Updating...' : 'Update'}
                </SmallBtn>
              )}
              <SmallBtn onClick={() => void handleRemove(s.id)} danger>
                <Trash2 className="h-3 w-3" />
              </SmallBtn>
            </div>
          </div>
        </div>
      ))}

      {managedBySource.length > 0 && (
        <GlobalSkillsPanel
          managedBySource={managedBySource}
          expanded={expanded}
          setExpanded={setExpanded}
          setSkillScope={setSkillScope}
          scanLibrary={scanLibrary}
        />
      )}

      {addOpen && (
        <AddSourceDialog
          onClose={() => setAddOpen(false)}
          onAdded={() => { setAddOpen(false); void loadSources(); void scanLibrary(); }}
        />
      )}
    </>
  );
}

function AddSourceDialog({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  onAdded: () => void;
}) {
  const addSource = useSkills((s) => s.addSource);
  const [id, setId] = useState('');
  const [sourceType, setSourceType] = useState<'git' | 'local'>('git');
  const [url, setUrl] = useState('');
  const [branch, setBranch] = useState('');
  const [subdirectory, setSubdirectory] = useState('');
  const [autoUpdate, setAutoUpdate] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  async function handleSubmit() {
    setError(null);
    setSaving(true);
    try {
      await addSource({
        id,
        sourceType,
        url,
        branch: branch || undefined,
        subdirectory: subdirectory || undefined,
        autoUpdate,
        addedAt: new Date().toISOString(),
      });
      onAdded();
    } catch (e: any) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-[440px] rounded-xl border border-border bg-background p-5 shadow-xl">
        <h3 className="mb-4 text-base font-semibold">Add Skill Source</h3>

        <FieldLabel label="Name">
          <input
            type="text"
            value={id}
            onChange={(e) => setId(e.target.value)}
            placeholder="e.g. my-team-skills"
            className="w-full rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs outline-none focus:border-clay"
          />
        </FieldLabel>

        <FieldLabel label="Type">
          <div className="flex gap-2">
            <TypeBtn on={sourceType === 'git'} onClick={() => setSourceType('git')}>
              Git Repository
            </TypeBtn>
            <TypeBtn on={sourceType === 'local'} onClick={() => setSourceType('local')}>
              Local Directory
            </TypeBtn>
          </div>
        </FieldLabel>

        <FieldLabel label={sourceType === 'git' ? 'Git URL' : 'Path'}>
          <input
            type="text"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder={
              sourceType === 'git'
                ? 'git@github.com:org/skills.git'
                : '/Users/me/my-skills'
            }
            className="w-full rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs outline-none focus:border-clay"
          />
        </FieldLabel>

        {sourceType === 'git' && (
          <FieldLabel label="Branch (optional)">
            <input
              type="text"
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
              placeholder="main"
              className="w-full rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs outline-none focus:border-clay"
            />
          </FieldLabel>
        )}

        <FieldLabel label="Subdirectory (optional)">
          <input
            type="text"
            value={subdirectory}
            onChange={(e) => setSubdirectory(e.target.value)}
            placeholder="e.g. skills/"
            className="w-full rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs outline-none focus:border-clay"
          />
        </FieldLabel>

        {sourceType === 'git' && (
          <label className="mt-2 flex cursor-pointer items-center gap-2 text-xs text-muted-foreground">
            <input
              type="checkbox"
              checked={autoUpdate}
              onChange={(e) => setAutoUpdate(e.target.checked)}
            />
            Auto-update on AD launch
          </label>
        )}

        {error && (
          <div className="mt-3 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            {error}
          </div>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md border border-border px-3 py-1.5 text-xs hover:bg-foreground/5"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => void handleSubmit()}
            disabled={!id || !url || saving}
            className="rounded-md bg-clay px-3 py-1.5 text-xs text-white hover:opacity-90 disabled:opacity-50"
          >
            {saving ? 'Adding...' : 'Add & Scan'}
          </button>
        </div>
      </div>
    </div>
  );
}

function GlobalSkillsPanel({
  managedBySource,
  expanded,
  setExpanded,
  setSkillScope,
  scanLibrary,
}: {
  managedBySource: [string, SkillEntry[]][];
  expanded: Record<string, boolean>;
  setExpanded: React.Dispatch<React.SetStateAction<Record<string, boolean>>>;
  setSkillScope: (skillId: string, scope: string) => Promise<void>;
  scanLibrary: () => Promise<void>;
}) {
  const [optimistic, setOptimistic] = useState<Record<string, boolean>>({});

  function isGlobal(entry: SkillEntry): boolean {
    const id = `${entry.sourceId}/${entry.name}`;
    if (id in optimistic) return optimistic[id] ?? false;
    return entry.scope === 'global';
  }

  function handleToggle(entry: SkillEntry) {
    const id = `${entry.sourceId}/${entry.name}`;
    const next = !isGlobal(entry);
    setOptimistic((prev) => ({ ...prev, [id]: next }));
    setSkillScope(id, next ? 'global' : 'none')
      .then(() => scanLibrary())
      .then(() => setOptimistic((prev) => { const c = { ...prev }; delete c[id]; return c; }))
      .catch(() => setOptimistic((prev) => { const c = { ...prev }; delete c[id]; return c; }));
  }

  function handleToggleAll(skills: SkillEntry[], enable: boolean) {
    const batch: Record<string, boolean> = {};
    for (const entry of skills) {
      if (isGlobal(entry) === enable) continue;
      batch[`${entry.sourceId}/${entry.name}`] = enable;
    }
    if (Object.keys(batch).length === 0) return;
    setOptimistic((prev) => ({ ...prev, ...batch }));
    const cleanup = () => setOptimistic((prev) => {
      const c = { ...prev };
      for (const id of Object.keys(batch)) delete c[id];
      return c;
    });
    (async () => {
      for (const [id, val] of Object.entries(batch)) {
        await setSkillScope(id, val ? 'global' : 'none');
      }
    })().then(() => scanLibrary()).then(cleanup).catch(cleanup);
  }

  return (
    <>
      <h2 className="mb-2 mt-6 text-xs font-medium uppercase tracking-wider text-muted-foreground">
        Global Skills
      </h2>
      <p className="mb-3 text-[11px] text-muted-foreground">
        Enabled skills are symlinked to ~/.claude/skills/ and visible to all projects.
      </p>

      {managedBySource.map(([sourceId, skills]) => {
        const globalCount = skills.filter(isGlobal).length;
        const allOn = globalCount === skills.length;
        const isExpanded = expanded[sourceId] ?? false;
        return (
          <div
            key={sourceId}
            className="mb-2 overflow-hidden rounded-lg border border-border"
          >
            <button
              type="button"
              onClick={() => setExpanded((p) => ({ ...p, [sourceId]: !p[sourceId] }))}
              className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-foreground/5"
              style={{ background: 'var(--muted)' }}
            >
              {isExpanded
                ? <ChevronDown className="h-3 w-3 text-muted-foreground" />
                : <ChevronRight className="h-3 w-3 text-muted-foreground" />
              }
              <span className="text-[12px] font-semibold">{sourceId}</span>
              <span className="text-[11px] text-muted-foreground">
                {skills.length} skills
              </span>
              <span className="ml-auto text-[11px] text-muted-foreground">
                {globalCount}/{skills.length}
              </span>
              <div onClick={(e) => e.stopPropagation()}>
                <Toggle on={allOn} onChange={() => void handleToggleAll(skills, !allOn)} />
              </div>
            </button>
            {isExpanded && (
              <div>
                {skills.map((entry) => {
                  const id = `${entry.sourceId}/${entry.name}`;
                  return (
                    <div
                      key={id}
                      className="flex items-center gap-3 py-[7px] text-[13px] transition-colors hover:bg-foreground/5"
                      style={{ paddingLeft: 28, paddingRight: 12, borderTop: '0.5px solid color-mix(in srgb, var(--border) 50%, transparent)' }}
                    >
                      <div className="min-w-[120px] font-medium">{entry.name}</div>
                      <div className="flex-1 truncate text-[12px] text-muted-foreground">
                        {entry.description ?? ''}
                      </div>
                      <div className="w-9">
                        <Toggle on={isGlobal(entry)} onChange={() => void handleToggle(entry)} />
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}
    </>
  );
}

function FieldLabel({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="mb-3">
      <div className="mb-1 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </div>
      {children}
    </div>
  );
}

function SmallBtn({
  children,
  onClick,
  disabled,
  danger,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={
        'inline-flex items-center gap-1 rounded-md border px-2 py-1 text-[11px] transition-colors disabled:opacity-50 ' +
        (danger
          ? 'border-destructive/30 text-destructive hover:bg-destructive/10'
          : 'border-border hover:bg-foreground/5')
      }
    >
      {children}
    </button>
  );
}

function TypeBtn({
  on,
  onClick,
  children,
}: {
  on: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        'rounded-md border px-3 py-1.5 text-xs ' +
        (on
          ? 'border-foreground bg-foreground text-background'
          : 'border-border hover:border-clay')
      }
    >
      {children}
    </button>
  );
}

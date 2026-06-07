import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSkills } from '@/store/skills';
import type { SkillEntry } from '@/lib/skillTypes';
import { Plus, RefreshCw, Trash2, ChevronDown, ChevronRight } from 'lucide-react';
import { Toggle } from './SkillToggle';

export function SkillSourcesSection() {
  const { t } = useTranslation();
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
    if (!confirm(t('settings.skills.removeConfirm', { id }))) return;
    await removeSource(id);
  }

  return (
    <>
      <header className="mb-5">
        <h1 className="text-xl font-semibold leading-tight">{t('settings.skills.title')}</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          {t('settings.skills.desc')}
        </p>
      </header>

      <div className="mb-4 flex items-center justify-between">
        <h2 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          {t('settings.skills.sourcesHeading')}
        </h2>
        <button
          type="button"
          onClick={() => setAddOpen(true)}
          className="inline-flex items-center gap-1.5 rounded-md border border-clay bg-clay/10 px-2.5 py-1 text-xs text-clay hover:bg-clay/20"
        >
          <Plus className="h-3 w-3" /> {t('settings.skills.addSource')}
        </button>
      </div>

      {sources.length === 0 && (
        <div className="rounded-lg border border-dashed border-border p-6 text-center text-xs text-muted-foreground">
          {t('settings.skills.noSources')}
        </div>
      )}

      {sources.map((s) => (
        <div key={s.id} className="mb-2 rounded-lg border border-border p-3">
          <div className="flex items-start justify-between">
            <div>
              <div className="text-sm font-medium">{s.id}</div>
              <div className="mt-0.5 font-mono text-[11px] text-muted-foreground">{s.url}</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">
                {s.sourceType === 'git'
                  ? (s.branch ? t('settings.skills.branch', { branch: s.branch }) : t('settings.skills.branchDefault'))
                  : t('settings.skills.localDir')}
                {s.subdirectory ? ` · ${t('settings.skills.subdir', { dir: s.subdirectory })}` : ''}
                {s.autoUpdate ? ` · ${t('settings.skills.autoUpdateOn')}` : ''}
              </div>
            </div>
            <div className="flex gap-1.5">
              {s.sourceType === 'git' && (
                <SmallBtn
                  onClick={() => void handleUpdate(s.id)}
                  disabled={updating === s.id}
                >
                  <RefreshCw className={`h-3 w-3 ${updating === s.id ? 'animate-spin' : ''}`} />
                  {updating === s.id ? t('settings.skills.updating') : t('settings.skills.update')}
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
  const { t } = useTranslation();
  const addSource = useSkills((s) => s.addSource);
  const [id, setId] = useState('');
  const [idManual, setIdManual] = useState(false);
  const [sourceType, setSourceType] = useState<'git' | 'local'>('git');
  const [url, setUrl] = useState('');
  const [branch, setBranch] = useState('');
  const [subdirectory, setSubdirectory] = useState('');
  const [autoUpdate, setAutoUpdate] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  function extractRepoName(gitUrl: string): string {
    const cleaned = gitUrl.replace(/\.git\s*$/, '').replace(/\/+$/, '');
    const last = cleaned.split(/[/:]+/).pop() ?? '';
    return last;
  }

  function handleUrlChange(val: string) {
    setUrl(val);
    if (!idManual) {
      const name = sourceType === 'git' ? extractRepoName(val) : val.split('/').filter(Boolean).pop() ?? '';
      setId(name);
    }
  }

  function handleIdChange(val: string) {
    setId(val);
    setIdManual(true);
  }

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
        <h3 className="mb-4 text-base font-semibold">{t('settings.skills.addDialog.title')}</h3>

        <FieldLabel label={t('settings.skills.addDialog.name')}>
          <input
            type="text"
            value={id}
            onChange={(e) => handleIdChange(e.target.value)}
            placeholder={t('settings.skills.addDialog.namePlaceholder')}
            className="w-full rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs outline-none focus:border-clay"
          />
        </FieldLabel>

        <FieldLabel label={t('settings.skills.addDialog.type')}>
          <div className="flex gap-2">
            <TypeBtn on={sourceType === 'git'} onClick={() => setSourceType('git')}>
              {t('settings.skills.addDialog.typeGit')}
            </TypeBtn>
            <TypeBtn on={sourceType === 'local'} onClick={() => setSourceType('local')}>
              {t('settings.skills.addDialog.typeLocal')}
            </TypeBtn>
          </div>
        </FieldLabel>

        <FieldLabel label={sourceType === 'git' ? t('settings.skills.addDialog.gitUrl') : t('settings.skills.addDialog.path')}>
          <input
            type="text"
            value={url}
            onChange={(e) => handleUrlChange(e.target.value)}
            placeholder={
              sourceType === 'git'
                ? t('settings.skills.addDialog.gitUrlPlaceholder')
                : t('settings.skills.addDialog.pathPlaceholder')
            }
            className="w-full rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs outline-none focus:border-clay"
          />
        </FieldLabel>

        {sourceType === 'git' && (
          <FieldLabel label={t('settings.skills.addDialog.branch')}>
            <input
              type="text"
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
              placeholder={t('settings.skills.addDialog.branchPlaceholder')}
              className="w-full rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs outline-none focus:border-clay"
            />
          </FieldLabel>
        )}

        <FieldLabel label={t('settings.skills.addDialog.subdirectory')}>
          <input
            type="text"
            value={subdirectory}
            onChange={(e) => setSubdirectory(e.target.value)}
            placeholder={t('settings.skills.addDialog.subdirectoryPlaceholder')}
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
            {t('settings.skills.addDialog.autoUpdate')}
          </label>
        )}

        {error && (
          <div className="mt-3 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            {error}
          </div>
        )}

        {saving && (
          <div className="mt-4 flex items-center gap-2.5 rounded-md border border-primary/30 bg-primary/5 px-3 py-2.5">
            <svg className="h-4 w-4 animate-spin text-primary" viewBox="0 0 24 24" fill="none">
              <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" strokeLinecap="round" className="opacity-20" />
              <path d="M12 2a10 10 0 0 1 10 10" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
            </svg>
            <span className="text-xs text-primary">
              {sourceType === 'git' ? t('settings.skills.addDialog.cloningGit') : t('settings.skills.addDialog.addingSource')}
            </span>
          </div>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={saving}
            className="rounded-md border border-border px-3 py-1.5 text-xs hover:bg-foreground/5 disabled:opacity-50"
          >
            {t('settings.skills.addDialog.cancel')}
          </button>
          <button
            type="button"
            onClick={() => void handleSubmit()}
            disabled={!id || !url || saving}
            className="rounded-md bg-primary px-3 py-1.5 text-xs text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {saving ? t('settings.skills.addDialog.submitting') : t('settings.skills.addDialog.submit')}
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
  const { t } = useTranslation();
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
        {t('settings.skills.globalSkills')}
      </h2>
      <p className="mb-3 text-[11px] text-muted-foreground">
        {t('settings.skills.globalSkillsDesc')}
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
                {t('settings.skills.skillCount', { count: skills.length })}
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
        'inline-flex items-center gap-1 whitespace-nowrap rounded-md border px-2 py-1 text-[11px] transition-colors disabled:opacity-50 ' +
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
        'rounded-md border px-3 py-1.5 text-xs transition-colors ' +
        (on
          ? 'border-foreground bg-foreground text-background'
          : 'border-border hover:border-clay')
      }
    >
      {children}
    </button>
  );
}

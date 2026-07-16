import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog } from './ui/dialog';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { useProjects } from '@/store/projects';
import { useUiState } from '@/store/ui';
import { Sparkles, X, Plus, Lock, AlertCircle, Search } from 'lucide-react';

/**
 * The auto-detect modal (D12 main path).
 *
 * Shows projects discovered across all enabled scan roots with a search
 * filter, a multi-select list, and a bottom panel for managing scan roots.
 */
export function DetectedProjectsModal() {
  const { t } = useTranslation();
  const open = useProjects((s) => s.detectedModalOpen);
  const close = useProjects((s) => s.closeDetectedModal);
  const detected = useProjects((s) => s.detected);
  const scanRoots = useProjects((s) => s.scanRoots);
  const refresh = useProjects((s) => s.refreshDetected);
  const addRoot = useProjects((s) => s.addScanRoot);
  const removeRoot = useProjects((s) => s.removeScanRoot);
  const toggleRoot = useProjects((s) => s.setScanRootEnabled);
  const addProject = useProjects((s) => s.addProject);
  const setActiveProject = useUiState((s) => s.setActiveProject);

  const [filter, setFilter] = useState('');
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [newRoot, setNewRoot] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset selection / filter every time the modal opens.
  useEffect(() => {
    if (open) {
      setFilter('');
      setSelected(new Set());
      setError(null);
    }
  }, [open]);

  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return detected.filter((d) => {
      if (d.alreadyAdded) return false;
      if (q === '') return true;
      return d.path.toLowerCase().includes(q);
    });
  }, [detected, filter]);

  function toggleSelect(path: string) {
    const next = new Set(selected);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    setSelected(next);
  }

  async function onAddSelected() {
    if (selected.size === 0) return;
    setBusy(true);
    setError(null);
    let firstAddedPath: string | null = null;
    try {
      // Sequential to keep ordering and surface first error clearly.
      for (const path of selected) {
        const project = await addProject(path);
        if (firstAddedPath === null) firstAddedPath = project.path;
      }
      // Jump to the first newly added project so the user immediately sees
      // the result of their action without a manual sidebar click.
      if (firstAddedPath !== null) setActiveProject(firstAddedPath);
      close();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onAddRoot() {
    const trimmed = newRoot.trim();
    if (trimmed === '') return;
    setError(null);
    try {
      await addRoot(trimmed);
      setNewRoot('');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()} size="lg">
      <div className="flex max-h-[70vh] flex-col gap-3">
        <div className="flex items-center gap-2">
          <Sparkles className="h-4 w-4 text-primary" />
          <h2 className="text-base font-semibold">{t('detected.title')}</h2>
          <span className="text-xs text-muted-foreground">
            {t('detected.summary', { total: detected.length, unadded: visible.length })}
          </span>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void refresh()}
            className="ml-auto"
          >
            {t('detected.rescan')}
          </Button>
        </div>

        <div className="relative">
          <Search
            className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-primary/70"
            aria-hidden
          />
          <Input
            autoFocus
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={t('detected.searchPlaceholder')}
            className="h-9 border-primary/40 pl-8 text-sm focus:border-primary focus:ring-2 focus:ring-ring/30"
          />
        </div>

        <div className="flex-1 overflow-auto rounded border border-border">
          {visible.length === 0 ? (
            <div className="p-6 text-center text-sm text-muted-foreground">
              {detected.length === 0
                ? t('detected.noneDetected')
                : t('detected.allTracked')}
            </div>
          ) : (
            <ul className="divide-y divide-border">
              {visible.map((d) => (
                <li key={d.path} className="flex items-center gap-3 px-3 py-2">
                  <input
                    type="checkbox"
                    checked={selected.has(d.path)}
                    onChange={() => toggleSelect(d.path)}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-mono text-xs">{d.path}</div>
                    <div className="flex items-center gap-2 text-[10px] text-muted-foreground">
                      <span>{d.sourceKind === 'cc_projects_meta' ? t('detected.sourceCcHistory') : t('detected.sourceCustom')}</span>
                      {d.signals.length > 0 && <span>· {d.signals.join(', ')}</span>}
                    </div>
                  </div>
                  {d.signals.includes('missing') && (
                    <span title={t('detected.pathMissing')} className="text-destructive">
                      <AlertCircle className="h-4 w-4" />
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>

        {/* Scan roots management */}
        <div className="rounded border border-border bg-muted/30 p-3">
          <div className="mb-2 text-xs uppercase tracking-wider text-muted-foreground">
            {t('detected.scanRootsHeader')}
          </div>
          <div className="mb-2 flex flex-wrap gap-2">
            {scanRoots.map((r) => (
              <ScanRootChip
                key={r.path}
                path={r.path}
                builtin={r.builtin}
                enabled={r.enabled}
                onRemove={() => void removeRoot(r.path)}
                onToggle={() => void toggleRoot(r.path, !r.enabled)}
              />
            ))}
          </div>
          <div className="flex items-center gap-2">
            <Input
              value={newRoot}
              onChange={(e) => setNewRoot(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void onAddRoot();
              }}
              placeholder={t('detected.scanRootPlaceholder')}
              className="h-7 font-mono text-xs"
            />
            <Button size="sm" variant="outline" onClick={() => void onAddRoot()}>
              <Plus className="h-3.5 w-3.5" />
              {t('detected.addRoot')}
            </Button>
          </div>
        </div>

        {error && (
          <div className="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            {error}
          </div>
        )}

        <div className="flex items-center justify-between">
          <div className="text-xs text-muted-foreground">{t('detected.selectedCount', { count: selected.size })}</div>
          <div className="flex gap-2">
            <Button variant="outline" onClick={close} disabled={busy}>
              {t('detected.skip')}
            </Button>
            <Button onClick={() => void onAddSelected()} disabled={busy || selected.size === 0}>
              {t('detected.addSelected', { count: selected.size })}
            </Button>
          </div>
        </div>
      </div>
    </Dialog>
  );
}

function ScanRootChip({
  path,
  builtin,
  enabled,
  onRemove,
  onToggle,
}: {
  path: string;
  builtin: boolean;
  enabled: boolean;
  onRemove: () => void;
  onToggle: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div
      className={
        'inline-flex items-center gap-1.5 rounded-full border px-2 py-1 text-[11px] font-mono ' +
        (enabled ? 'border-success/60 bg-success/10' : 'border-border bg-muted/30 opacity-60')
      }
    >
      <button
        type="button"
        onClick={onToggle}
        title={enabled ? t('detected.scanRootClickDisable') : t('detected.scanRootClickEnable')}
        className="flex items-center gap-1.5"
      >
        {builtin && <Lock className="h-3 w-3 text-muted-foreground" />}
        <span>{path.replace(/^\/Users\/[^/]+/, '~')}</span>
      </button>
      {!builtin && (
        <button
          type="button"
          onClick={onRemove}
          aria-label={t('detected.removeScanRoot', { path })}
          className="rounded p-0.5 text-destructive hover:bg-destructive/10"
        >
          <X className="h-3 w-3" />
        </button>
      )}
    </div>
  );
}

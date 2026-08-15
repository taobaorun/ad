import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, RefreshCw, RotateCcw, ShieldAlert, Trash2 } from 'lucide-react';
import { useSkills } from '@/store/skills';
import type {
  SkillCatalogPlanView,
  SkillSourcePreviewProgress,
  SkillSourceRequest,
} from '@/lib/skillCatalogTypes';
import { Button } from './ui/button';
import { SkillSourceAddDialog } from './SkillSourceAddDialog';
import { SkillCatalogPlanDialog } from './SkillCatalogPlanDialog';

export function SkillSourcesSection() {
  const { t } = useTranslation();
  const sources = useSkills((state) => state.sources);
  const loadSources = useSkills((state) => state.loadSources);
  const previewAddSource = useSkills((state) => state.previewAddSource);
  const previewUpdateSource = useSkills((state) => state.previewUpdateSource);
  const previewRollbackSourceUpdate = useSkills((state) => state.previewRollbackSourceUpdate);
  const previewRemoveSource = useSkills((state) => state.previewRemoveSource);
  const applySourcePlan = useSkills((state) => state.applySourcePlan);
  const cancelSourcePlan = useSkills((state) => state.cancelSourcePlan);
  const [addOpen, setAddOpen] = useState(false);
  const [plan, setPlan] = useState<SkillCatalogPlanView | null>(null);
  const [previewing, setPreviewing] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [previewProgress, setPreviewProgress] = useState<SkillSourcePreviewProgress | null>(null);
  const [previewStartedAt, setPreviewStartedAt] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [rollbackCandidate, setRollbackCandidate] = useState<{
    receiptId: string;
    sourceId: string;
  } | null>(null);

  useEffect(() => {
    let active = true;
    loadSources()
      .catch((reason: unknown) => active && setError(String(reason)))
      .finally(() => active && setLoading(false));
    return () => {
      active = false;
    };
  }, [loadSources]);

  async function previewAdd(request: SkillSourceRequest) {
    setError(null);
    setPreviewing('add');
    setPreviewStartedAt(Date.now());
    setPreviewProgress({ sequence: 1, phase: 'preparing' });
    try {
      const nextPlan = await previewAddSource(request, setPreviewProgress);
      setAddOpen(false);
      setPlan(nextPlan);
    } finally {
      setPreviewing(null);
      setPreviewProgress(null);
      setPreviewStartedAt(null);
    }
  }

  async function previewExisting(sourceId: string, action: 'update' | 'remove') {
    setError(null);
    setPreviewing(`${action}:${sourceId}`);
    try {
      setPlan(
        action === 'update'
          ? await previewUpdateSource(sourceId)
          : await previewRemoveSource(sourceId),
      );
    } catch (reason) {
      setError(String(reason));
    } finally {
      setPreviewing(null);
    }
  }

  async function cancelPlan() {
    const current = plan;
    setPlan(null);
    if (current) await cancelSourcePlan(current.id).catch(() => undefined);
  }

  async function previewRollback(receiptId: string) {
    setError(null);
    setPreviewing(`rollback:${receiptId}`);
    try {
      setPlan(await previewRollbackSourceUpdate(receiptId));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setPreviewing(null);
    }
  }

  async function applyPlan() {
    if (!plan) return;
    setApplying(true);
    setError(null);
    try {
      const report = await applySourcePlan(plan);
      setPlan(null);
      const receipt = report.receipt;
      if (
        report.outcome === 'changed' &&
        receipt?.binding?.sourceType === 'git' &&
        receipt.previousBinding?.sourceType === 'git'
      ) {
        setRollbackCandidate({ receiptId: receipt.id, sourceId: receipt.sourceId });
      } else if (receipt && rollbackCandidate?.sourceId === receipt.sourceId) {
        setRollbackCandidate(null);
      }
      if (report.outcome === 'compensated' || report.outcome === 'partial_failure') {
        setError(report.issues.join('\n') || t('settings.skills.operationFailed'));
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setApplying(false);
    }
  }

  return (
    <>
      <header className="mb-5">
        <h1 className="text-xl font-semibold leading-tight">{t('settings.skills.title')}</h1>
        <p className="mt-1 text-sm text-muted-foreground">{t('settings.skills.desc')}</p>
      </header>

      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <h2 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            {t('settings.skills.sourcesHeading')}
          </h2>
          <p className="mt-1 text-[11px] text-muted-foreground">
            {t('settings.skills.catalogIsolation')}
          </p>
        </div>
        <Button type="button" size="sm" variant="outline" onClick={() => setAddOpen(true)}>
          <Plus className="h-3.5 w-3.5" /> {t('settings.skills.addSource')}
        </Button>
      </div>

      {error && (
        <div
          role="alert"
          className="mb-3 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          {error}
        </div>
      )}

      {loading && (
        <div className="text-xs text-muted-foreground">{t('settings.skills.loading')}</div>
      )}
      {!loading && sources.length === 0 && (
        <div className="rounded-lg border border-dashed border-border p-6 text-center text-xs text-muted-foreground">
          {t('settings.skills.noSources')}
        </div>
      )}

      <div className="space-y-2">
        {sources.map((source) => {
          const payload = source.currentBinding ?? source.currentArtifact;
          if (!payload) return null;
          const impact = payload.activationImpact;
          const executableCount =
            impact.hooks.length +
            impact.mcp.length +
            impact.commands.length +
            impact.scripts.length +
            impact.binaries.length;
          const updating = previewing === `update:${source.sourceId}`;
          const removing = previewing === `remove:${source.sourceId}`;
          const rollingBack =
            rollbackCandidate?.sourceId === source.sourceId &&
            previewing === `rollback:${rollbackCandidate.receiptId}`;
          return (
            <article key={source.sourceId} className="rounded-lg border border-border p-3">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="text-sm font-medium">{source.displayName}</h3>
                    <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] uppercase text-muted-foreground">
                      {source.sourceType}
                    </span>
                  </div>
                  <div className="mt-1 break-all font-mono text-[11px] text-muted-foreground">
                    {source.location}
                  </div>
                  <div className="mt-1 text-[11px] text-muted-foreground">
                    {t('settings.skills.artifactSummary', {
                      count: payload.skills.length,
                      revision: shortRevision(payload.sourceRevision),
                    })}
                  </div>
                  {source.currentBinding && (
                    <div className="mt-1 break-all font-mono text-[10px] text-muted-foreground">
                      {source.currentBinding.stableRoot}
                    </div>
                  )}
                  {executableCount > 0 && (
                    <div className="text-warning-foreground mt-2 inline-flex items-center gap-1.5 rounded border border-warning/30 bg-warning/10 px-2 py-1 text-[11px]">
                      <ShieldAlert className="h-3 w-3" />
                      {t('settings.skills.activationImpact', { count: executableCount })}
                    </div>
                  )}
                </div>
                <div className="flex shrink-0 gap-1.5">
                  {rollbackCandidate?.sourceId === source.sourceId && (
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      disabled={previewing !== null}
                      onClick={() => void previewRollback(rollbackCandidate.receiptId)}
                    >
                      <RotateCcw className={`h-3 w-3 ${rollingBack ? 'animate-spin' : ''}`} />
                      {rollingBack
                        ? t('settings.skills.previewing')
                        : t('settings.skills.rollback')}
                    </Button>
                  )}
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    disabled={previewing !== null}
                    onClick={() => void previewExisting(source.sourceId, 'update')}
                  >
                    <RefreshCw className={`h-3 w-3 ${updating ? 'animate-spin' : ''}`} />
                    {updating ? t('settings.skills.previewing') : t('settings.skills.update')}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    aria-label={t('settings.skills.removeSource', { name: source.displayName })}
                    disabled={previewing !== null}
                    onClick={() => void previewExisting(source.sourceId, 'remove')}
                  >
                    <Trash2 className={`h-3.5 w-3.5 ${removing ? 'animate-pulse' : ''}`} />
                  </Button>
                </div>
              </div>
            </article>
          );
        })}
      </div>

      <SkillSourceAddDialog
        open={addOpen}
        busy={previewing === 'add'}
        progress={previewProgress}
        startedAt={previewStartedAt}
        onOpenChange={setAddOpen}
        onPreview={previewAdd}
      />
      <SkillCatalogPlanDialog
        plan={plan}
        busy={applying}
        onCancel={() => void cancelPlan()}
        onConfirm={() => void applyPlan()}
      />
    </>
  );
}

function shortRevision(revision: string): string {
  const value = revision.split(':').at(-1) ?? revision;
  return value.length > 12 ? value.slice(0, 12) : value;
}

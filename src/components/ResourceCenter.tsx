import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Boxes,
  GitBranch,
  Link2,
  Network,
  Plus,
  RefreshCw,
  RotateCcw,
  Search,
  Trash2,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { tauri } from '@/lib/tauri';
import type {
  CatalogResource,
  CatalogSource,
  ResourceCatalogSnapshot,
  ResourceRemovalPlan,
  ResourceRemovalProgress,
  ResourceRemovalOperation,
  SourceRemovalPlan,
  SourceRemovalProgress,
} from '@/lib/resourceCatalogTypes';
import { formatAgentErrorMessage } from '@/lib/agentErrors';
import { Button } from './ui/button';
import { SkillSourceAddDialog } from './SkillSourceAddDialog';
import { SkillCatalogPlanDialog } from './SkillCatalogPlanDialog';
import type { SkillCatalogPlanView, SkillSourceRequest } from '@/lib/skillCatalogTypes';
import { ResourceRemovalDialog } from './ResourceRemovalDialog';
import { SourceRemovalDialog } from './SourceRemovalDialog';

type Filter = 'all' | 'skills' | 'plugins';

export function ResourceCenter() {
  const { t } = useTranslation();
  const [catalog, setCatalog] = useState<ResourceCatalogSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<Filter>('all');
  const [addOpen, setAddOpen] = useState(false);
  const [sourcePlan, setSourcePlan] = useState<SkillCatalogPlanView | null>(null);
  const [sourceBusy, setSourceBusy] = useState(false);
  const [removingResource, setRemovingResource] = useState<CatalogResource | null>(null);
  const [removalPlan, setRemovalPlan] = useState<ResourceRemovalPlan | null>(null);
  const [removalProgress, setRemovalProgress] = useState<ResourceRemovalProgress | null>(null);
  const [removalBusy, setRemovalBusy] = useState(false);
  const [removalOperation, setRemovalOperation] = useState<ResourceRemovalOperation | null>(null);
  const [readdingResourceId, setReaddingResourceId] = useState<string | null>(null);
  const [sourceActionId, setSourceActionId] = useState<string | null>(null);
  const [removingSource, setRemovingSource] = useState<CatalogSource | null>(null);
  const [sourceRemovalPlan, setSourceRemovalPlan] = useState<SourceRemovalPlan | null>(null);
  const [sourceRemovalProgress, setSourceRemovalProgress] = useState<SourceRemovalProgress | null>(
    null,
  );
  const [sourceRemovalBusy, setSourceRemovalBusy] = useState(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [nextCatalog, operations] = await Promise.all([
        tauri.listResourceCatalog(),
        tauri.listResourceRemovalOperations(),
      ]);
      setCatalog(nextCatalog);
      const resumable = operations.filter((operation) => operation.phase !== 'complete').at(-1);
      setRemovalOperation(resumable ?? null);
      if (resumable) {
        setRemovingResource(nextCatalog.resources[resumable.resourceId] ?? null);
        setRemovalProgress({
          operationId: resumable.operationId,
          sequence: 1,
          phase: resumable.phase,
          completed: resumable.completed,
          total: resumable.total,
        });
        setRemovalPlan(null);
      }
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => void load(), [load]);

  const resources = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return Object.values(catalog?.resources ?? {})
      .filter((resource) => resource.present)
      .filter((resource) => filter === 'all' || resource.kind === filter)
      .filter((resource) => {
        const source = catalog?.sources[resource.sourceId];
        return (
          !normalized ||
          [resource.displayName, resource.installId, source?.displayName].some((value) =>
            value?.toLowerCase().includes(normalized),
          )
        );
      })
      .sort((left, right) => left.displayName.localeCompare(right.displayName));
  }, [catalog, filter, query]);
  const managedResources = resources.filter((resource) => resource.lifecycle === 'managed');
  const suppressedResources = resources.filter((resource) => resource.lifecycle === 'suppressed');

  async function previewAdd(request: SkillSourceRequest) {
    setSourceBusy(true);
    try {
      setSourcePlan(await tauri.previewAddSkillCatalogSource(request));
      setAddOpen(false);
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setSourceBusy(false);
    }
  }

  async function applySourcePlan() {
    if (!sourcePlan) return;
    setSourceBusy(true);
    try {
      await tauri.applySkillCatalogSourcePlan(sourcePlan);
      setSourcePlan(null);
      await load();
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setSourceBusy(false);
    }
  }

  async function previewSourceUpdate(source: CatalogSource) {
    setSourceActionId(source.id);
    setError(null);
    try {
      setSourcePlan(await tauri.previewUpdateSkillCatalogSource(source.id));
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setSourceActionId(null);
    }
  }

  async function previewSourceRemoval(source: CatalogSource) {
    setRemovingSource(source);
    setSourceRemovalPlan(null);
    setSourceRemovalProgress(null);
    setError(null);
    try {
      setSourceRemovalPlan(await tauri.previewRemoveCatalogSource(source.id));
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    }
  }

  async function applySourceRemoval() {
    if (!sourceRemovalPlan) return;
    setSourceRemovalBusy(true);
    setError(null);
    try {
      const report = await tauri.applyRemoveCatalogSource(
        sourceRemovalPlan,
        setSourceRemovalProgress,
      );
      if (report.phase === 'complete') {
        setRemovingSource(null);
        setSourceRemovalPlan(null);
        setSourceRemovalProgress(null);
        await load();
      } else {
        setError(t('resourceCenter.sourceRemove.retryHint'));
        setSourceRemovalPlan(await tauri.previewRemoveCatalogSource(sourceRemovalPlan.sourceId));
      }
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setSourceRemovalBusy(false);
    }
  }

  async function previewRemoval(resource: CatalogResource) {
    setRemovingResource(resource);
    setRemovalProgress(null);
    setRemovalOperation(null);
    setError(null);
    try {
      setRemovalPlan(await tauri.previewRemoveCatalogResource(resource.id));
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    }
  }

  async function applyRemoval() {
    if (!removalPlan) return;
    setRemovalBusy(true);
    try {
      const report = await tauri.applyRemoveCatalogResource(removalPlan, setRemovalProgress);
      if (report.phase === 'complete') {
        setRemovingResource(null);
        setRemovalPlan(null);
        setRemovalProgress(null);
        setRemovalOperation(null);
        await load();
      } else {
        setRemovalOperation({
          schemaVersion: 1,
          operationId: report.operationId,
          resourceId: report.resourceId,
          startedAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
          phase: report.phase,
          completed: report.completed,
          total: report.total,
          installations: report.installations,
        });
        setError(t('resourceCenter.remove.retryHint'));
        setRemovalPlan(await tauri.previewRemoveCatalogResource(removalPlan.resourceId));
      }
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setRemovalBusy(false);
    }
  }

  async function retryRemoval() {
    if (!removalOperation) return;
    setRemovalBusy(true);
    try {
      const report = await tauri.retryRemoveCatalogResource(
        removalOperation.operationId,
        setRemovalProgress,
      );
      if (report.phase === 'complete') {
        setRemovingResource(null);
        setRemovalPlan(null);
        setRemovalProgress(null);
        setRemovalOperation(null);
        await load();
      }
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setRemovalBusy(false);
    }
  }

  async function readd(resource: CatalogResource) {
    setReaddingResourceId(resource.id);
    setError(null);
    try {
      setCatalog(await tauri.readdCatalogResource(resource.id));
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setReaddingResourceId(null);
    }
  }

  return (
    <section
      className="h-full w-full overflow-y-auto bg-background"
      aria-labelledby="harness-title"
    >
      <div className="mx-auto w-full max-w-[1180px] px-6 py-8 lg:px-10">
        <header>
          <div>
            <div className="mb-2 inline-flex items-center gap-2 text-xs font-medium text-muted-foreground">
              <Boxes className="h-4 w-4" /> {t('resourceCenter.eyebrow')}
            </div>
            <h1 id="harness-title" className="text-2xl font-semibold tracking-tight">
              {t('resourceCenter.title')}
            </h1>
            <p className="mt-1 text-sm text-muted-foreground">{t('resourceCenter.subtitle')}</p>
          </div>
        </header>

        <nav
          className="mt-6 flex items-center gap-1 border-b border-border"
          aria-label={t('resourceCenter.capabilityNavLabel')}
        >
          <a
            href="#harness-skills-plugins"
            aria-current="page"
            className="inline-flex items-center gap-2 border-b-2 border-primary px-3 py-2 text-sm font-medium text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <Boxes className="h-4 w-4 text-primary" />
            {t('resourceCenter.sectionTitle')}
          </a>
          <span
            role="link"
            aria-disabled="true"
            className="inline-flex cursor-not-allowed items-center gap-2 border-b-2 border-transparent px-3 py-2 text-sm text-muted-foreground opacity-70"
          >
            <Network className="h-4 w-4" />
            <span>{t('resourceCenter.capability.mcp')}</span>
            <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] font-medium">
              {t('resourceCenter.capability.mcpStatus')}
            </span>
          </span>
        </nav>

        <section
          id="harness-skills-plugins"
          className="mt-6 scroll-mt-4"
          aria-labelledby="skills-plugins-title"
        >
          <header className="flex flex-wrap items-end justify-between gap-4">
            <div>
              <h2 id="skills-plugins-title" className="text-base font-semibold tracking-tight">
                {t('resourceCenter.sectionTitle')}
              </h2>
              <p className="mt-1 text-xs text-muted-foreground">
                {t('resourceCenter.sectionSubtitle')}
              </p>
            </div>
            <Button type="button" onClick={() => setAddOpen(true)}>
              <Plus className="h-4 w-4" /> {t('resourceCenter.addSource')}
            </Button>
          </header>

          <div className="mt-4 flex flex-wrap gap-3 rounded-xl border border-border bg-card p-3">
            <label
              data-input-shell=""
              className="flex min-w-[220px] flex-1 items-center gap-2 rounded-lg bg-muted/60 px-3"
            >
              <Search className="h-4 w-4 text-muted-foreground" />
              <span className="sr-only">{t('resourceCenter.search')}</span>
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t('resourceCenter.search')}
                className="h-9 min-w-0 flex-1 bg-transparent text-sm outline-none"
              />
            </label>
            <div
              className="flex rounded-lg bg-muted/60 p-1"
              role="group"
              aria-label={t('resourceCenter.filterLabel')}
            >
              {(['all', 'skills', 'plugins'] as const).map((value) => (
                <button
                  key={value}
                  type="button"
                  aria-pressed={filter === value}
                  onClick={() => setFilter(value)}
                  className="rounded-md px-3 py-1.5 text-xs font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  style={{ background: filter === value ? 'var(--ds-bg-card)' : 'transparent' }}
                >
                  {t(`resourceCenter.filter.${value}`)}
                </button>
              ))}
            </div>
            <Button type="button" variant="ghost" size="sm" onClick={() => void load()}>
              <RefreshCw className="h-3.5 w-3.5" /> {t('resourceCenter.refresh')}
            </Button>
          </div>

          {catalog && Object.values(catalog.sources).length > 0 && (
            <div
              className="mt-4 flex flex-wrap items-center gap-2"
              aria-label={t('resourceCenter.sources')}
            >
              <span className="mr-1 text-xs font-medium text-muted-foreground">
                {t('resourceCenter.sources')}
              </span>
              {Object.values(catalog.sources)
                .sort((left, right) => left.displayName.localeCompare(right.displayName))
                .map((source) => (
                  <div
                    key={source.id}
                    className="inline-flex items-center gap-1 rounded-full border border-border bg-card py-1 pl-2.5 pr-1"
                  >
                    {source.sourceType === 'git' ? (
                      <GitBranch className="h-3 w-3 text-muted-foreground" />
                    ) : (
                      <Link2 className="h-3 w-3 text-muted-foreground" />
                    )}
                    <span className="max-w-44 truncate text-xs">{source.displayName}</span>
                    <span className="text-[9px] uppercase text-muted-foreground">
                      {source.sourceType}
                    </span>
                    <button
                      type="button"
                      disabled={sourceActionId !== null}
                      onClick={() => void previewSourceUpdate(source)}
                      aria-label={t('resourceCenter.updateSource', { name: source.displayName })}
                      className="rounded-full p-1 text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-50"
                    >
                      <RefreshCw
                        className={`h-3 w-3 ${sourceActionId === source.id ? 'animate-spin' : ''}`}
                      />
                    </button>
                    <button
                      type="button"
                      disabled={sourceActionId !== null}
                      onClick={() => void previewSourceRemoval(source)}
                      aria-label={t('resourceCenter.removeSource', { name: source.displayName })}
                      className="rounded-full p-1 text-muted-foreground hover:bg-destructive/10 hover:text-destructive disabled:opacity-50"
                    >
                      <Trash2 className="h-3 w-3" />
                    </button>
                  </div>
                ))}
            </div>
          )}

          {error && (
            <div
              role="alert"
              className="mt-4 rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive"
            >
              {error}
            </div>
          )}
          {loading ? (
            <div className="py-20 text-center text-sm text-muted-foreground">
              {t('resourceCenter.loading')}
            </div>
          ) : resources.length === 0 ? (
            <div className="mt-6 rounded-xl border border-dashed border-border px-6 py-16 text-center">
              <Boxes className="mx-auto h-7 w-7 text-muted-foreground" />
              <h2 className="mt-3 text-sm font-medium">{t('resourceCenter.empty')}</h2>
              <p className="mt-1 text-xs text-muted-foreground">{t('resourceCenter.emptyHint')}</p>
            </div>
          ) : managedResources.length === 0 ? (
            <div className="mt-6 rounded-xl border border-dashed border-border px-6 py-12 text-center">
              <Boxes className="mx-auto h-7 w-7 text-muted-foreground" />
              <h2 className="mt-3 text-sm font-medium">{t('resourceCenter.noManaged')}</h2>
            </div>
          ) : (
            <div className="mt-6 grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
              {managedResources.map((resource) => (
                <ResourceCard
                  key={resource.id}
                  resource={resource}
                  source={catalog?.sources[resource.sourceId]}
                  onRemove={() => void previewRemoval(resource)}
                  onReadd={() => void readd(resource)}
                  busy={readdingResourceId === resource.id}
                />
              ))}
            </div>
          )}
          {!loading && suppressedResources.length > 0 && (
            <details className="mt-8 rounded-xl border border-border bg-card">
              <summary className="cursor-pointer list-none px-4 py-3 text-xs font-medium text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                {t('resourceCenter.removedResources', { count: suppressedResources.length })}
              </summary>
              <div className="grid gap-3 border-t border-border p-3 sm:grid-cols-2 xl:grid-cols-3">
                {suppressedResources.map((resource) => (
                  <ResourceCard
                    key={resource.id}
                    resource={resource}
                    source={catalog?.sources[resource.sourceId]}
                    onRemove={() => void previewRemoval(resource)}
                    onReadd={() => void readd(resource)}
                    busy={readdingResourceId === resource.id}
                  />
                ))}
              </div>
            </details>
          )}
        </section>
      </div>
      <SkillSourceAddDialog
        open={addOpen}
        busy={sourceBusy}
        resourceMode
        onOpenChange={setAddOpen}
        onPreview={previewAdd}
      />
      <SkillCatalogPlanDialog
        plan={sourcePlan}
        busy={sourceBusy}
        resourceMode
        onCancel={() => setSourcePlan(null)}
        onConfirm={() => void applySourcePlan()}
      />
      <ResourceRemovalDialog
        resource={removingResource}
        plan={
          removalPlan ??
          (removalOperation && removingResource
            ? {
                planId: removalOperation.operationId,
                resourceId: removalOperation.resourceId,
                resourceName: removingResource.displayName,
                expectedCatalogRevision: catalog?.revision ?? 0,
                affectedProjectCount: new Set(
                  removalOperation.installations.map((item) => item.projectPath),
                ).size,
                affectedAgentCount: removalOperation.installations.length,
                installations: removalOperation.installations,
                riskFingerprint: removalOperation.operationId,
                expiresAt: removalOperation.updatedAt,
              }
            : null)
        }
        progress={removalProgress}
        busy={removalBusy}
        error={removingResource ? error : null}
        onCancel={() => {
          setRemovingResource(null);
          setRemovalPlan(null);
          setRemovalProgress(null);
          setRemovalOperation(null);
        }}
        onConfirm={() => void (removalOperation ? retryRemoval() : applyRemoval())}
      />
      <SourceRemovalDialog
        source={removingSource}
        plan={sourceRemovalPlan}
        progress={sourceRemovalProgress}
        busy={sourceRemovalBusy}
        error={removingSource ? error : null}
        onCancel={() => {
          setRemovingSource(null);
          setSourceRemovalPlan(null);
          setSourceRemovalProgress(null);
        }}
        onConfirm={() => void applySourceRemoval()}
      />
    </section>
  );
}

function ResourceCard({
  resource,
  source,
  onRemove,
  onReadd,
  busy,
}: {
  resource: CatalogResource;
  source?: CatalogSource;
  onRemove: () => void;
  onReadd: () => void;
  busy: boolean;
}) {
  const { t } = useTranslation();
  return (
    <article className="group rounded-xl border border-border bg-card p-4 transition-colors hover:border-border/80">
      <div className="flex items-start gap-3">
        <div className="rounded-lg bg-muted p-2 text-muted-foreground">
          {resource.kind === 'skills' ? (
            <Link2 className="h-4 w-4" />
          ) : (
            <Boxes className="h-4 w-4" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h2 className="truncate text-sm font-semibold" title={resource.displayName}>
              {resource.displayName}
            </h2>
            <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] uppercase tracking-wide text-muted-foreground">
              {t(`resourceCenter.kind.${resource.kind}`)}
            </span>
            {resource.lifecycle === 'suppressed' && (
              <span className="text-warning-foreground rounded-full bg-warning/10 px-2 py-0.5 text-[10px]">
                {t('resourceCenter.suppressed')}
              </span>
            )}
          </div>
          {resource.description && (
            <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
              {resource.description}
            </p>
          )}
          <div className="mt-2 flex flex-wrap gap-1">
            {resource.compatibleAgents.length > 0 ? (
              resource.compatibleAgents.map((agent) => (
                <span
                  key={agent}
                  className="rounded-full border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground"
                >
                  {t(`resourceCenter.agent.${agent}`)}
                </span>
              ))
            ) : (
              <span className="rounded-full border border-warning/40 px-1.5 py-0.5 text-[10px] text-muted-foreground">
                {t('resourceCenter.unsupported')}
              </span>
            )}
          </div>
          <div className="mt-4 flex items-center justify-between gap-2">
            <div className="min-w-0 text-[11px] text-muted-foreground">
              <div className="flex items-center gap-1.5">
                {source?.sourceType === 'git' ? (
                  <GitBranch className="h-3 w-3" />
                ) : (
                  <Link2 className="h-3 w-3" />
                )}
                <span className="truncate">{source?.displayName}</span>
                <span className="uppercase">{source?.sourceType}</span>
              </div>
            </div>
            {resource.lifecycle === 'managed' ? (
              <button
                type="button"
                onClick={onRemove}
                aria-label={t('resourceCenter.remove.aria', { name: resource.displayName })}
                className="rounded-md p-1.5 text-muted-foreground opacity-70 hover:bg-destructive/10 hover:text-destructive focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring group-hover:opacity-100"
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            ) : (
              <button
                type="button"
                disabled={busy}
                onClick={onReadd}
                aria-label={t('resourceCenter.readdAria', { name: resource.displayName })}
                className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-primary hover:bg-primary/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
              >
                <RotateCcw className="h-3 w-3" /> {t('resourceCenter.readd')}
              </button>
            )}
          </div>
        </div>
      </div>
    </article>
  );
}

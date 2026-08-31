import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Boxes,
  Download,
  GitBranch,
  Link2,
  Network,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentErrorMessage } from '@/lib/agentErrors';
import type { ResourceSourceView } from '@/lib/agentResourceInventoryTypes';
import type {
  AgentContext,
  AgentInstallation,
  CapabilityDescriptor,
  InstallationId,
} from '@/lib/agentTypes';
import type {
  CatalogSource,
  ResourceCatalogSnapshot,
  SourceRemovalPlan,
  SourceRemovalProgress,
} from '@/lib/resourceCatalogTypes';
import type {
  SkillCatalogPlanView,
  SkillSourcePreviewProgress,
  SkillSourceRequest,
} from '@/lib/skillCatalogTypes';
import { tauri } from '@/lib/tauri';
import { useAgents } from '@/store/agents';

import { AgentCollectionPanel } from './AgentCollectionPanel';
import { SkillCatalogPlanDialog } from './SkillCatalogPlanDialog';
import { SkillSourceAddDialog } from './SkillSourceAddDialog';
import { SourceRemovalDialog } from './SourceRemovalDialog';
import { Button } from './ui/button';
import { Dialog } from './ui/dialog';

type Filter = 'all' | 'skills' | 'plugins';

export function ResourceCenter() {
  const { t } = useTranslation();
  const agents = useAgents((state) => state.agents);
  const installations = useAgents((state) => state.installations);
  const activeContext = useAgents((state) => state.activeContext);
  const activeAgentId = useAgents((state) => state.activeAgentId);
  const capabilitiesByAgent = useAgents((state) => state.capabilitiesByAgent);
  const [catalog, setCatalog] = useState<ResourceCatalogSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<Filter>('all');
  const [addOpen, setAddOpen] = useState(false);
  const [sourcePlan, setSourcePlan] = useState<SkillCatalogPlanView | null>(null);
  const [sourceBusy, setSourceBusy] = useState(false);
  const [sourcePreviewProgress, setSourcePreviewProgress] =
    useState<SkillSourcePreviewProgress | null>(null);
  const [sourcePreviewStartedAt, setSourcePreviewStartedAt] = useState<number | null>(null);
  const [sourceActionId, setSourceActionId] = useState<string | null>(null);
  const [removingSource, setRemovingSource] = useState<CatalogSource | null>(null);
  const [sourceRemovalPlan, setSourceRemovalPlan] = useState<SourceRemovalPlan | null>(null);
  const [sourceRemovalProgress, setSourceRemovalProgress] = useState<SourceRemovalProgress | null>(
    null,
  );
  const [sourceRemovalBusy, setSourceRemovalBusy] = useState(false);
  const [resourceReloadKey, setResourceReloadKey] = useState(0);
  const [installingSource, setInstallingSource] = useState<CatalogSource | null>(null);
  const [installDialogBusy, setInstallDialogBusy] = useState(false);
  const [installInstallationId, setInstallInstallationId] = useState<InstallationId | null>(null);

  const baseInstallations = useMemo(
    () =>
      installations.filter(
        (installation) => !installation.projectPath && !installation.baseInstallationId,
      ),
    [installations],
  );
  const defaultInstallation = useMemo(() => {
    const selected = installations.find(
      (installation) => installation.id === activeContext?.installationId,
    );
    const baseId = selected?.baseInstallationId ?? activeContext?.installationId;
    return (
      baseInstallations.find((installation) => installation.id === baseId) ??
      baseInstallations.find((installation) => installation.agentId === activeAgentId) ??
      baseInstallations[0]
    );
  }, [activeAgentId, activeContext?.installationId, baseInstallations, installations]);
  const installInstallation = useMemo(
    () =>
      baseInstallations.find((installation) => installation.id === installInstallationId) ??
      defaultInstallation,
    [baseInstallations, defaultInstallation, installInstallationId],
  );
  const installContext = useMemo(
    () => (installInstallation ? { installationId: installInstallation.id } : null),
    [installInstallation],
  );

  const load = useCallback(async () => {
    setError(null);
    try {
      setCatalog(await tauri.listResourceCatalog());
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => void load(), [load]);

  const catalogSources = useMemo(
    () =>
      Object.values(catalog?.sources ?? {}).sort((left, right) =>
        left.displayName.localeCompare(right.displayName),
      ),
    [catalog],
  );
  const visibleSources = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    const resources = Object.values(catalog?.resources ?? {}).filter(
      (resource) => resource.present,
    );
    return catalogSources.filter((source) => {
      const sourceResources = resources.filter((resource) => resource.sourceId === source.id);
      const kindMatches =
        filter === 'all' || sourceResources.some((resource) => resource.kind === filter);
      const queryMatches =
        !normalized ||
        [source.displayName, source.location, source.branch ?? '', source.subdirectory ?? ''].some(
          (value) => value.toLocaleLowerCase().includes(normalized),
        );
      return kindMatches && queryMatches;
    });
  }, [catalog, catalogSources, filter, query]);

  async function previewAdd(request: SkillSourceRequest) {
    const duplicate = catalogSources.find(
      (source) =>
        source.displayName.toLocaleLowerCase() === request.displayName.trim().toLocaleLowerCase() ||
        (source.sourceType === request.sourceType &&
          source.location === request.location &&
          source.branch === request.branch &&
          source.subdirectory === request.subdirectory),
    );
    if (duplicate) {
      setAddOpen(false);
      setQuery(duplicate.displayName);
      setNotice(
        t('resourceCenter.duplicateSource', {
          name: duplicate.displayName,
        }),
      );
      return;
    }
    setSourceBusy(true);
    setSourcePreviewStartedAt(Date.now());
    setSourcePreviewProgress({ sequence: 1, phase: 'preparing' });
    setError(null);
    setNotice(null);
    try {
      setSourcePlan(await tauri.previewAddSkillCatalogSource(request, setSourcePreviewProgress));
      setAddOpen(false);
    } catch (reason) {
      throw new Error(formatAgentErrorMessage(reason));
    } finally {
      setSourceBusy(false);
      setSourcePreviewProgress(null);
      setSourcePreviewStartedAt(null);
    }
  }

  async function applySourcePlan() {
    if (!sourcePlan) return;
    setSourceBusy(true);
    try {
      await tauri.applySkillCatalogSourcePlan(sourcePlan);
      setSourcePlan(null);
      await load();
      setResourceReloadKey((key) => key + 1);
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      setSourceBusy(false);
    }
  }

  async function previewSourceUpdate(source: CatalogSource) {
    setSourceActionId(source.id);
    setError(null);
    setNotice(null);
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
    setNotice(null);
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
        setResourceReloadKey((key) => key + 1);
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

  return (
    <section
      className="h-full w-full overflow-y-auto bg-background"
      aria-labelledby="harness-title"
    >
      <div className="mx-auto w-full max-w-[1180px] px-6 py-8 lg:px-10">
        <header>
          <div className="mb-2 inline-flex items-center gap-2 text-xs font-medium text-muted-foreground">
            <Boxes className="h-4 w-4" /> {t('resourceCenter.eyebrow')}
          </div>
          <h1 id="harness-title" className="text-2xl font-semibold tracking-tight">
            {t('resourceCenter.title')}
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">{t('resourceCenter.subtitle')}</p>
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
                onChange={(event) => {
                  setQuery(event.target.value);
                  setNotice(null);
                }}
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
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => {
                setResourceReloadKey((key) => key + 1);
                void load();
              }}
            >
              <RefreshCw className="h-3.5 w-3.5" /> {t('resourceCenter.refresh')}
            </Button>
          </div>

          {notice && (
            <div
              role="status"
              className="mt-4 rounded-lg border border-primary/30 bg-primary/5 px-4 py-3 text-sm text-foreground"
            >
              {notice}
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
          ) : visibleSources.length === 0 ? (
            <div className="mt-6 rounded-xl border border-dashed border-border px-6 py-16 text-center">
              <Boxes className="mx-auto h-7 w-7 text-muted-foreground" />
              <h2 className="mt-3 text-sm font-medium">{t('resourceCenter.empty')}</h2>
              <p className="mt-1 text-xs text-muted-foreground">{t('resourceCenter.emptyHint')}</p>
            </div>
          ) : (
            <div className="mt-6 grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
              {visibleSources.map((source) => (
                <SourceCard
                  key={source.id}
                  source={source}
                  busy={sourceActionId !== null}
                  updating={sourceActionId === source.id}
                  installDisabled={baseInstallations.length === 0}
                  onInstall={() => {
                    setInstallDialogBusy(false);
                    setInstallInstallationId(defaultInstallation?.id ?? null);
                    setInstallingSource(source);
                  }}
                  onUpdate={() => void previewSourceUpdate(source)}
                  onRemove={() => void previewSourceRemoval(source)}
                />
              ))}
            </div>
          )}
        </section>
      </div>

      <SkillSourceAddDialog
        open={addOpen}
        busy={sourceBusy}
        progress={sourcePreviewProgress}
        startedAt={sourcePreviewStartedAt}
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
      <UserSourceInstallationDialog
        source={installingSource}
        context={installContext}
        installation={installInstallation}
        installations={baseInstallations}
        agentNames={Object.fromEntries(agents.map((agent) => [agent.id, agent.displayName]))}
        capabilities={
          installInstallation ? (capabilitiesByAgent[installInstallation.agentId] ?? []) : []
        }
        onSelectInstallation={setInstallInstallationId}
        reloadKey={`${catalog?.revision ?? 0}:${resourceReloadKey}`}
        busy={installDialogBusy}
        onBusyChange={setInstallDialogBusy}
        onClose={() => !installDialogBusy && setInstallingSource(null)}
      />
    </section>
  );
}

function SourceCard({
  source,
  busy,
  updating,
  installDisabled,
  onInstall,
  onUpdate,
  onRemove,
}: {
  source: CatalogSource;
  busy: boolean;
  updating: boolean;
  installDisabled: boolean;
  onInstall: () => void;
  onUpdate: () => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  return (
    <article className="group rounded-xl border border-border bg-card p-4 transition-colors hover:border-border/80">
      <div className="flex items-start gap-3">
        <div className="rounded-lg bg-muted p-2 text-muted-foreground">
          {source.sourceType === 'git' ? (
            <GitBranch className="h-4 w-4" aria-hidden />
          ) : (
            <Link2 className="h-4 w-4" aria-hidden />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h2 className="truncate text-sm font-semibold" title={source.displayName}>
              {source.displayName}
            </h2>
            <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] uppercase tracking-wide text-muted-foreground">
              {source.sourceType}
            </span>
          </div>
          <p className="mt-2 break-all font-mono text-[11px] leading-relaxed text-muted-foreground">
            {source.location}
          </p>
          {(source.branch || source.subdirectory) && (
            <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
              {source.branch && (
                <span>{t('resourceCenter.sourceBranch', { branch: source.branch })}</span>
              )}
              {source.subdirectory && (
                <span className="break-all">
                  {t('resourceCenter.sourceSubdirectory', { subdirectory: source.subdirectory })}
                </span>
              )}
            </div>
          )}
          <div className="mt-4 flex justify-end gap-1">
            <Button
              type="button"
              size="sm"
              variant="secondary"
              className="mr-auto h-7 px-2 text-xs"
              disabled={busy || installDisabled}
              title={installDisabled ? t('resourceCenter.installDialog.unavailable') : undefined}
              onClick={onInstall}
            >
              <Download className="h-3.5 w-3.5" /> {t('resourceCenter.install')}
            </Button>
            <button
              type="button"
              disabled={busy}
              onClick={onUpdate}
              aria-label={t('resourceCenter.updateSource', { name: source.displayName })}
              className="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${updating ? 'animate-spin' : ''}`} />
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={onRemove}
              aria-label={t('resourceCenter.removeSource', { name: source.displayName })}
              className="rounded-md p-1.5 text-muted-foreground hover:bg-destructive/10 hover:text-destructive focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>
    </article>
  );
}

function UserSourceInstallationDialog({
  source,
  context,
  installation,
  installations,
  agentNames,
  capabilities,
  onSelectInstallation,
  reloadKey,
  busy,
  onBusyChange,
  onClose,
}: {
  source: CatalogSource | null;
  context: AgentContext | null;
  installation: AgentInstallation | undefined;
  installations: AgentInstallation[];
  agentNames: Record<string, string>;
  capabilities: CapabilityDescriptor[];
  onSelectInstallation: (installationId: InstallationId) => void;
  reloadKey: string;
  busy: boolean;
  onBusyChange: (busy: boolean) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const sourceFilter = useMemo<ResourceSourceView | undefined>(
    () =>
      source
        ? {
            kind: source.sourceType === 'git' ? 'catalog_git' : 'catalog_local',
            displayName: source.displayName,
            location: source.location,
            branch: source.branch,
            subdirectory: source.subdirectory,
          }
        : undefined,
    [source],
  );
  return (
    <Dialog
      open={source !== null}
      onOpenChange={(open) => !open && onClose()}
      closeDisabled={busy}
      size="lg"
      title={t('resourceCenter.installDialog.title', { name: source?.displayName ?? '' })}
      description={t('resourceCenter.installDialog.description')}
    >
      <div className="mb-3 flex flex-wrap items-end justify-between gap-3 rounded-lg border border-border bg-muted/30 px-3 py-2">
        <label className="min-w-[220px] flex-1 text-xs font-medium text-foreground">
          <span className="mb-1 block text-muted-foreground">
            {t('resourceCenter.installDialog.agentLabel')}
          </span>
          <select
            value={installation?.id ?? ''}
            disabled={busy || installations.length === 0}
            onChange={(event) => onSelectInstallation(event.target.value as InstallationId)}
            className="h-9 w-full rounded-md border border-border bg-card px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {installations.map((candidate) => (
              <option key={candidate.id} value={candidate.id}>
                {agentNames[candidate.agentId] ?? candidate.agentId}
                {installations.filter((item) => item.agentId === candidate.agentId).length > 1
                  ? ` — ${candidate.rootPath}`
                  : ''}
              </option>
            ))}
          </select>
        </label>
        {installation && (
          <p className="max-w-full truncate font-mono text-[10px] text-muted-foreground">
            {installation.rootPath}
          </p>
        )}
      </div>
      <div className="h-[min(560px,calc(100vh-15rem))] min-h-[340px]">
        {source && context && installation && sourceFilter ? (
          <AgentCollectionPanel
            key={`${installation.id}:${source.id}:${reloadKey}`}
            context={context}
            scope="user"
            sourceFilter={sourceFilter}
            capabilities={capabilities}
            onBusyChange={onBusyChange}
          />
        ) : (
          <div
            role="status"
            className="flex h-full items-center justify-center text-sm text-muted-foreground"
          >
            {t('resourceCenter.installDialog.unavailable')}
          </div>
        )}
      </div>
    </Dialog>
  );
}

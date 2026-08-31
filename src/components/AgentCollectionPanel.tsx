import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, ChevronDown, Layers3, Search, ShieldAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import type {
  CollectionResourceView,
  ProjectCollectionActionPreview,
  ProjectCollectionSourceInstallPreview,
  ProjectWorkspaceInventory,
  UserCollectionActionPreview,
  UserCollectionSourceInstallPreview,
  UserResourceInventory,
  ResourceAction,
  ResourceActionView,
  ResourceSourceView,
} from '@/lib/agentResourceInventoryTypes';
import type { AgentContext, CapabilityDescriptor } from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';

import { AgentPlanDialog, type AgentPlanProgress } from './AgentPlanDialog';
import { Button } from './ui/button';

interface AgentCollectionPanelProps {
  context: AgentContext;
  scope?: 'project' | 'user';
  sourceFilter?: ResourceSourceView;
  capabilities: CapabilityDescriptor[];
  onOpenHistory?: () => void;
  onBusyChange?: (busy: boolean) => void;
}

export function AgentCollectionPanel({
  context,
  scope = 'project',
  sourceFilter,
  capabilities,
  onOpenHistory,
  onBusyChange,
}: AgentCollectionPanelProps) {
  const { t } = useTranslation();
  const userInstallationId = context.installationId;
  const [inventory, setInventory] = useState<
    ProjectWorkspaceInventory | UserResourceInventory | null
  >(null);
  const [filter, setFilter] = useState('');
  const [expandedSkillSources, setExpandedSkillSources] = useState<Set<string>>(() => new Set());
  const [collapsedSearchSkillSources, setCollapsedSearchSkillSources] = useState<Set<string>>(
    () => new Set(),
  );
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionPreview, setActionPreview] = useState<
    | ProjectCollectionActionPreview
    | ProjectCollectionSourceInstallPreview
    | UserCollectionActionPreview
    | UserCollectionSourceInstallPreview
    | null
  >(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionProgress, setActionProgress] = useState<AgentPlanProgress | null>(null);
  const [pendingApplyOutcome, setPendingApplyOutcome] = useState<'changed' | 'no_change' | null>(
    null,
  );
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionResult, setActionResult] = useState<string | null>(null);
  const loadRequestRef = useRef(0);
  const actionRequestRef = useRef(0);
  const actionBusyRef = useRef(false);
  const contextKey = useMemo(() => JSON.stringify({ context, scope }), [context, scope]);
  const activeContextKeyRef = useRef(contextKey);
  activeContextKeyRef.current = contextKey;

  const load = useCallback(async () => {
    const requestId = ++loadRequestRef.current;
    const requestContextKey = contextKey;
    setLoading(true);
    setError(null);
    try {
      const next =
        scope === 'user'
          ? await tauri.inspectUserAgentResources(userInstallationId)
          : context.projectPath
            ? await tauri.inspectProjectAgentWorkspace(context.installationId, context.projectPath)
            : (() => {
                throw new Error('Project resources require a project context');
              })();
      if (
        requestId !== loadRequestRef.current ||
        requestContextKey !== activeContextKeyRef.current
      ) {
        return;
      }
      setInventory(next);
    } catch (caught) {
      if (
        requestId !== loadRequestRef.current ||
        requestContextKey !== activeContextKeyRef.current
      ) {
        return;
      }
      setError(formatAgentError(caught));
    } finally {
      if (
        requestId === loadRequestRef.current &&
        requestContextKey === activeContextKeyRef.current
      ) {
        setLoading(false);
      }
    }
  }, [context, contextKey, scope, userInstallationId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const reloadWorkspace = () => void load();
    window.addEventListener('ad:agent-workspace-changed', reloadWorkspace);
    return () => window.removeEventListener('ad:agent-workspace-changed', reloadWorkspace);
  }, [load]);

  useEffect(() => {
    actionRequestRef.current += 1;
    actionBusyRef.current = false;
    setActionBusy(false);
    setActionProgress(null);
    setActionPreview(null);
    setActionError(null);
    setActionResult(null);
    setPendingApplyOutcome(null);
    setExpandedSkillSources(new Set());
    setCollapsedSearchSkillSources(new Set());
  }, [contextKey]);

  useEffect(() => {
    onBusyChange?.(actionBusy);
    return () => onBusyChange?.(false);
  }, [actionBusy, onBusyChange]);

  const previewAction = useCallback(
    async (resource: CollectionResourceView, action: ResourceAction) => {
      if (!inventory || (scope === 'project' && !context.projectPath)) return;
      const requestId = ++actionRequestRef.current;
      const requestContextKey = contextKey;
      setActionBusy(true);
      setActionError(null);
      setActionResult(null);
      try {
        const request = {
          workspaceKey: inventory.workspace.key,
          inventoryRevision: inventory.revision,
          resourceKey: resource.key,
          action,
        };
        const next =
          scope === 'user'
            ? await tauri.previewUserCollectionAction(userInstallationId, request)
            : await tauri.previewProjectCollectionAction(
                context.installationId,
                context.projectPath!,
                request,
              );
        if (
          requestId === actionRequestRef.current &&
          requestContextKey === activeContextKeyRef.current
        ) {
          setActionPreview(next);
        }
      } catch (caught) {
        if (
          requestId === actionRequestRef.current &&
          requestContextKey === activeContextKeyRef.current
        ) {
          setActionError(formatAgentError(caught));
        }
      } finally {
        if (
          requestId === actionRequestRef.current &&
          requestContextKey === activeContextKeyRef.current
        ) {
          setActionBusy(false);
        }
      }
    },
    [context.installationId, context.projectPath, contextKey, inventory, userInstallationId, scope],
  );

  const previewSourceInstall = useCallback(
    async (sourceResource: CollectionResourceView) => {
      if (!inventory || (scope === 'project' && !context.projectPath)) return;
      const requestId = ++actionRequestRef.current;
      const requestContextKey = contextKey;
      setActionBusy(true);
      setActionError(null);
      setActionResult(null);
      try {
        const request = {
          workspaceKey: inventory.workspace.key,
          inventoryRevision: inventory.revision,
          sourceResourceKey: sourceResource.key,
        };
        const next =
          scope === 'user'
            ? await tauri.previewUserCollectionSourceInstall(userInstallationId, request)
            : await tauri.previewProjectCollectionSourceInstall(
                context.installationId,
                context.projectPath!,
                request,
              );
        if (
          requestId === actionRequestRef.current &&
          requestContextKey === activeContextKeyRef.current
        ) {
          setActionPreview(next);
        }
      } catch (caught) {
        if (
          requestId === actionRequestRef.current &&
          requestContextKey === activeContextKeyRef.current
        ) {
          setActionError(formatAgentError(caught));
        }
      } finally {
        if (
          requestId === actionRequestRef.current &&
          requestContextKey === activeContextKeyRef.current
        ) {
          setActionBusy(false);
        }
      }
    },
    [context.installationId, context.projectPath, contextKey, inventory, userInstallationId, scope],
  );

  const applyAction = useCallback(async () => {
    if (!actionPreview || actionBusyRef.current) return;
    actionBusyRef.current = true;
    const preview = actionPreview;
    const requestId = actionRequestRef.current;
    const requestContextKey = contextKey;
    let successfulOutcome = pendingApplyOutcome;
    const applyingPhase: AgentPlanProgress['phase'] =
      'resourceKeys' in preview
        ? 'installing'
        : preview.action === 'install'
          ? 'installing'
          : preview.action === 'remove'
            ? 'removing'
            : preview.action === 'update'
              ? 'updating'
              : 'applying';
    setActionBusy(true);
    setActionProgress({
      phase: successfulOutcome ? 'refreshing' : applyingPhase,
      startedAt: Date.now(),
    });
    setActionError(null);
    setActionResult(null);
    try {
      if (!successfulOutcome) {
        const report =
          scope === 'user'
            ? await tauri.applyUserCollectionAction(
                preview.plan.id,
                preview.plan.context,
                preview.plan.riskFingerprint,
              )
            : await tauri.applyProjectCollectionAction(
                preview.plan.id,
                preview.plan.context,
                preview.plan.riskFingerprint,
              );
        if (
          requestId !== actionRequestRef.current ||
          requestContextKey !== activeContextKeyRef.current
        ) {
          return;
        }
        if (report.workspaceKey !== preview.workspaceKey) {
          throw new Error('Workspace operation returned a result for a different workspace');
        }
        if (report.outcome !== 'changed' && report.outcome !== 'no_change') {
          setActionError(
            report.outcome === 'partial_failure'
              ? t('agentCollections.partialFailure')
              : t(`agentCollections.outcome.${report.outcome}`),
          );
          return;
        }
        successfulOutcome = report.outcome;
        setPendingApplyOutcome(successfulOutcome);
        setActionProgress({ phase: 'refreshing', startedAt: Date.now() });
      }

      const next =
        scope === 'user'
          ? await tauri.inspectUserAgentResources(userInstallationId)
          : await tauri.inspectProjectAgentWorkspace(context.installationId, context.projectPath!);
      if (
        requestId !== actionRequestRef.current ||
        requestContextKey !== activeContextKeyRef.current
      ) {
        return;
      }
      setInventory(next);
      setActionPreview(null);
      setPendingApplyOutcome(null);
      setActionResult(
        successfulOutcome === 'changed'
          ? t('agentCollections.applySuccess')
          : t('agentCollections.noChange'),
      );
    } catch (caught) {
      if (
        requestId === actionRequestRef.current &&
        requestContextKey === activeContextKeyRef.current
      ) {
        setActionError(formatAgentError(caught));
        if (scope === 'user') {
          setActionPreview(null);
          setPendingApplyOutcome(null);
          void load();
        }
      }
    } finally {
      if (
        requestId === actionRequestRef.current &&
        requestContextKey === activeContextKeyRef.current
      ) {
        actionBusyRef.current = false;
        setActionBusy(false);
        setActionProgress(null);
      }
    }
  }, [
    actionPreview,
    context.installationId,
    context.projectPath,
    contextKey,
    load,
    pendingApplyOutcome,
    scope,
    t,
    userInstallationId,
  ]);

  const cancelAction = useCallback(() => {
    if (actionBusy) return;
    actionRequestRef.current += 1;
    setActionPreview(null);
    setActionError(null);
    setPendingApplyOutcome(null);
  }, [actionBusy]);

  const query = filter.trim().toLocaleLowerCase();
  const toggleSkillSource = useCallback(
    (sourceKey: string, expanded: boolean) => {
      const update = (current: Set<string>, include: boolean) => {
        const next = new Set(current);
        if (include) next.add(sourceKey);
        else next.delete(sourceKey);
        return next;
      };
      if (query) {
        setCollapsedSearchSkillSources((current) => update(current, expanded));
      } else {
        setExpandedSkillSources((current) => update(current, !expanded));
      }
    },
    [query],
  );
  const sourceSkills = useMemo(
    () =>
      inventory?.skills.resources.filter(
        (resource) =>
          !sourceFilter ||
          sourceIdentity(resource.provenance.source) === sourceIdentity(sourceFilter),
      ) ?? [],
    [inventory, sourceFilter],
  );
  const sourcePlugins = useMemo(
    () =>
      inventory?.plugins.resources.filter(
        (resource) =>
          !sourceFilter ||
          sourceIdentity(resource.provenance.source) === sourceIdentity(sourceFilter),
      ) ?? [],
    [inventory, sourceFilter],
  );
  const filteredSkills = useMemo(
    () => sourceSkills.filter((resource) => matches(resource, query)),
    [query, sourceSkills],
  );
  const filteredPlugins = useMemo(
    () => sourcePlugins.filter((resource) => matches(resource, query)),
    [query, sourcePlugins],
  );
  const hasResources = sourceSkills.length + sourcePlugins.length > 0;
  const limitations = capabilities.flatMap((capability) => capability.limitations);

  if (loading) {
    return (
      <div
        className="flex h-full items-center justify-center text-sm text-muted-foreground"
        aria-busy="true"
      >
        {t('agentCollections.loading')}
      </div>
    );
  }

  if (error && !inventory) {
    return (
      <div role="alert" className="p-6 text-sm text-destructive">
        {error}
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden rounded-lg border border-border bg-card">
      <div
        data-input-shell=""
        className="flex shrink-0 items-center gap-2 rounded-t-lg border-b border-border px-3 py-2"
      >
        <Search className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
        <label htmlFor="agent-resource-filter" className="sr-only">
          {t('agentCollections.filter')}
        </label>
        <input
          id="agent-resource-filter"
          type="search"
          value={filter}
          onChange={(event) => {
            setFilter(event.target.value);
            setCollapsedSearchSkillSources(new Set());
          }}
          placeholder={t('agentCollections.filter')}
          className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
        />
      </div>

      {error && (
        <div
          role="alert"
          className="shrink-0 border-b border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          {error}
        </div>
      )}
      {actionError && !actionPreview && (
        <div
          role="alert"
          className="flex shrink-0 items-center justify-between gap-3 border-b border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          <span>{actionError}</span>
        </div>
      )}
      {actionResult && (
        <div
          role="status"
          aria-live="polite"
          className="flex shrink-0 items-center justify-between gap-3 border-b border-success/40 bg-success/10 px-3 py-2 text-xs text-foreground"
        >
          <span>{actionResult}</span>
          {onOpenHistory && (
            <Button type="button" size="sm" variant="ghost" onClick={onOpenHistory}>
              {t('agentCollections.viewReceipt')}
            </Button>
          )}
        </div>
      )}
      {limitations.length > 0 && (
        <ul className="shrink-0 border-b border-warning/40 bg-warning/10 px-4 py-2 text-xs text-foreground">
          {limitations.map((limitation) => (
            <li key={limitation.code}>{t(limitation.messageKey)}</li>
          ))}
        </ul>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        {inventory && (
          <>
            <CollectionSection
              title={t('agentCollections.skills')}
              inventory={inventory.skills}
              allResources={sourceSkills}
              resources={filteredSkills}
              t={t}
              busy={actionBusy}
              queryActive={query.length > 0}
              showEmptyState={hasResources}
              onAction={previewAction}
              onInstallSource={previewSourceInstall}
              expandedSourceKeys={expandedSkillSources}
              collapsedSearchSourceKeys={collapsedSearchSkillSources}
              onToggleSource={toggleSkillSource}
              onReload={load}
            />
            <CollectionSection
              title={t('agentCollections.plugins')}
              inventory={inventory.plugins}
              allResources={sourcePlugins}
              resources={filteredPlugins}
              t={t}
              busy={actionBusy}
              queryActive={query.length > 0}
              showEmptyState={hasResources}
              onAction={previewAction}
              onReload={load}
            />
          </>
        )}
        {inventory && !hasResources && (
          <div role="status" className="py-12 text-center text-sm text-muted-foreground">
            {t('agentCollections.workspaceEmpty')}
          </div>
        )}
        {hasResources && filteredSkills.length === 0 && filteredPlugins.length === 0 && (
          <div role="status" className="py-12 text-center text-sm text-muted-foreground">
            {t('agentCollections.noMatches')}
          </div>
        )}
      </div>
      <AgentPlanDialog
        plan={actionPreview?.plan ?? null}
        busy={actionBusy}
        error={actionPreview ? actionError : null}
        progress={actionProgress}
        onCancel={cancelAction}
        onConfirm={() => void applyAction()}
      />
    </div>
  );
}

interface CollectionSectionProps {
  title: string;
  inventory: ProjectWorkspaceInventory['skills'] | UserResourceInventory['skills'];
  allResources?: CollectionResourceView[];
  resources: CollectionResourceView[];
  t: ReturnType<typeof useTranslation>['t'];
  busy: boolean;
  queryActive: boolean;
  showEmptyState: boolean;
  onAction: (resource: CollectionResourceView, action: ResourceAction) => void;
  onInstallSource?: (sourceResource: CollectionResourceView) => void;
  expandedSourceKeys?: ReadonlySet<string>;
  collapsedSearchSourceKeys?: ReadonlySet<string>;
  onToggleSource?: (sourceKey: string, expanded: boolean) => void;
  onOpenSkillSources?: () => Promise<void>;
  onReload: () => Promise<void>;
}

function CollectionSection({
  title,
  inventory,
  allResources = inventory.resources,
  resources,
  t,
  busy,
  queryActive,
  showEmptyState,
  onAction,
  onInstallSource,
  expandedSourceKeys,
  collapsedSearchSourceKeys,
  onToggleSource,
  onOpenSkillSources,
  onReload,
}: CollectionSectionProps) {
  const conflicts = groupedConflicts(allResources, resources);
  return (
    <section className="mb-5" aria-labelledby={`collection-${inventory.kind}`}>
      <div className="mb-2 flex items-center justify-between gap-3">
        <h3
          id={`collection-${inventory.kind}`}
          className="text-xs font-semibold uppercase tracking-wide text-muted-foreground"
        >
          {title}
        </h3>
        <span className="text-[11px] text-muted-foreground">
          {t('agentCollections.coverageCount', {
            visible: inventory.coverage.visible,
            observed: inventory.coverage.observed,
          })}
        </span>
      </div>
      {inventory.coverage.status !== 'complete' && (
        <div
          className="mb-2 flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-foreground"
          role="status"
        >
          <ShieldAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          {t(`agentCollections.coverage.${inventory.coverage.status}`)}
        </div>
      )}
      {conflicts.map((conflict) => (
        <ConflictGuidance
          key={conflict.logicalId}
          kind={inventory.kind}
          logicalId={conflict.logicalId}
          resources={conflict.resources}
          busy={busy}
          t={t}
          onOpenSkillSources={onOpenSkillSources}
          onReload={onReload}
        />
      ))}
      {resources.length > 0 && inventory.kind === 'skills' && onInstallSource ? (
        <SkillSourceGroups
          resources={resources}
          allResources={allResources}
          busy={busy}
          t={t}
          onAction={onAction}
          onInstallSource={onInstallSource}
          queryActive={queryActive}
          expandedSourceKeys={expandedSourceKeys ?? new Set()}
          collapsedSearchSourceKeys={collapsedSearchSourceKeys ?? new Set()}
          onToggleSource={onToggleSource ?? (() => {})}
        />
      ) : resources.length > 0 ? (
        <ResourceList resources={resources} busy={busy} t={t} onAction={onAction} showSource />
      ) : null}
      {showEmptyState && resources.length === 0 && !queryActive && allResources.length === 0 && (
        <div className="rounded-md border border-dashed border-border px-3 py-4 text-xs text-muted-foreground">
          {t('agentCollections.categoryEmpty', { category: title })}
        </div>
      )}
    </section>
  );
}

interface ResourceListProps {
  resources: CollectionResourceView[];
  busy: boolean;
  t: ReturnType<typeof useTranslation>['t'];
  onAction: (resource: CollectionResourceView, action: ResourceAction) => void;
  showSource: boolean;
  embedded?: boolean;
}

function ResourceList({
  resources,
  busy,
  t,
  onAction,
  showSource,
  embedded = false,
}: ResourceListProps) {
  return (
    <ul
      className={
        embedded
          ? 'divide-y divide-border'
          : 'divide-y divide-border overflow-hidden rounded-lg border border-border'
      }
    >
      {resources.map((resource) => (
        <li key={resource.key} className="flex items-start gap-3 px-3 py-2.5">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="truncate text-sm font-medium">{resource.displayName}</span>
              <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">
                {t(resourceStateMessageKey(resource))}
              </span>
              {resource.health.status !== 'healthy' && (
                <AlertTriangle
                  className="h-3.5 w-3.5 text-warning"
                  aria-label={t(`agentCollections.health.${resource.health.status}`)}
                />
              )}
            </div>
            {resource.description && (
              <div className="mt-0.5 truncate text-xs text-muted-foreground">
                {resource.description}
              </div>
            )}
            {showSource && resource.provenance.source && (
              <div
                className="mt-1 flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground"
                title={resource.provenance.source.location}
              >
                <span className="truncate">{resource.provenance.source.displayName}</span>
                <span className="shrink-0 rounded-full bg-muted px-1.5 py-0.5 text-[10px]">
                  {t(
                    `agentCollections.conflictResolution.sourceKind.${resource.provenance.source.kind}`,
                  )}
                </span>
              </div>
            )}
            <div className="mt-1 flex items-center gap-1 text-[11px] text-muted-foreground">
              <Layers3 className="h-3 w-3" aria-hidden="true" />
              <span>{resourceEvidenceLabel(resource, t)}</span>
              <span>· {t(`agentCollections.management.${resource.management.status}`)}</span>
            </div>
            <ResourceActions resource={resource} busy={busy} t={t} onAction={onAction} />
          </div>
        </li>
      ))}
    </ul>
  );
}

interface SkillSourceGroup {
  key: string;
  source?: ResourceSourceView;
  resources: CollectionResourceView[];
}

function SkillSourceGroups({
  resources,
  allResources,
  busy,
  t,
  onAction,
  onInstallSource,
  queryActive,
  expandedSourceKeys,
  collapsedSearchSourceKeys,
  onToggleSource,
}: Omit<ResourceListProps, 'showSource'> & {
  allResources: CollectionResourceView[];
  onInstallSource: (sourceResource: CollectionResourceView) => void;
  queryActive: boolean;
  expandedSourceKeys: ReadonlySet<string>;
  collapsedSearchSourceKeys: ReadonlySet<string>;
  onToggleSource: (sourceKey: string, expanded: boolean) => void;
}) {
  return (
    <div className="space-y-3">
      {groupSkillsBySource(resources).map((group) => {
        const allGroupResources = allResources.filter(
          (resource) => sourceIdentity(resource.provenance.source) === group.key,
        );
        const sourceResource = allGroupResources[0];
        const installable = allGroupResources.filter((resource) =>
          actionIsAvailable(resource, 'install'),
        );
        const canBatchInstall =
          sourceResource &&
          group.source &&
          ['catalog_git', 'catalog_local'].includes(group.source.kind) &&
          installable.length > 1;
        if (allGroupResources.length === 1) {
          return (
            <MergedSkillSourceCard
              key={group.key}
              resource={group.resources[0]!}
              source={group.source}
              busy={busy}
              t={t}
              onAction={onAction}
            />
          );
        }
        const expanded = queryActive
          ? !collapsedSearchSourceKeys.has(group.key)
          : expandedSourceKeys.has(group.key);
        const resourceListId = `skill-source-${sourceResource?.key.replace(/[^a-zA-Z0-9_-]/g, '-') ?? 'unknown'}`;
        return (
          <section
            key={group.key}
            className="overflow-hidden rounded-xl border border-border bg-card"
          >
            <div
              className={`flex flex-wrap items-start justify-between gap-3 bg-muted/20 px-4 py-3 ${
                expanded ? 'border-b border-border' : ''
              }`}
            >
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <h4 className="min-w-0 text-sm font-semibold">
                    <button
                      type="button"
                      className="flex min-w-0 items-center gap-1.5 rounded text-left hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      aria-expanded={expanded}
                      aria-controls={resourceListId}
                      onClick={() => onToggleSource(group.key, expanded)}
                    >
                      <ChevronDown
                        className={`h-3.5 w-3.5 shrink-0 transition-transform motion-reduce:transition-none ${
                          expanded ? 'rotate-180' : ''
                        }`}
                        aria-hidden="true"
                      />
                      <span className="truncate">
                        {group.source?.displayName ?? t('agentCollections.sourceGroup.unknown')}
                      </span>
                    </button>
                  </h4>
                  {group.source && (
                    <span className="rounded-full bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                      {t(`agentCollections.conflictResolution.sourceKind.${group.source.kind}`)}
                    </span>
                  )}
                  <span className="text-[11px] text-muted-foreground">
                    {t('agentCollections.sourceGroup.skillsCount', {
                      count: allGroupResources.length,
                    })}
                  </span>
                </div>
                {group.source && <SourceDetails source={group.source} t={t} />}
              </div>
              {canBatchInstall && (
                <Button
                  type="button"
                  size="sm"
                  variant="secondary"
                  className="h-7 shrink-0 whitespace-nowrap px-2 text-xs"
                  disabled={busy}
                  onClick={() => onInstallSource(sourceResource)}
                >
                  {t('agentCollections.sourceGroup.installAll', { count: installable.length })}
                </Button>
              )}
            </div>
            {expanded && (
              <div id={resourceListId} className="ml-4 border-l border-border/70 bg-background/20">
                <ResourceList
                  resources={group.resources}
                  busy={busy}
                  t={t}
                  onAction={onAction}
                  showSource={false}
                  embedded
                />
              </div>
            )}
          </section>
        );
      })}
    </div>
  );
}

function MergedSkillSourceCard({
  resource,
  source,
  busy,
  t,
  onAction,
}: {
  resource: CollectionResourceView;
  source?: ResourceSourceView;
  busy: boolean;
  t: ReturnType<typeof useTranslation>['t'];
  onAction: (resource: CollectionResourceView, action: ResourceAction) => void;
}) {
  const distinctSourceName =
    source &&
    source.displayName.trim().toLocaleLowerCase() !==
      resource.displayName.trim().toLocaleLowerCase()
      ? source.displayName
      : null;
  return (
    <section className="rounded-xl border border-border bg-card px-4 py-3">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h4 className="truncate text-sm font-semibold">{resource.displayName}</h4>
            <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">
              {t(resourceStateMessageKey(resource))}
            </span>
            {source && (
              <span className="rounded-full bg-muted/70 px-2 py-0.5 text-[10px] text-muted-foreground">
                {t(`agentCollections.conflictResolution.sourceKind.${source.kind}`)}
              </span>
            )}
            {resource.health.status !== 'healthy' && (
              <AlertTriangle
                className="h-3.5 w-3.5 text-warning"
                aria-label={t(`agentCollections.health.${resource.health.status}`)}
              />
            )}
          </div>
          {distinctSourceName && (
            <p className="mt-1 text-xs text-muted-foreground">
              {t('agentCollections.sourceGroup.fromSource', { source: distinctSourceName })}
            </p>
          )}
          {resource.description && (
            <p className="mt-1 text-xs text-muted-foreground">{resource.description}</p>
          )}
          <div className="mt-2 flex flex-wrap items-center gap-x-1 gap-y-1 text-[11px] text-muted-foreground">
            <Layers3 className="h-3 w-3" aria-hidden="true" />
            <span>{resourceEvidenceLabel(resource, t)}</span>
            <span aria-hidden="true">·</span>
            <span>{t(`agentCollections.management.${resource.management.status}`)}</span>
          </div>
        </div>
        <ResourceActions
          resource={resource}
          busy={busy}
          t={t}
          onAction={onAction}
          className="shrink-0"
        />
      </div>
      {source && <SourceDetails source={source} t={t} />}
    </section>
  );
}

function SourceDetails({
  source,
  t,
}: {
  source: ResourceSourceView;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  return (
    <details className="group/details mt-2 text-[11px] text-muted-foreground">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1 rounded py-0.5 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <ChevronDown
          className="h-3 w-3 transition-transform group-open/details:rotate-180 motion-reduce:transition-none"
          aria-hidden="true"
        />
        {t('agentCollections.sourceGroup.details')}
      </summary>
      <div className="mt-2 rounded-md border border-border/70 bg-muted/20 px-3 py-2">
        <div
          className="break-all font-mono leading-relaxed text-foreground"
          title={source.location}
        >
          {source.location}
        </div>
        {(source.branch || source.subdirectory) && (
          <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-1">
            {source.branch && (
              <span>
                {t('agentCollections.conflictResolution.branch', { branch: source.branch })}
              </span>
            )}
            {source.subdirectory && (
              <span className="break-all">
                {t('agentCollections.conflictResolution.subdirectory', {
                  subdirectory: source.subdirectory,
                })}
              </span>
            )}
          </div>
        )}
      </div>
    </details>
  );
}

function groupSkillsBySource(resources: CollectionResourceView[]): SkillSourceGroup[] {
  const groups = new Map<string, SkillSourceGroup>();
  for (const resource of resources) {
    const source = resource.provenance.source;
    const key = sourceIdentity(source);
    const group = groups.get(key) ?? { key, source, resources: [] };
    group.resources.push(resource);
    groups.set(key, group);
  }
  return Array.from(groups.values());
}

function sourceIdentity(source: ResourceSourceView | undefined): string {
  return source ? JSON.stringify(source) : 'unknown';
}

interface ConflictGuidanceProps {
  kind: CollectionResourceView['kind'];
  logicalId: string;
  resources: CollectionResourceView[];
  busy: boolean;
  t: ReturnType<typeof useTranslation>['t'];
  onOpenSkillSources?: () => Promise<void>;
  onReload: () => Promise<void>;
}

function ConflictGuidance({
  kind,
  logicalId,
  resources,
  busy,
  t,
  onOpenSkillSources,
  onReload,
}: ConflictGuidanceProps) {
  const candidates = conflictCandidates(kind, resources, t);
  return (
    <div
      role="alert"
      className="mb-2 flex items-start gap-2 rounded-md border border-warning/50 bg-warning/10 px-3 py-2.5 text-xs text-foreground"
    >
      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-warning" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <div className="font-medium">
          {t(`agentCollections.conflictResolution.${kind}.title`, {
            name: logicalId,
            count: candidates.length,
          })}
        </div>
        <p className="mt-1 text-muted-foreground">
          {t(`agentCollections.conflictResolution.${kind}.description`)}
        </p>
        <ul
          className="mt-2 space-y-1.5"
          aria-label={t('agentCollections.conflictResolution.candidates')}
        >
          {candidates.map((candidate, index) => (
            <li
              key={candidate.key}
              className="flex min-w-0 items-start gap-2 rounded border border-border bg-background/70 px-2 py-2 text-[11px]"
            >
              <span className="text-muted-foreground">{index + 1}.</span>
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <span className="break-words font-medium">{candidate.displayName}</span>
                  {candidate.source && (
                    <span className="whitespace-nowrap rounded-full bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                      {t(`agentCollections.conflictResolution.sourceKind.${candidate.source.kind}`)}
                    </span>
                  )}
                </div>
                {candidate.source && (
                  <>
                    <div
                      aria-label={`${t('agentCollections.conflictResolution.sourceAddress')}: ${candidate.source.location}`}
                      className="mt-1 break-all font-mono leading-relaxed text-foreground [overflow-wrap:anywhere]"
                    >
                      {candidate.source.location}
                    </div>
                    {(candidate.source.branch || candidate.source.subdirectory) && (
                      <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-muted-foreground">
                        {candidate.source.branch && (
                          <span>
                            {t('agentCollections.conflictResolution.branch', {
                              branch: candidate.source.branch,
                            })}
                          </span>
                        )}
                        {candidate.source.subdirectory && (
                          <span className="break-all">
                            {t('agentCollections.conflictResolution.subdirectory', {
                              subdirectory: candidate.source.subdirectory,
                            })}
                          </span>
                        )}
                      </div>
                    )}
                  </>
                )}
                {candidate.sourceUnavailable && (
                  <div className="mt-1 text-muted-foreground">
                    {t('agentCollections.conflictResolution.sourceAddressUnavailable')}
                  </div>
                )}
              </div>
            </li>
          ))}
        </ul>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {onOpenSkillSources && (
            <Button
              type="button"
              size="sm"
              variant="secondary"
              className="h-7 whitespace-nowrap px-2 text-xs"
              disabled={busy}
              onClick={() => void onOpenSkillSources()}
            >
              {t('agentCollections.conflictResolution.manageSkillSources')}
            </Button>
          )}
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-7 whitespace-nowrap px-2 text-xs"
            disabled={busy}
            onClick={() => void onReload()}
          >
            {t('agentCollections.conflictResolution.checkAgain')}
          </Button>
        </div>
      </div>
    </div>
  );
}

function groupedConflicts(
  resources: CollectionResourceView[],
  visibleResources: CollectionResourceView[],
): Array<{ logicalId: string; resources: CollectionResourceView[] }> {
  const visibleKeys = new Set(visibleResources.map((resource) => resource.key));
  const groups = new Map<string, CollectionResourceView[]>();
  for (const resource of resources) {
    if (resource.effectiveState !== 'conflict') continue;
    const group = groups.get(resource.logicalId) ?? [];
    group.push(resource);
    groups.set(resource.logicalId, group);
  }
  return Array.from(groups, ([logicalId, conflictingResources]) => ({
    logicalId,
    resources: conflictingResources,
  })).filter((group) => group.resources.some((resource) => visibleKeys.has(resource.key)));
}

interface ConflictCandidate {
  key: string;
  displayName: string;
  source?: NonNullable<CollectionResourceView['provenance']['source']>;
  sourceUnavailable?: boolean;
}

function conflictCandidates(
  kind: CollectionResourceView['kind'],
  resources: CollectionResourceView[],
  t: ReturnType<typeof useTranslation>['t'],
): ConflictCandidate[] {
  const orderedResources = [...resources].sort((left, right) => left.key.localeCompare(right.key));
  if (kind === 'skills') {
    return orderedResources.map((resource, index) => ({
      key: resource.key,
      displayName:
        resource.provenance.source?.displayName ??
        resource.description ??
        t('agentCollections.conflictResolution.unnamedSource', { index: index + 1 }),
      source: resource.provenance.source,
      sourceUnavailable: !resource.provenance.source,
    }));
  }

  const declarations = orderedResources
    .flatMap((resource) => resource.provenance.declarations)
    .sort((left, right) => left.key.localeCompare(right.key));
  if (declarations.length > 0) {
    return declarations.map((declaration) => ({
      key: declaration.key,
      displayName: t('agentCollections.conflictResolution.layerDeclaration', {
        layer: t(`agentSettings.layer.${declaration.layer}`),
      }),
    }));
  }
  return orderedResources.map((resource, index) => ({
    key: resource.key,
    displayName:
      resource.description ??
      t('agentCollections.conflictResolution.unnamedSource', { index: index + 1 }),
  }));
}

interface ResourceActionsProps {
  resource: CollectionResourceView;
  busy: boolean;
  t: ReturnType<typeof useTranslation>['t'];
  onAction: (resource: CollectionResourceView, action: ResourceAction) => void;
  className?: string;
}

function ResourceActions({
  resource,
  busy,
  t,
  onAction,
  className = 'mt-2',
}: ResourceActionsProps) {
  const actions = resource.management.actions.filter(isMutationAction);
  if (actions.length === 0) return null;
  return (
    <div className={`flex flex-wrap items-center gap-1.5 ${className}`}>
      {actions.map((action) => {
        const available = ['available', 'confirmation_required'].includes(action.availability);
        const label = actionLabel(resource, action, t);
        return (
          <Button
            key={action.action}
            type="button"
            size="sm"
            variant={action.action === 'remove' ? 'outline' : 'secondary'}
            className="h-7 whitespace-nowrap px-2 text-xs"
            disabled={busy || !available}
            title={action.limitation ? t(action.limitation.messageKey) : undefined}
            aria-label={`${label}: ${resource.displayName}`}
            onClick={() => onAction(resource, action.action)}
          >
            {label}
          </Button>
        );
      })}
    </div>
  );
}

function isMutationAction(action: ResourceActionView): boolean {
  return ['install', 'update', 'remove', 'enable', 'disable'].includes(action.action);
}

function actionIsAvailable(resource: CollectionResourceView, action: ResourceAction): boolean {
  return resource.management.actions.some(
    (candidate) =>
      candidate.action === action &&
      ['available', 'confirmation_required'].includes(candidate.availability),
  );
}

function actionLabel(
  resource: CollectionResourceView,
  action: ResourceActionView,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  if (action.intent === 'relink') {
    return t('agentCollections.action.relink');
  }
  if (action.intent === 'repair') {
    return t('agentCollections.action.repair');
  }
  if (resource.kind === 'plugins' && action.action === 'remove') {
    return t('agentCollections.action.resetPluginOverride');
  }
  return t(`agentCollections.action.${action.action}`);
}

function isCatalogCandidate(resource: CollectionResourceView): boolean {
  return (
    resource.provenance.declarations.length === 0 &&
    ['catalog_git', 'catalog_local'].includes(resource.provenance.source?.kind ?? '')
  );
}

function resourceStateMessageKey(resource: CollectionResourceView): string {
  if (isCatalogCandidate(resource) && actionIsAvailable(resource, 'install')) {
    return 'agentCollections.state.available';
  }
  return `agentCollections.state.${resource.effectiveState}`;
}

function resourceEvidenceLabel(
  resource: CollectionResourceView,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  if (isCatalogCandidate(resource)) {
    return t('agentCollections.catalogCandidate');
  }
  return t('agentCollections.declarations', {
    count: resource.provenance.declarations.length,
  });
}

function matches(resource: CollectionResourceView, query: string): boolean {
  if (!query) return true;
  return [
    resource.displayName,
    resource.logicalId,
    resource.description ?? '',
    resource.provenance.source?.displayName ?? '',
    resource.provenance.source?.location ?? '',
  ].some((value) => value.toLocaleLowerCase().includes(query));
}

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, Layers3, Search, ShieldAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import type {
  CollectionResourceView,
  ProjectCollectionActionPreview,
  ProjectWorkspaceInventory,
  ResourceAction,
  ResourceActionView,
} from '@/lib/agentResourceInventoryTypes';
import type { AgentContext, CapabilityDescriptor } from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';
import { runDetachedWorkspaceOperation, useWorkspaceOperations } from '@/store/workspaceOperations';

import { AgentPlanDialog } from './AgentPlanDialog';
import { Button } from './ui/button';

interface AgentCollectionPanelProps {
  context: AgentContext;
  capabilities: CapabilityDescriptor[];
  onOpenHistory?: () => void;
}

export function AgentCollectionPanel({
  context,
  capabilities,
  onOpenHistory,
}: AgentCollectionPanelProps) {
  const { t } = useTranslation();
  const [inventory, setInventory] = useState<ProjectWorkspaceInventory | null>(null);
  const [filter, setFilter] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionPreview, setActionPreview] = useState<ProjectCollectionActionPreview | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionResult, setActionResult] = useState<string | null>(null);
  const loadRequestRef = useRef(0);
  const actionRequestRef = useRef(0);
  const handledOperationRef = useRef<string | null>(null);
  const contextKey = useMemo(() => JSON.stringify(context), [context]);
  const activeContextKeyRef = useRef(contextKey);
  activeContextKeyRef.current = contextKey;

  const load = useCallback(async () => {
    const requestId = ++loadRequestRef.current;
    const requestContextKey = JSON.stringify(context);
    setLoading(true);
    setError(null);
    try {
      if (!context.projectPath) throw new Error('Project resources require a project context');
      const next = await tauri.inspectProjectAgentWorkspace(
        context.installationId,
        context.projectPath,
      );
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
  }, [context]);

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
    setActionPreview(null);
    setActionError(null);
    setActionResult(null);
  }, [contextKey]);

  const previewAction = useCallback(
    async (resource: CollectionResourceView, action: ResourceAction) => {
      if (!inventory || !context.projectPath) return;
      const requestId = ++actionRequestRef.current;
      const requestContextKey = contextKey;
      setActionBusy(true);
      setActionError(null);
      setActionResult(null);
      try {
        const next = await tauri.previewProjectCollectionAction(
          context.installationId,
          context.projectPath,
          {
            workspaceKey: inventory.workspace.key,
            inventoryRevision: inventory.revision,
            resourceKey: resource.key,
            action,
          },
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
    [context.installationId, context.projectPath, contextKey, inventory],
  );

  const applyAction = useCallback(() => {
    if (!actionPreview) return;
    const preview = actionPreview;
    setActionPreview(null);
    setActionError(null);
    setActionResult(null);
    void runDetachedWorkspaceOperation(preview.workspaceKey, preview.plan.id, () =>
      tauri.applyProjectCollectionAction(
        preview.plan.id,
        preview.plan.context,
        preview.plan.riskFingerprint,
      ),
    ).catch(() => undefined);
  }, [actionPreview]);

  const cancelAction = useCallback(() => {
    if (actionBusy) return;
    actionRequestRef.current += 1;
    setActionPreview(null);
    setActionError(null);
  }, [actionBusy]);

  const openSkillSources = useCallback(async () => {
    setActionError(null);
    try {
      await tauri.openSettingsWindow();
    } catch (caught) {
      setActionError(formatAgentError(caught));
    }
  }, []);

  const query = filter.trim().toLocaleLowerCase();
  const filteredSkills = useMemo(
    () => inventory?.skills.resources.filter((resource) => matches(resource, query)) ?? [],
    [inventory, query],
  );
  const filteredPlugins = useMemo(
    () => inventory?.plugins.resources.filter((resource) => matches(resource, query)) ?? [],
    [inventory, query],
  );
  const workspaceKey = inventory?.workspace.key;
  const trackedOperation = useWorkspaceOperations((state) =>
    workspaceKey ? state.operations[workspaceKey] : undefined,
  );
  const operationBusy = trackedOperation?.status === 'applying';
  const hasResources = Boolean(
    inventory && inventory.skills.resources.length + inventory.plugins.resources.length > 0,
  );
  const limitations = capabilities.flatMap((capability) => capability.limitations);

  useEffect(() => {
    if (!trackedOperation) return;
    if (trackedOperation.status === 'applying') {
      setActionError(null);
      setActionResult(null);
      return;
    }
    const completionKey = `${trackedOperation.operationId}:${trackedOperation.finishedAt ?? ''}`;
    if (handledOperationRef.current === completionKey) return;
    handledOperationRef.current = completionKey;
    if (trackedOperation.status === 'failed') {
      setActionError(trackedOperation.error ?? t('agentCollections.applyFailed'));
    } else if (trackedOperation.status === 'partial_failure') {
      setActionError(t('agentCollections.partialFailure'));
    } else {
      const outcome = trackedOperation.report?.outcome;
      if (outcome === 'changed') setActionResult(t('agentCollections.applySuccess'));
      else if (outcome === 'no_change') setActionResult(t('agentCollections.noChange'));
      else if (outcome) setActionError(t(`agentCollections.outcome.${outcome}`));
      else setActionError(t('agentCollections.applyFailed'));
    }
  }, [t, trackedOperation]);

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
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <Search className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
        <label htmlFor="agent-resource-filter" className="sr-only">
          {t('agentCollections.filter')}
        </label>
        <input
          id="agent-resource-filter"
          type="search"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
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
          {trackedOperation?.report?.receipt && onOpenHistory && (
            <Button type="button" size="sm" variant="ghost" onClick={onOpenHistory}>
              {t('agentCollections.viewReceipt')}
            </Button>
          )}
        </div>
      )}
      {operationBusy && (
        <div
          role="status"
          aria-live="polite"
          className="flex shrink-0 items-center justify-between gap-3 border-b border-primary/30 bg-primary/5 px-3 py-2 text-xs text-foreground"
        >
          <span>{t('agentCollections.applyDetached')}</span>
          {onOpenHistory && (
            <Button type="button" size="sm" variant="ghost" onClick={onOpenHistory}>
              {t('agentCollections.viewHistory')}
            </Button>
          )}
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
              resources={filteredSkills}
              t={t}
              busy={actionBusy || operationBusy}
              queryActive={query.length > 0}
              showEmptyState={hasResources}
              onAction={previewAction}
              onOpenSkillSources={openSkillSources}
              onReload={load}
            />
            <CollectionSection
              title={t('agentCollections.plugins')}
              inventory={inventory.plugins}
              resources={filteredPlugins}
              t={t}
              busy={actionBusy || operationBusy}
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
        onCancel={cancelAction}
        onConfirm={() => void applyAction()}
      />
    </div>
  );
}

interface CollectionSectionProps {
  title: string;
  inventory: ProjectWorkspaceInventory['skills'];
  resources: CollectionResourceView[];
  t: ReturnType<typeof useTranslation>['t'];
  busy: boolean;
  queryActive: boolean;
  showEmptyState: boolean;
  onAction: (resource: CollectionResourceView, action: ResourceAction) => void;
  onOpenSkillSources?: () => Promise<void>;
  onReload: () => Promise<void>;
}

function CollectionSection({
  title,
  inventory,
  resources,
  t,
  busy,
  queryActive,
  showEmptyState,
  onAction,
  onOpenSkillSources,
  onReload,
}: CollectionSectionProps) {
  const conflicts = groupedConflicts(inventory.resources, resources);
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
      {resources.length > 0 && (
        <ul className="divide-y divide-border overflow-hidden rounded-lg border border-border">
          {resources.map((resource) => (
            <li key={resource.key} className="flex items-start gap-3 px-3 py-2.5">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="truncate text-sm font-medium">{resource.displayName}</span>
                  <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">
                    {t(`agentCollections.state.${resource.effectiveState}`)}
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
                <div className="mt-1 flex items-center gap-1 text-[11px] text-muted-foreground">
                  <Layers3 className="h-3 w-3" aria-hidden="true" />
                  <span>
                    {t('agentCollections.declarations', {
                      count: resource.provenance.declarations.length,
                    })}
                  </span>
                  <span>· {t(`agentCollections.management.${resource.management.status}`)}</span>
                </div>
                <ResourceActions resource={resource} busy={busy} t={t} onAction={onAction} />
              </div>
            </li>
          ))}
        </ul>
      )}
      {showEmptyState &&
        resources.length === 0 &&
        !queryActive &&
        inventory.resources.length === 0 && (
          <div className="rounded-md border border-dashed border-border px-3 py-4 text-xs text-muted-foreground">
            {t('agentCollections.categoryEmpty', { category: title })}
          </div>
        )}
    </section>
  );
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
}

function ResourceActions({ resource, busy, t, onAction }: ResourceActionsProps) {
  const actions = resource.management.actions.filter(isMutationAction);
  if (actions.length === 0) return null;
  return (
    <div className="mt-2 flex flex-wrap items-center gap-1.5">
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

function matches(resource: CollectionResourceView, query: string): boolean {
  if (!query) return true;
  return [resource.displayName, resource.logicalId, resource.description ?? ''].some((value) =>
    value.toLocaleLowerCase().includes(query),
  );
}

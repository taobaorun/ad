import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, Layers3, Search, ShieldAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import type {
  CollectionResourceView,
  ProjectWorkspaceInventory,
} from '@/lib/agentResourceInventoryTypes';
import type { AgentContext, CapabilityDescriptor } from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';

interface AgentCollectionPanelProps {
  context: AgentContext;
  capabilities: CapabilityDescriptor[];
}

export function AgentCollectionPanel({ context, capabilities }: AgentCollectionPanelProps) {
  const { t } = useTranslation();
  const [inventory, setInventory] = useState<ProjectWorkspaceInventory | null>(null);
  const [filter, setFilter] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const loadRequestRef = useRef(0);
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

  const query = filter.trim().toLocaleLowerCase();
  const filteredSkills = useMemo(
    () => inventory?.skills.resources.filter((resource) => matches(resource, query)) ?? [],
    [inventory, query],
  );
  const filteredPlugins = useMemo(
    () => inventory?.plugins.resources.filter((resource) => matches(resource, query)) ?? [],
    [inventory, query],
  );
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
            />
            <CollectionSection
              title={t('agentCollections.plugins')}
              inventory={inventory.plugins}
              resources={filteredPlugins}
              t={t}
            />
          </>
        )}
        {filteredSkills.length === 0 && filteredPlugins.length === 0 && (
          <div role="status" className="py-12 text-center text-sm text-muted-foreground">
            {t('agentCollections.empty')}
          </div>
        )}
      </div>
    </div>
  );
}

interface CollectionSectionProps {
  title: string;
  inventory: ProjectWorkspaceInventory['skills'];
  resources: CollectionResourceView[];
  t: ReturnType<typeof useTranslation>['t'];
}

function CollectionSection({ title, inventory, resources, t }: CollectionSectionProps) {
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
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function matches(resource: CollectionResourceView, query: string): boolean {
  if (!query) return true;
  return [resource.displayName, resource.logicalId, resource.description ?? ''].some((value) =>
    value.toLocaleLowerCase().includes(query),
  );
}

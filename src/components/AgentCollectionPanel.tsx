import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { RotateCcw, Search } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { collectionItemView } from '@/lib/agentResourceViews';
import { formatAgentError } from '@/lib/agentErrors';
import { capabilityAllows, capabilityFor } from '@/lib/agentCapabilities';
import type {
  AgentContext,
  CapabilityDescriptor,
  MutationPlanView,
  OperationReceipt,
  ResourceSnapshot,
} from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';

import { AgentPlanDialog } from './AgentPlanDialog';
import { Toggle } from './SkillToggle';
import { Button } from './ui/button';

interface AgentCollectionPanelProps {
  context: AgentContext;
  capabilities: CapabilityDescriptor[];
}

function notifyAgentWorkspaceChanged() {
  window.dispatchEvent(new Event('ad:project-codex-runtime-changed'));
  window.dispatchEvent(new Event('ad:agent-workspace-changed'));
}

export function AgentCollectionPanel({ context, capabilities }: AgentCollectionPanelProps) {
  const { t } = useTranslation();
  const [skills, setSkills] = useState<ResourceSnapshot[]>([]);
  const [plugins, setPlugins] = useState<ResourceSnapshot[]>([]);
  const [filter, setFilter] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [plan, setPlan] = useState<MutationPlanView | null>(null);
  const [planBusy, setPlanBusy] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  const [lastReceipt, setLastReceipt] = useState<OperationReceipt | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const loadRequestRef = useRef(0);
  const previewRequestRef = useRef(0);
  const contextKey = useMemo(() => JSON.stringify(context), [context]);
  const activeContextKeyRef = useRef(contextKey);
  activeContextKeyRef.current = contextKey;

  const skillsCapability = capabilityFor(capabilities, 'skills');
  const pluginsCapability = capabilityFor(capabilities, 'plugins');
  const canListSkills = capabilityAllows(capabilities, 'skills', 'list');
  const canListPlugins = capabilityAllows(capabilities, 'plugins', 'list');

  const load = useCallback(async () => {
    const requestId = ++loadRequestRef.current;
    const requestContextKey = JSON.stringify(context);
    setLoading(true);
    setError(null);
    try {
      const [skillsResult, pluginsResult] = await Promise.allSettled([
        canListSkills ? tauri.listAgentSkills(context) : Promise.resolve([]),
        canListPlugins ? tauri.listAgentPlugins(context) : Promise.resolve([]),
      ]);
      if (
        requestId !== loadRequestRef.current ||
        requestContextKey !== activeContextKeyRef.current
      ) {
        return;
      }
      const failures: string[] = [];
      if (skillsResult.status === 'fulfilled') {
        setSkills(skillsResult.value);
      } else {
        failures.push(formatAgentError(skillsResult.reason));
      }
      if (pluginsResult.status === 'fulfilled') {
        setPlugins(pluginsResult.value);
      } else {
        failures.push(formatAgentError(pluginsResult.reason));
      }
      setError(failures.length > 0 ? failures.join(' · ') : null);
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
  }, [canListPlugins, canListSkills, context]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    previewRequestRef.current += 1;
    setPlan(null);
    setPlanError(null);
    setLastReceipt(null);
    setStatus(null);
  }, [contextKey]);

  const query = filter.trim().toLocaleLowerCase();
  const filteredSkills = useMemo(
    () => skills.filter((snapshot) => matches(snapshot, query)),
    [query, skills],
  );
  const filteredPlugins = useMemo(
    () => plugins.filter((snapshot) => matches(snapshot, query)),
    [plugins, query],
  );
  const limitations = [
    ...(skillsCapability?.limitations ?? []),
    ...(pluginsCapability?.limitations ?? []),
  ];

  async function previewToggle(snapshot: ResourceSnapshot) {
    const requestId = ++previewRequestRef.current;
    const requestContextKey = contextKey;
    const item = collectionItemView(snapshot);
    setError(null);
    setStatus(null);
    try {
      const nextPlan = await tauri.previewAgentCollectionToggle(
        context,
        snapshot.resource,
        !item.enabled,
      );
      if (
        requestId !== previewRequestRef.current ||
        requestContextKey !== activeContextKeyRef.current
      ) {
        return;
      }
      setPlan(nextPlan);
      setPlanError(null);
    } catch (caught) {
      if (
        requestId !== previewRequestRef.current ||
        requestContextKey !== activeContextKeyRef.current
      ) {
        return;
      }
      setError(formatAgentError(caught));
    }
  }

  async function applyPlan() {
    if (!plan) return;
    const requestContextKey = contextKey;
    setPlanBusy(true);
    setPlanError(null);
    try {
      const receipt = await tauri.applyAgentPlan(plan.id, context, plan.riskFingerprint);
      if (requestContextKey !== activeContextKeyRef.current) return;
      setPlan(null);
      if (receipt.status === 'partial_failure') {
        setLastReceipt(receipt);
        await load();
        if (requestContextKey !== activeContextKeyRef.current) return;
        setError(t('agentCollections.partialFailure'));
        notifyAgentWorkspaceChanged();
      } else if (receipt.status === 'compensated') {
        setLastReceipt(null);
        setError(t('agentCollections.compensated'));
      } else {
        setLastReceipt(receipt);
        setStatus(t('agentCollections.applySuccess'));
        await load();
        if (requestContextKey !== activeContextKeyRef.current) return;
        notifyAgentWorkspaceChanged();
      }
    } catch (caught) {
      if (requestContextKey !== activeContextKeyRef.current) return;
      setPlanError(formatAgentError(caught));
    } finally {
      setPlanBusy(false);
    }
  }

  async function rollback() {
    if (!lastReceipt) return;
    const requestContextKey = contextKey;
    setPlanBusy(true);
    setError(null);
    try {
      const rollbackPlan = await tauri.previewAgentRollback(lastReceipt.id, context);
      if (requestContextKey !== activeContextKeyRef.current) return;
      if (!window.confirm(t('agentCollections.rollbackConfirm'))) return;
      await tauri.applyAgentRollbackPlan(
        rollbackPlan.id,
        context,
        rollbackPlan.riskFingerprint,
        true,
      );
      if (requestContextKey !== activeContextKeyRef.current) return;
      setLastReceipt(null);
      setStatus(t('agentCollections.rollbackSuccess'));
      await load();
      if (requestContextKey !== activeContextKeyRef.current) return;
      notifyAgentWorkspaceChanged();
    } catch (caught) {
      if (requestContextKey !== activeContextKeyRef.current) return;
      setError(formatAgentError(caught));
    } finally {
      setPlanBusy(false);
    }
  }

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
          className="flex shrink-0 items-center justify-between gap-3 border-b border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          <span>{error}</span>
          {lastReceipt?.status === 'partial_failure' && (
            <Button type="button" size="sm" variant="outline" onClick={() => void rollback()}>
              <RotateCcw className="h-3.5 w-3.5" />
              {t('agentCollections.rollback')}
            </Button>
          )}
        </div>
      )}
      {status && (
        <div
          role="status"
          className="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-muted/40 px-3 py-2 text-xs text-foreground"
        >
          <span>{status}</span>
          {lastReceipt?.status === 'complete' && (
            <Button type="button" size="sm" variant="outline" onClick={() => void rollback()}>
              <RotateCcw className="h-3.5 w-3.5" />
              {t('agentCollections.rollback')}
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
        {filteredSkills.length > 0 && (
          <CollectionSection
            title={t('agentCollections.skills')}
            kind="skills"
            snapshots={filteredSkills}
            onToggle={previewToggle}
            capabilities={capabilities}
            t={t}
          />
        )}
        {filteredPlugins.length > 0 && (
          <CollectionSection
            title={t('agentCollections.plugins')}
            kind="plugins"
            snapshots={filteredPlugins}
            onToggle={previewToggle}
            capabilities={capabilities}
            t={t}
          />
        )}
        {filteredSkills.length === 0 && filteredPlugins.length === 0 && (
          <div role="status" className="py-12 text-center text-sm text-muted-foreground">
            {t('agentCollections.empty')}
          </div>
        )}
      </div>

      <AgentPlanDialog
        plan={plan}
        busy={planBusy}
        error={planError}
        onCancel={() => setPlan(null)}
        onConfirm={() => void applyPlan()}
      />
    </div>
  );
}

interface CollectionSectionProps {
  title: string;
  kind: 'skills' | 'plugins';
  snapshots: ResourceSnapshot[];
  onToggle: (snapshot: ResourceSnapshot) => Promise<void>;
  capabilities: CapabilityDescriptor[];
  t: ReturnType<typeof useTranslation>['t'];
}

function CollectionSection({
  title,
  kind,
  snapshots,
  onToggle,
  capabilities,
  t,
}: CollectionSectionProps) {
  return (
    <section className="mb-4" aria-labelledby={`collection-${title}`}>
      <h3
        id={`collection-${title}`}
        className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground"
      >
        {title}
      </h3>
      <ul className="divide-y divide-border overflow-hidden rounded-lg border border-border">
        {snapshots.map((snapshot) => {
          const item = collectionItemView(snapshot);
          const canToggle = capabilityAllows(
            capabilities,
            kind,
            item.enabled ? 'disable' : 'enable',
            snapshot.resource.scope,
          );
          return (
            <li
              key={`${snapshot.resource.scope}:${snapshot.resource.logicalId}`}
              className="flex items-center gap-3 px-3 py-2.5"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-medium">{item.name}</div>
                <div className="truncate text-xs text-muted-foreground">
                  {item.description ?? `${snapshot.resource.scope} · ${snapshot.location.path}`}
                </div>
              </div>
              <Toggle
                on={item.enabled}
                onChange={() => void onToggle(snapshot)}
                disabled={!canToggle}
                ariaLabel={t(
                  item.enabled ? 'agentCollections.disable' : 'agentCollections.enable',
                  {
                    name: item.name,
                  },
                )}
              />
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function matches(snapshot: ResourceSnapshot, query: string): boolean {
  if (!query) return true;
  const item = collectionItemView(snapshot);
  return [item.name, item.description, snapshot.resource.logicalId]
    .filter((value): value is string => typeof value === 'string')
    .some((value) => value.toLocaleLowerCase().includes(query));
}

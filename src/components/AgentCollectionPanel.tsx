import { useCallback, useEffect, useMemo, useState } from 'react';
import { Search } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { collectionItemView } from '@/lib/agentResourceViews';
import type {
  AgentContext,
  CapabilityDescriptor,
  MutationPlanView,
  ResourceSnapshot,
} from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';

import { AgentPlanDialog } from './AgentPlanDialog';
import { Toggle } from './SkillToggle';

interface AgentCollectionPanelProps {
  context: AgentContext;
  capabilities: CapabilityDescriptor[];
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

  const skillsCapability = capabilityFor(capabilities, 'skills');
  const pluginsCapability = capabilityFor(capabilities, 'plugins');
  const canListSkills = capabilityAllows(skillsCapability, 'list');
  const canListPlugins = capabilityAllows(pluginsCapability, 'list');

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [nextSkills, nextPlugins] = await Promise.all([
        canListSkills ? tauri.listAgentSkills(context) : Promise.resolve([]),
        canListPlugins ? tauri.listAgentPlugins(context) : Promise.resolve([]),
      ]);
      setSkills(nextSkills);
      setPlugins(nextPlugins);
    } catch (caught) {
      setError(messageOf(caught));
    } finally {
      setLoading(false);
    }
  }, [canListPlugins, canListSkills, context]);

  useEffect(() => {
    void load();
  }, [load]);

  const query = filter.trim().toLocaleLowerCase();
  const filteredSkills = useMemo(
    () => skills.filter((snapshot) => matches(snapshot, query)),
    [query, skills],
  );
  const filteredPlugins = useMemo(
    () => plugins.filter((snapshot) => matches(snapshot, query)),
    [plugins, query],
  );
  const limitations = [...(skillsCapability?.limitations ?? []), ...(pluginsCapability?.limitations ?? [])];

  async function previewToggle(snapshot: ResourceSnapshot) {
    const item = collectionItemView(snapshot);
    setError(null);
    try {
      const nextPlan = await tauri.previewAgentCollectionToggle(
        context,
        snapshot.resource,
        !item.enabled,
      );
      setPlan(nextPlan);
      setPlanError(null);
    } catch (caught) {
      setError(messageOf(caught));
    }
  }

  async function applyPlan() {
    if (!plan) return;
    setPlanBusy(true);
    setPlanError(null);
    try {
      await tauri.applyAgentPlan(plan.id);
      setPlan(null);
      await load();
    } catch (caught) {
      setPlanError(messageOf(caught));
    } finally {
      setPlanBusy(false);
    }
  }

  if (loading) {
    return <div className="flex h-full items-center justify-center text-sm text-muted-foreground" aria-busy="true">{t('agentCollections.loading')}</div>;
  }

  return (
    <div className="flex h-full flex-col overflow-hidden rounded-lg border border-border bg-card">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <Search className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
        <label htmlFor="agent-resource-filter" className="sr-only">{t('agentCollections.filter')}</label>
        <input
          id="agent-resource-filter"
          type="search"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
          placeholder={t('agentCollections.filter')}
          className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
        />
      </div>

      {error && <div role="alert" className="shrink-0 border-b border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</div>}
      {limitations.length > 0 && (
        <ul className="shrink-0 border-b border-amber-500/30 bg-amber-500/10 px-4 py-2 text-xs text-amber-800 dark:text-amber-200">
          {limitations.map((limitation) => (
            <li key={limitation.code}>{t(limitation.messageKey)}</li>
          ))}
        </ul>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        {filteredSkills.length > 0 && (
          <CollectionSection
            title={t('agentCollections.skills')}
            snapshots={filteredSkills}
            onToggle={previewToggle}
            t={t}
          />
        )}
        {filteredPlugins.length > 0 && (
          <CollectionSection
            title={t('agentCollections.plugins')}
            snapshots={filteredPlugins}
            onToggle={previewToggle}
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
  snapshots: ResourceSnapshot[];
  onToggle: (snapshot: ResourceSnapshot) => Promise<void>;
  t: ReturnType<typeof useTranslation>['t'];
}

function CollectionSection({ title, snapshots, onToggle, t }: CollectionSectionProps) {
  return (
    <section className="mb-4" aria-labelledby={`collection-${title}`}>
      <h3 id={`collection-${title}`} className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{title}</h3>
      <ul className="divide-y divide-border overflow-hidden rounded-lg border border-border">
        {snapshots.map((snapshot) => {
          const item = collectionItemView(snapshot);
          return (
            <li key={`${snapshot.resource.scope}:${snapshot.resource.logicalId}`} className="flex items-center gap-3 px-3 py-2.5">
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-medium">{item.name}</div>
                <div className="truncate text-xs text-muted-foreground">
                  {item.description ?? `${snapshot.resource.scope} · ${snapshot.location.path}`}
                </div>
              </div>
              <Toggle
                on={item.enabled}
                onChange={() => void onToggle(snapshot)}
                ariaLabel={t(item.enabled ? 'agentCollections.disable' : 'agentCollections.enable', {
                  name: item.name,
                })}
              />
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function capabilityFor(
  capabilities: CapabilityDescriptor[],
  kind: CapabilityDescriptor['kind'],
): CapabilityDescriptor | undefined {
  return capabilities.find((capability) => capability.kind === kind);
}

function capabilityAllows(
  capability: CapabilityDescriptor | undefined,
  operation: CapabilityDescriptor['operations'][number],
): boolean {
  return (
    capability !== undefined &&
    capability.availability !== 'unavailable' &&
    capability.operations.includes(operation)
  );
}

function matches(snapshot: ResourceSnapshot, query: string): boolean {
  if (!query) return true;
  const item = collectionItemView(snapshot);
  return [item.name, item.description, snapshot.resource.logicalId]
    .filter((value): value is string => typeof value === 'string')
    .some((value) => value.toLocaleLowerCase().includes(query));
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

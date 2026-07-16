import { useCallback, useEffect, useMemo, useState } from 'react';
import { CheckCircle2, RotateCcw, Save } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { editedResourceContent, editableResourceText } from '@/lib/agentResourceViews';
import { formatAgentError } from '@/lib/agentErrors';
import type {
  AgentContext,
  MutationPlanView,
  OperationReceipt,
  SettingsDocument,
} from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';
import { useUiSettings } from '@/store/uiSettings';

import { AgentPlanDialog } from './AgentPlanDialog';
import { JsonEditor } from './JsonEditor';
import { Button } from './ui/button';

interface AgentSettingsEditorProps {
  context: AgentContext;
}

export function AgentSettingsEditor({ context }: AgentSettingsEditorProps) {
  const { t } = useTranslation();
  const darkMode = useUiSettings((state) => state.darkMode);
  const [snapshots, setSnapshots] = useState<SettingsDocument[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [plan, setPlan] = useState<MutationPlanView | null>(null);
  const [planBusy, setPlanBusy] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  const [lastReceipt, setLastReceipt] = useState<OperationReceipt | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await tauri.listAgentSettingsDocuments(context);
      const nextDrafts = Object.fromEntries(
        next.map((snapshot) => [resourceKey(snapshot), editableResourceText(snapshot)]),
      );
      setSnapshots(next);
      setDrafts(nextDrafts);
      setSelectedKey((current) =>
        current && nextDrafts[current] !== undefined
          ? current
          : next[0]
            ? resourceKey(next[0])
            : null,
      );
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setLoading(false);
    }
  }, [context]);

  useEffect(() => {
    void load();
  }, [load]);

  const selected = useMemo(
    () => snapshots.find((snapshot) => resourceKey(snapshot) === selectedKey) ?? null,
    [selectedKey, snapshots],
  );
  const selectedText = selectedKey ? (drafts[selectedKey] ?? '') : '';
  const dirty = selected ? selectedText !== editableResourceText(selected) : false;

  async function preview() {
    if (!selected || !dirty) return;
    setError(null);
    setStatus(null);
    try {
      const content = editedResourceContent(selected, selectedText);
      const nextPlan = await tauri.previewAgentSettingsEdit(context, {
        resource: selected.resource,
        mediaType: selected.mediaType,
        content,
      });
      setPlan(nextPlan);
      setPlanError(null);
    } catch (caught) {
      setError(
        selected.mediaType === 'application/json' && caught instanceof SyntaxError
          ? t('agentSettings.invalidJson')
          : formatAgentError(caught),
      );
    }
  }

  async function applyPlan() {
    if (!plan) return;
    setPlanBusy(true);
    setPlanError(null);
    try {
      const receipt = await tauri.applyAgentPlan(plan.id);
      setLastReceipt(receipt);
      setPlan(null);
      setStatus(t('agentSettings.applySuccess'));
      await load();
    } catch (caught) {
      setPlanError(formatAgentError(caught));
    } finally {
      setPlanBusy(false);
    }
  }

  async function rollback() {
    if (!lastReceipt || !window.confirm(t('agentSettings.rollbackConfirm'))) return;
    setError(null);
    try {
      await tauri.rollbackAgentReceipt(lastReceipt.id, true);
      setLastReceipt(null);
      setStatus(t('agentSettings.rollbackSuccess'));
      await load();
    } catch (caught) {
      setError(formatAgentError(caught));
    }
  }

  if (loading) {
    return (
      <div
        className="flex h-full items-center justify-center text-sm text-muted-foreground"
        aria-busy="true"
      >
        {t('agentSettings.loading')}
      </div>
    );
  }

  if (error && snapshots.length === 0) {
    return (
      <div role="alert" className="p-6 text-sm text-destructive">
        {t('agentSettings.loadError', { error })}
      </div>
    );
  }

  if (!selected) {
    return (
      <div
        role="status"
        className="flex h-full items-center justify-center text-sm text-muted-foreground"
      >
        {t('agentSettings.empty')}
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden rounded-lg border border-border bg-card">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <div
          className="flex min-w-0 flex-1 gap-1 overflow-x-auto"
          role="tablist"
          aria-label={t('agentSettings.resources')}
        >
          {snapshots.map((snapshot) => {
            const key = resourceKey(snapshot);
            return (
              <button
                key={key}
                type="button"
                role="tab"
                aria-selected={key === selectedKey}
                onClick={() => setSelectedKey(key)}
                className="whitespace-nowrap rounded-md px-2.5 py-1.5 text-xs"
                style={{
                  background: key === selectedKey ? 'var(--ds-bg-hover)' : 'transparent',
                  color: key === selectedKey ? 'var(--ds-fg-1)' : 'var(--ds-fg-3)',
                }}
              >
                {t(`agentSettings.scope.${snapshot.resource.scope}`)} ·{' '}
                {snapshot.resource.logicalId}
              </button>
            );
          })}
        </div>
        {status && (
          <span
            role="status"
            className="flex items-center gap-1 whitespace-nowrap text-xs text-success"
          >
            <CheckCircle2 className="h-3.5 w-3.5" />
            {status}
          </span>
        )}
        {lastReceipt && (
          <Button type="button" size="sm" variant="outline" onClick={() => void rollback()}>
            <RotateCcw className="h-3.5 w-3.5" />
            {t('agentSettings.rollback')}
          </Button>
        )}
        <Button type="button" size="sm" onClick={() => void preview()} disabled={!dirty}>
          <Save className="h-3.5 w-3.5" />
          {t('agentSettings.preview')}
        </Button>
      </div>

      <div className="shrink-0 border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
        <span className="font-mono">{selected.location.path}</span>
        {dirty && (
          <span className="ml-2" aria-live="polite">
            · {t('agentSettings.unsaved')}
          </span>
        )}
      </div>
      {error && (
        <div
          role="alert"
          className="shrink-0 border-b border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          {error}
        </div>
      )}
      <div className="min-h-0 flex-1">
        <JsonEditor
          key={`${selected.resource.logicalId}:${selected.mediaType}`}
          value={selectedText}
          onChange={(next) => {
            if (selectedKey) setDrafts((current) => ({ ...current, [selectedKey]: next }));
          }}
          dark={darkMode}
          language={selected.mediaType === 'application/json' ? 'json' : 'text'}
        />
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

function resourceKey(snapshot: SettingsDocument): string {
  return `${snapshot.resource.scope}:${snapshot.resource.logicalId}`;
}

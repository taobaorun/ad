import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { CheckCircle2, RotateCcw, Save } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { editedResourceContent, editableResourceText } from '@/lib/agentResourceViews';
import { mergeSettingsDrafts, settingsDocumentKey } from '@/lib/agentSettingsDrafts';
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
  const snapshotsRef = useRef<SettingsDocument[]>([]);
  const draftsRef = useRef<Record<string, string>>({});
  const loadedContextRef = useRef<string | null>(null);
  const loadRequestRef = useRef(0);
  const previewRequestRef = useRef(0);
  const contextKey = useMemo(() => JSON.stringify(context), [context]);
  const activeContextKeyRef = useRef(contextKey);
  activeContextKeyRef.current = contextKey;

  const load = useCallback(async () => {
    const requestId = ++loadRequestRef.current;
    setLoading(true);
    setError(null);
    try {
      const next = await tauri.listAgentSettingsDocuments(context);
      if (requestId !== loadRequestRef.current) return;
      const contextKey = JSON.stringify(context);
      const sameContext = loadedContextRef.current === contextKey;
      const nextDrafts = mergeSettingsDrafts(
        sameContext ? snapshotsRef.current : [],
        sameContext ? draftsRef.current : {},
        next,
      );
      snapshotsRef.current = next;
      draftsRef.current = nextDrafts;
      loadedContextRef.current = contextKey;
      setSnapshots(next);
      setDrafts(nextDrafts);
      setSelectedKey((current) =>
        current && nextDrafts[current] !== undefined
          ? current
          : next[0]
            ? settingsDocumentKey(next[0])
            : null,
      );
    } catch (caught) {
      if (requestId !== loadRequestRef.current) return;
      setError(formatAgentError(caught));
    } finally {
      if (requestId === loadRequestRef.current) {
        setLoading(false);
      }
    }
  }, [context]);

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

  const selected = useMemo(
    () => snapshots.find((snapshot) => settingsDocumentKey(snapshot) === selectedKey) ?? null,
    [selectedKey, snapshots],
  );
  const selectedText = selectedKey ? (drafts[selectedKey] ?? '') : '';
  const dirty = selected ? selectedText !== editableResourceText(selected) : false;

  async function preview() {
    if (!selected || !dirty) return;
    const requestId = ++previewRequestRef.current;
    const requestContextKey = contextKey;
    setError(null);
    setStatus(null);
    try {
      const content = editedResourceContent(selected, selectedText);
      const nextPlan = await tauri.previewAgentSettingsEdit(context, {
        resource: selected.resource,
        mediaType: selected.mediaType,
        content,
      });
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
      setError(
        selected.mediaType === 'application/json' && caught instanceof SyntaxError
          ? t('agentSettings.invalidJson')
          : formatAgentError(caught),
      );
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
      setLastReceipt(receipt);
      setPlan(null);
      if (receipt.status === 'complete') {
        setStatus(t('agentSettings.applySuccess'));
        await load();
      } else if (receipt.status === 'compensated') {
        setLastReceipt(null);
        setError(t('agentSettings.compensated'));
      } else {
        await load();
        if (requestContextKey !== activeContextKeyRef.current) return;
        setError(t('agentSettings.partialFailure'));
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
      if (!window.confirm(t('agentSettings.rollbackConfirm'))) return;
      await tauri.applyAgentRollbackPlan(
        rollbackPlan.id,
        context,
        rollbackPlan.riskFingerprint,
        true,
      );
      if (requestContextKey !== activeContextKeyRef.current) return;
      setLastReceipt(null);
      setStatus(t('agentSettings.rollbackSuccess'));
      await load();
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
            const key = settingsDocumentKey(snapshot);
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
            className="flex items-center gap-1 whitespace-nowrap text-xs text-foreground"
          >
            <CheckCircle2 className="h-3.5 w-3.5 text-success" />
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
            if (selectedKey) {
              setDrafts((current) => {
                const nextDrafts = { ...current, [selectedKey]: next };
                draftsRef.current = nextDrafts;
                return nextDrafts;
              });
            }
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

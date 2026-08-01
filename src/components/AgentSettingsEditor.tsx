import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { CheckCircle2, Eye, RotateCcw, Save, ShieldAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import type { AgentContext, MutationPlanView, OperationReceipt } from '@/lib/agentTypes';
import type {
  ProjectWorkspaceInventory,
  SettingsLayerView,
} from '@/lib/agentResourceInventoryTypes';
import { tauri } from '@/lib/tauri';
import { WORKSPACE_DRAFT_GUARD_EVENT } from '@/lib/workspaceDraftGuard';
import { useUiSettings } from '@/store/uiSettings';

import { AgentPlanDialog } from './AgentPlanDialog';
import { JsonEditor } from './JsonEditor';
import { Button } from './ui/button';

interface AgentSettingsEditorProps {
  context: AgentContext;
}

const EFFECTIVE_KEY = 'effective';

function layerText(layer: SettingsLayerView): string {
  return JSON.stringify(layer.content, null, 2);
}

function layerKey(layer: SettingsLayerView): string {
  return layer.declaration.key;
}

export function AgentSettingsEditor({ context }: AgentSettingsEditorProps) {
  const { t } = useTranslation();
  const darkMode = useUiSettings((state) => state.darkMode);
  const [inventory, setInventory] = useState<ProjectWorkspaceInventory | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [selectedKey, setSelectedKey] = useState(EFFECTIVE_KEY);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [plan, setPlan] = useState<MutationPlanView | null>(null);
  const [planBusy, setPlanBusy] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  const [lastReceipt, setLastReceipt] = useState<OperationReceipt | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const inventoryRef = useRef<ProjectWorkspaceInventory | null>(null);
  const draftsRef = useRef<Record<string, string>>({});
  const loadRequestRef = useRef(0);
  const previewRequestRef = useRef(0);
  const contextKey = useMemo(() => JSON.stringify(context), [context]);
  const activeContextKeyRef = useRef(contextKey);
  activeContextKeyRef.current = contextKey;

  const load = useCallback(
    async (discardDraftKey?: string) => {
      const requestId = ++loadRequestRef.current;
      const requestContextKey = JSON.stringify(context);
      setLoading(true);
      setError(null);
      try {
        if (!context.projectPath) {
          throw new Error('Project Settings require a project context');
        }
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
        const previous = inventoryRef.current;
        const sameWorkspace = previous?.workspace.key === next.workspace.key;
        const nextDrafts = Object.fromEntries(
          next.settings.layers.map((layer) => {
            const key = layerKey(layer);
            const baseline = layerText(layer);
            if (!sameWorkspace || key === discardDraftKey) return [key, baseline];
            const previousLayer = previous?.settings.layers.find(
              (candidate) => layerKey(candidate) === key,
            );
            const previousBaseline = previousLayer ? layerText(previousLayer) : baseline;
            const previousDraft = draftsRef.current[key] ?? previousBaseline;
            return [key, previousDraft === previousBaseline ? baseline : previousDraft];
          }),
        );
        inventoryRef.current = next;
        draftsRef.current = nextDrafts;
        setInventory(next);
        setDrafts(nextDrafts);
        setSelectedKey((current) => {
          if (current === EFFECTIVE_KEY) return current;
          if (next.settings.layers.some((layer) => layerKey(layer) === current)) return current;
          return next.settings.editableTargets[0]?.declarationKey ?? EFFECTIVE_KEY;
        });
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
    },
    [context],
  );

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

  const selectedLayer = useMemo(
    () => inventory?.settings.layers.find((layer) => layerKey(layer) === selectedKey) ?? null,
    [inventory, selectedKey],
  );
  const selectedTarget = useMemo(
    () =>
      inventory?.settings.editableTargets.find((target) => target.declarationKey === selectedKey) ??
      null,
    [inventory, selectedKey],
  );
  const workspaceContext = useMemo<AgentContext>(
    () =>
      inventory
        ? {
            installationId: inventory.workspace.effectiveInstallationId,
            projectPath: inventory.workspace.canonicalProjectPath,
          }
        : context,
    [context, inventory],
  );
  const effectiveText = useMemo(
    () => JSON.stringify(inventory?.settings.effectiveContent ?? {}, null, 2),
    [inventory],
  );
  const selectedText =
    selectedKey === EFFECTIVE_KEY ? effectiveText : (drafts[selectedKey] ?? '{}');
  const selectedBaseline = selectedLayer ? layerText(selectedLayer) : effectiveText;
  const dirty = selectedTarget !== null && selectedText !== selectedBaseline;
  const hasDirtyDraft = useMemo(
    () =>
      inventory?.settings.editableTargets.some((target) => {
        const layer = inventory.settings.layers.find(
          (candidate) => layerKey(candidate) === target.declarationKey,
        );
        return layer !== undefined && drafts[target.declarationKey] !== layerText(layer);
      }) ?? false,
    [drafts, inventory],
  );

  useEffect(() => {
    if (!hasDirtyDraft) return;
    const guardWorkspaceChange = (event: Event) => {
      if (!window.confirm(t('agentSettings.discardDraftConfirm'))) event.preventDefault();
    };
    const guardWindowClose = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener(WORKSPACE_DRAFT_GUARD_EVENT, guardWorkspaceChange);
    window.addEventListener('beforeunload', guardWindowClose);
    return () => {
      window.removeEventListener(WORKSPACE_DRAFT_GUARD_EVENT, guardWorkspaceChange);
      window.removeEventListener('beforeunload', guardWindowClose);
    };
  }, [hasDirtyDraft, t]);

  async function preview() {
    if (!selectedTarget || !dirty) return;
    const requestId = ++previewRequestRef.current;
    const requestContextKey = contextKey;
    setError(null);
    setStatus(null);
    try {
      const content = JSON.parse(selectedText) as unknown;
      const nextPlan = await tauri.previewAgentSettingsEdit(workspaceContext, {
        resource: selectedTarget.resource,
        mediaType: selectedTarget.mediaType,
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
        caught instanceof SyntaxError ? t('agentSettings.invalidJson') : formatAgentError(caught),
      );
    }
  }

  async function applyPlan() {
    if (!plan || !selectedTarget) return;
    const requestContextKey = contextKey;
    const appliedDraftKey = selectedTarget.declarationKey;
    setPlanBusy(true);
    setPlanError(null);
    try {
      const receipt = await tauri.applyAgentPlan(plan.id, plan.context, plan.riskFingerprint);
      if (requestContextKey !== activeContextKeyRef.current) return;
      setLastReceipt(receipt);
      setPlan(null);
      if (receipt.status === 'complete') {
        setStatus(t('agentSettings.applySuccess'));
        await load(appliedDraftKey);
      } else if (receipt.status === 'compensated') {
        setLastReceipt(null);
        setError(t('agentSettings.compensated'));
      } else {
        await load(appliedDraftKey);
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
      const rollbackContext = lastReceipt.context ?? workspaceContext;
      const rollbackPlan = await tauri.previewAgentRollback(lastReceipt.id, rollbackContext);
      if (requestContextKey !== activeContextKeyRef.current) return;
      if (!window.confirm(t('agentSettings.rollbackConfirm'))) return;
      await tauri.applyAgentRollbackPlan(
        rollbackPlan.id,
        rollbackContext,
        rollbackPlan.riskFingerprint,
        true,
      );
      if (requestContextKey !== activeContextKeyRef.current) return;
      setLastReceipt(null);
      setStatus(t('agentSettings.rollbackSuccess'));
      await load(selectedTarget?.declarationKey);
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

  if (error && !inventory) {
    return (
      <div role="alert" className="p-6 text-sm text-destructive">
        {t('agentSettings.loadError', { error })}
      </div>
    );
  }

  if (!inventory) {
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
      {inventory.settings.coverage.status !== 'complete' && (
        <div
          className="flex shrink-0 items-center gap-2 border-b border-warning/40 bg-warning/10 px-3 py-2 text-xs text-foreground"
          role="status"
        >
          <ShieldAlert className="h-3.5 w-3.5" aria-hidden="true" />
          {t(`agentSettings.coverage.${inventory.settings.coverage.status}`)}
        </div>
      )}
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <div
          className="flex min-w-0 flex-1 gap-1 overflow-x-auto"
          role="tablist"
          aria-label={t('agentSettings.resources')}
        >
          <SettingsTab
            active={selectedKey === EFFECTIVE_KEY}
            onClick={() => setSelectedKey(EFFECTIVE_KEY)}
          >
            {t('agentSettings.effective')}
          </SettingsTab>
          {inventory.settings.layers.map((layer) => (
            <SettingsTab
              key={layerKey(layer)}
              active={selectedKey === layerKey(layer)}
              onClick={() => setSelectedKey(layerKey(layer))}
            >
              {t(`agentSettings.layer.${layer.declaration.layer}`)} · {layer.logicalId}
            </SettingsTab>
          ))}
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

      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
        <Eye className="h-3.5 w-3.5" aria-hidden="true" />
        <span className="font-mono">
          {selectedLayer?.declaration.sourceId ?? t('agentSettings.effective')}
        </span>
        {!selectedTarget && <span>· {t('agentSettings.readOnly')}</span>}
        {dirty && <span aria-live="polite">· {t('agentSettings.unsaved')}</span>}
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
          key={selectedKey}
          value={selectedText}
          onChange={(next) => {
            if (!selectedTarget) return;
            setDrafts((current) => {
              const nextDrafts = { ...current, [selectedKey]: next };
              draftsRef.current = nextDrafts;
              return nextDrafts;
            });
          }}
          dark={darkMode}
          readOnly={!selectedTarget}
          language="json"
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

function SettingsTab({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className="whitespace-nowrap rounded-md px-2.5 py-1.5 text-xs"
      style={{
        background: active ? 'var(--ds-bg-hover)' : 'transparent',
        color: active ? 'var(--ds-fg-1)' : 'var(--ds-fg-3)',
      }}
    >
      {children}
    </button>
  );
}

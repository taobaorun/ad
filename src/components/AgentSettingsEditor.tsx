import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { CheckCircle2, Eye, RotateCcw, Save } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import type {
  AgentContext,
  MutationPlanView,
  OperationReceipt,
  ResourceRef,
} from '@/lib/agentTypes';
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

type SettingsOperation =
  | { kind: 'save'; draftKey: string; text: string; direct: boolean }
  | { kind: 'undo'; draftKey: string; text: string };

// The backend compares real file contents and defaults this hint to false.
// Keep the UI intent bound to the exact context and target the user selected.
function isDirectSettingsSave(
  plan: MutationPlanView,
  context: AgentContext,
  resource: ResourceRef,
): boolean {
  const change = plan.changes[0];
  return (
    plan.directApplyEligible &&
    plan.requiredAcknowledgements.length === 0 &&
    plan.changes.length === 1 &&
    !!change &&
    plan.context.installationId === context.installationId &&
    plan.context.projectPath === context.projectPath &&
    change.resource.installationId === resource.installationId &&
    change.resource.projectPath === resource.projectPath &&
    change.resource.logicalId === resource.logicalId &&
    change.resource.scope === 'project' &&
    change.resource.kind === 'settings'
  );
}

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
  const [lastSave, setLastSave] = useState<{
    receipt: OperationReceipt;
    draftKey: string;
    direct: boolean;
  } | null>(null);
  const pendingOperationRef = useRef<SettingsOperation | null>(null);
  const operationBusyRef = useRef(false);
  const pendingRefreshRef = useRef<{ key: string; text: string; saved: boolean } | undefined>(
    undefined,
  );
  const [status, setStatus] = useState<string | null>(null);
  const inventoryRef = useRef<ProjectWorkspaceInventory | null>(null);
  const draftsRef = useRef<Record<string, string>>({});
  const loadRequestRef = useRef(0);
  const previewRequestRef = useRef(0);
  const contextKey = useMemo(() => JSON.stringify(context), [context]);
  const activeContextKeyRef = useRef(contextKey);
  activeContextKeyRef.current = contextKey;

  const load = useCallback(
    async (savedDraft = pendingRefreshRef.current) => {
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
            if (!sameWorkspace) return [key, baseline];
            if (key === savedDraft?.key) {
              const currentDraft = draftsRef.current[key] ?? baseline;
              return [key, currentDraft === savedDraft.text ? baseline : currentDraft];
            }
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
        if (pendingRefreshRef.current === savedDraft) pendingRefreshRef.current = undefined;
        return true;
      } catch (caught) {
        if (
          requestId !== loadRequestRef.current ||
          requestContextKey !== activeContextKeyRef.current
        ) {
          return;
        }
        setError(formatAgentError(caught));
        return false;
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
    const reloadWorkspace = () => void load();
    window.addEventListener('ad:agent-workspace-changed', reloadWorkspace);
    return () => window.removeEventListener('ad:agent-workspace-changed', reloadWorkspace);
  }, [load]);

  useEffect(() => {
    previewRequestRef.current += 1;
    setPlan(null);
    setPlanError(null);
    setLastSave(null);
    setStatus(null);
    setPlanBusy(false);
    operationBusyRef.current = false;
    pendingOperationRef.current = null;
    pendingRefreshRef.current = undefined;
    setInventory(null);
  }, [contextKey]);

  useEffect(() => {
    activeContextKeyRef.current = contextKey;
    return () => {
      previewRequestRef.current += 1;
      activeContextKeyRef.current = '';
    };
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
  const savedAwaitingRefresh = pendingRefreshRef.current?.saved
    ? pendingRefreshRef.current
    : undefined;
  const selectedComparison =
    savedAwaitingRefresh?.key === selectedKey ? savedAwaitingRefresh.text : selectedBaseline;
  const dirty = selectedTarget !== null && selectedText !== selectedComparison;
  const hasDirtyDraft = useMemo(
    () =>
      inventory?.settings.editableTargets.some((target) => {
        const layer = inventory.settings.layers.find(
          (candidate) => layerKey(candidate) === target.declarationKey,
        );
        const baseline =
          savedAwaitingRefresh?.key === target.declarationKey
            ? savedAwaitingRefresh.text
            : layer
              ? layerText(layer)
              : undefined;
        return layer !== undefined && drafts[target.declarationKey] !== baseline;
      }) ?? false,
    [drafts, inventory, savedAwaitingRefresh],
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

  async function executePlan(nextPlan: MutationPlanView, operation: SettingsOperation) {
    const requestContextKey = contextKey;
    const requestId = previewRequestRef.current;
    const receipt =
      operation.kind === 'undo'
        ? await tauri.applyAgentRollbackPlan(
            nextPlan.id,
            nextPlan.context,
            nextPlan.riskFingerprint,
            true,
          )
        : await tauri.applyAgentPlan(nextPlan.id, nextPlan.context, nextPlan.riskFingerprint);
    if (
      requestContextKey !== activeContextKeyRef.current ||
      requestId !== previewRequestRef.current
    )
      return;
    setPlan(null);
    pendingOperationRef.current = null;
    const savedDraft = {
      key: operation.draftKey,
      text: operation.text,
      saved: operation.kind === 'save' && receipt.status === 'complete',
    };
    if (operation.kind === 'save') {
      setLastSave(
        receipt.rollback.available
          ? {
              receipt,
              draftKey: operation.draftKey,
              direct: operation.direct,
            }
          : null,
      );
    } else if (receipt.status !== 'compensated') {
      setLastSave(null);
    }
    if (receipt.status === 'compensated') {
      setError(
        t(
          operation.kind === 'undo' ? 'agentSettings.undoCompensated' : 'agentSettings.compensated',
        ),
      );
      return;
    }
    // Refresh never reapplies an already completed mutation. Preserve any draft
    // typed after the submitted snapshot while waiting for the backend.
    pendingRefreshRef.current = savedDraft;
    const refreshed = await load(savedDraft);
    if (
      requestContextKey !== activeContextKeyRef.current ||
      requestId !== previewRequestRef.current
    )
      return;
    if (receipt.status === 'partial_failure') {
      setStatus(null);
      setError(
        t(
          operation.kind === 'undo'
            ? 'agentSettings.undoPartialFailure'
            : 'agentSettings.partialFailure',
        ),
      );
    } else {
      setStatus(
        t(
          operation.kind === 'undo'
            ? 'agentSettings.rollbackSuccess'
            : 'agentSettings.applySuccess',
        ),
      );
      if (!refreshed) setError(t('agentSettings.refreshFailed'));
    }
  }

  async function preview() {
    if (!selectedTarget || !selectedLayer || !dirty || operationBusyRef.current) return;
    operationBusyRef.current = true;
    const requestId = ++previewRequestRef.current;
    const requestContextKey = contextKey;
    const submittedText = selectedText;
    setPlanBusy(true);
    setError(null);
    setStatus(null);
    setPlanError(null);
    try {
      const content = JSON.parse(submittedText) as unknown;
      if (!content || typeof content !== 'object' || Array.isArray(content))
        throw new SyntaxError('Settings must be an object');
      const nextPlan = await tauri.previewAgentSettingsEdit(workspaceContext, {
        resource: selectedTarget.resource,
        mediaType: selectedTarget.mediaType,
        content,
      });
      if (
        requestId !== previewRequestRef.current ||
        requestContextKey !== activeContextKeyRef.current
      )
        return;
      const operation: SettingsOperation = {
        kind: 'save',
        draftKey: selectedTarget.declarationKey,
        text: submittedText,
        direct: isDirectSettingsSave(nextPlan, workspaceContext, selectedTarget.resource),
      };
      if (operation.direct) {
        await executePlan(nextPlan, operation);
      } else {
        pendingOperationRef.current = operation;
        setPlan(nextPlan);
      }
    } catch (caught) {
      if (
        requestId !== previewRequestRef.current ||
        requestContextKey !== activeContextKeyRef.current
      )
        return;
      setError(
        caught instanceof SyntaxError ? t('agentSettings.invalidJson') : formatAgentError(caught),
      );
    } finally {
      if (
        requestId === previewRequestRef.current &&
        requestContextKey === activeContextKeyRef.current
      ) {
        operationBusyRef.current = false;
        setPlanBusy(false);
      }
    }
  }

  async function applyPlan() {
    if (!plan || !pendingOperationRef.current || operationBusyRef.current) return;
    operationBusyRef.current = true;
    const requestId = previewRequestRef.current;
    const requestContextKey = contextKey;
    setPlanBusy(true);
    setPlanError(null);
    try {
      await executePlan(plan, pendingOperationRef.current);
    } catch (caught) {
      if (
        requestId === previewRequestRef.current &&
        requestContextKey === activeContextKeyRef.current
      ) {
        setPlanError(formatAgentError(caught));
      }
    } finally {
      if (
        requestId === previewRequestRef.current &&
        requestContextKey === activeContextKeyRef.current
      ) {
        operationBusyRef.current = false;
        setPlanBusy(false);
      }
    }
  }

  async function rollback() {
    if (!lastSave?.receipt.rollback.available || hasDirtyDraft || operationBusyRef.current) return;
    operationBusyRef.current = true;
    const requestId = ++previewRequestRef.current;
    const requestContextKey = contextKey;
    const undoText = draftsRef.current[lastSave.draftKey] ?? '';
    setPlanBusy(true);
    setError(null);
    setStatus(null);
    setPlanError(null);
    try {
      const rollbackContext = lastSave.receipt.context ?? workspaceContext;
      const rollbackPlan = await tauri.previewAgentRollback(lastSave.receipt.id, rollbackContext);
      if (
        requestId !== previewRequestRef.current ||
        requestContextKey !== activeContextKeyRef.current
      )
        return;
      const operation: SettingsOperation = {
        kind: 'undo',
        draftKey: lastSave.draftKey,
        text: undoText,
      };
      if (
        lastSave.direct &&
        rollbackPlan.requiredAcknowledgements.every(
          (item) => item.code === 'rollback_apply' && item.risk === 'confirmation',
        )
      ) {
        await executePlan(rollbackPlan, operation);
      } else {
        pendingOperationRef.current = operation;
        setPlan(rollbackPlan);
      }
    } catch (caught) {
      if (
        requestId === previewRequestRef.current &&
        requestContextKey === activeContextKeyRef.current
      ) {
        setError(t('agentSettings.undoFailed', { error: formatAgentError(caught) }));
      }
    } finally {
      if (
        requestId === previewRequestRef.current &&
        requestContextKey === activeContextKeyRef.current
      ) {
        operationBusyRef.current = false;
        setPlanBusy(false);
      }
    }
  }

  if (loading && !inventory) {
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
          className={`shrink-0 border-b px-3 py-2 text-xs ${
            inventory.settings.coverage.status === 'failed'
              ? 'border-warning/40 bg-warning/10 text-foreground'
              : 'border-border bg-muted/30 text-muted-foreground'
          }`}
          role="status"
        >
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
        {lastSave?.receipt.rollback.available && (
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => void rollback()}
            disabled={planBusy || hasDirtyDraft}
            title={hasDirtyDraft ? t('agentSettings.undoDirtyHint') : undefined}
          >
            <RotateCcw className="h-3.5 w-3.5" />
            {t('agentSettings.rollback')}
          </Button>
        )}
        <Button
          type="button"
          size="sm"
          onClick={() => void preview()}
          disabled={!dirty || planBusy || loading}
        >
          <Save className="h-3.5 w-3.5" />
          {t(planBusy ? 'agentSettings.saving' : 'agentSettings.save')}
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
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={planBusy || loading}
            onClick={() => void load()}
          >
            {t('agentSettings.refresh')}
          </Button>
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
        description={
          pendingOperationRef.current?.kind === 'save'
            ? plan?.changedSettingsKeys.length
              ? t('agentSettings.reviewFields', {
                  fields: plan.changedSettingsKeys.map((key) => key || '""').join(', '),
                })
              : t('agentSettings.reviewReplacement')
            : undefined
        }
        busy={planBusy}
        error={planError}
        onCancel={() => {
          setPlan(null);
          setPlanError(null);
          pendingOperationRef.current = null;
        }}
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

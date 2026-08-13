import { useEffect, useMemo, useRef, useState } from 'react';
import { RotateCcw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import {
  AgentContextSchema,
  type AgentInstallation,
  type ConversionProgressEvent,
  type ConversionRoutePreview,
  type PlanAcknowledgement,
} from '@/lib/agentTypes';
import type { OperationReceipt } from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';
import {
  clearConversionOperation,
  runDetachedConversionOperation,
  useConversionOperations,
} from '@/store/conversionOperations';
import { DetachedOperationInProgressError } from '@/store/detachedOperations';

import { Button } from './ui/button';
import { Dialog } from './ui/dialog';
import { AgentConversionArtifacts } from './AgentConversionArtifacts';
import {
  AgentConversionProgress,
  type ConversionOperationProgress,
} from './AgentConversionProgress';
import { AgentConversionRiskDialog } from './AgentConversionRiskDialog';
import { AgentConversionReport } from './AgentConversionReport';
import {
  AgentConversionOptions,
  type ConversionScope,
  type PermissionPreset,
} from './AgentConversionOptions';

type BusyOperation = 'preview' | 'apply' | 'rollback' | null;

function receiptCanRollback(receipt: OperationReceipt | null): boolean {
  return receipt?.status === 'complete' || receipt?.status === 'partial_failure';
}

async function resolveConversionContext(
  installationId: AgentInstallation['id'],
  scope: ConversionScope,
  projectPath: string | null,
) {
  if (scope === 'project' && projectPath) {
    return tauri.resolveAgentContext(installationId, projectPath);
  }
  return AgentContextSchema.parse({ installationId });
}

interface AgentConversionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  sourceInstallations: AgentInstallation[];
  targetInstallations: AgentInstallation[];
  activeProjectPath: string | null;
  inheritBaseConfig: boolean;
  preferProjectScope: boolean;
}

export function AgentConversionDialog({
  open,
  onOpenChange,
  sourceInstallations,
  targetInstallations,
  activeProjectPath,
  inheritBaseConfig,
  preferProjectScope,
}: AgentConversionDialogProps) {
  const { t } = useTranslation();
  const [sourceId, setSourceId] = useState(sourceInstallations[0]?.id ?? null);
  const [targetId, setTargetId] = useState(targetInstallations[0]?.id ?? null);
  const [scope, setScope] = useState<ConversionScope>('user');
  const [targetModel, setTargetModel] = useState('');
  const [permissionPreset, setPermissionPreset] = useState<PermissionPreset>('');
  const [profileId, setProfileId] = useState('');
  const [safeSubset, setSafeSubset] = useState(false);
  const [preview, setPreview] = useState<ConversionRoutePreview | null>(null);
  const [busyOperation, setBusyOperation] = useState<BusyOperation>(null);
  const [progress, setProgress] = useState<ConversionOperationProgress | null>(null);
  const [operationStartedAt, setOperationStartedAt] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [dangerConfirmOpen, setDangerConfirmOpen] = useState(false);
  const previewWorkspaceKey = preview?.report.workspaceKey;
  const trackedOperation = useConversionOperations((state) =>
    previewWorkspaceKey ? state.operations[previewWorkspaceKey] : undefined,
  );
  const activeOperation =
    trackedOperation && preview?.plan?.id === trackedOperation.operationId
      ? trackedOperation
      : undefined;
  const receipt = activeOperation?.report?.receipt ?? null;
  const applying = trackedOperation?.status === 'applying';
  const previewRequestRef = useRef(0);
  const activeProjectPathRef = useRef(activeProjectPath);
  activeProjectPathRef.current = activeProjectPath;
  const busy = busyOperation !== null || applying;
  const eligibleTargetInstallations = useMemo(
    () =>
      targetInstallations.filter((installation) => {
        const derived = Boolean(installation.projectPath || installation.baseInstallationId);
        if (!derived) return true;
        return (
          scope === 'project' &&
          activeProjectPath !== null &&
          installation.projectPath === activeProjectPath
        );
      }),
    [activeProjectPath, scope, targetInstallations],
  );

  useEffect(() => {
    if (!sourceInstallations.some((installation) => installation.id === sourceId)) {
      setSourceId(sourceInstallations[0]?.id ?? null);
    }
  }, [sourceId, sourceInstallations]);
  useEffect(() => {
    if (!eligibleTargetInstallations.some((installation) => installation.id === targetId)) {
      setTargetId(eligibleTargetInstallations[0]?.id ?? null);
    }
  }, [eligibleTargetInstallations, targetId]);
  useEffect(() => {
    previewRequestRef.current += 1;
    setBusyOperation((current) => (current === 'preview' ? null : current));
    if (!activeProjectPath) setScope('user');
    setTargetModel('');
    setPermissionPreset('');
    setProfileId('');
    setSafeSubset(false);
    setPreview(null);
    setError(null);
    setProgress(null);
    return () => {
      previewRequestRef.current += 1;
    };
  }, [activeProjectPath]);
  useEffect(() => {
    if (open && preferProjectScope && activeProjectPath) {
      setScope('project');
      resetResult();
    }
  }, [activeProjectPath, open, preferProjectScope]);
  useEffect(() => {
    if (!inheritBaseConfig) {
      setProfileId('');
      resetResult();
    }
  }, [inheritBaseConfig]);

  const source = useMemo(
    () => sourceInstallations.find((installation) => installation.id === sourceId),
    [sourceId, sourceInstallations],
  );
  const target = useMemo(
    () => eligibleTargetInstallations.find((installation) => installation.id === targetId),
    [eligibleTargetInstallations, targetId],
  );

  function resetResult() {
    setPreview(null);
    setError(null);
    setDangerConfirmOpen(false);
  }

  function resetDecisions() {
    setTargetModel('');
    setPermissionPreset('');
    setProfileId('');
    setSafeSubset(false);
  }

  async function runPreview(requestedSafeSubset = safeSubset) {
    if (!source || !target) return;
    const requestId = ++previewRequestRef.current;
    const requestedProjectPath = activeProjectPath;
    const isCurrentRequest = () =>
      requestId === previewRequestRef.current &&
      requestedProjectPath === activeProjectPathRef.current;
    setBusyOperation('preview');
    setOperationStartedAt(Date.now());
    setProgress({ phase: 'resolving_contexts', current: 0 });
    setError(null);
    try {
      const [sourceContext, targetContext] = await Promise.all([
        resolveConversionContext(source.id, scope, activeProjectPath),
        resolveConversionContext(target.id, scope, activeProjectPath),
      ]);
      if (!isCurrentRequest()) return;
      const result = await tauri.previewClaudeToCodexRoute(
        sourceContext,
        targetContext,
        {
          ...(targetModel ? { targetModel } : {}),
          ...(permissionPreset ? { permissionPreset } : {}),
          ...(scope === 'project' && profileId ? { profileId } : {}),
          ...(scope === 'project' ? { inheritBaseConfig } : {}),
          ...(requestedSafeSubset ? { safeSubset: true } : {}),
        },
        (event: ConversionProgressEvent) => {
          if (isCurrentRequest()) setProgress(event);
        },
      );
      if (!isCurrentRequest()) return;
      setSafeSubset(result.safeSubset);
      setPreview(result);
    } catch (caught) {
      if (isCurrentRequest()) setError(formatAgentError(caught));
    } finally {
      if (isCurrentRequest()) {
        setProgress(null);
        setBusyOperation(null);
      }
    }
  }

  async function applyConversion() {
    if (!preview?.plan) return;
    if (
      preview.plan.requiredAcknowledgements.some((requirement) => requirement.risk === 'dangerous')
    ) {
      setDangerConfirmOpen(true);
      return;
    }
    await submitConversion();
  }

  async function submitConversion() {
    if (!preview?.plan || !target) return;
    const requestedWorkspaceKey = preview.report.workspaceKey;
    setDangerConfirmOpen(false);
    setBusyOperation('apply');
    setOperationStartedAt(Date.now());
    setProgress({ phase: 'applying', current: 0 });
    setError(null);
    try {
      // Project conversion previews are rebound by the backend from the selected
      // Base Codex installation to an isolated derived runtime. Applying with a
      // newly resolved Base context would make the backend-owned plan reject
      // itself as stale, so the plan view remains the context authority here.
      const expectedContext = preview.plan.context;
      const acknowledgements: PlanAcknowledgement[] = preview.plan.requiredAcknowledgements.map(
        (requirement) => ({
          code: requirement.code,
          accepted: true,
        }),
      );
      const task = runDetachedConversionOperation(requestedWorkspaceKey, preview.plan.id, () =>
        tauri.applyConversionPlan(
          preview.plan!.id,
          expectedContext,
          preview.plan!.riskFingerprint,
          acknowledgements,
        ),
      );
      setProgress(null);
      setBusyOperation(null);
      await task;
    } catch (caught) {
      if (preview?.report.workspaceKey === requestedWorkspaceKey) {
        setError(
          caught instanceof DetachedOperationInProgressError
            ? t('agentConversion.operationInProgress')
            : formatAgentError(caught),
        );
      }
    } finally {
      setProgress(null);
      setBusyOperation(null);
    }
  }

  async function rollback() {
    if (!receipt || !target) return;
    setBusyOperation('rollback');
    setError(null);
    try {
      const expectedContext =
        receipt.context ??
        preview?.plan?.context ??
        (await resolveConversionContext(target.id, scope, activeProjectPath));
      const rollbackPlan = await tauri.previewAgentRollback(receipt.id, expectedContext);
      if (!window.confirm(t('agentConversion.rollbackConfirm'))) return;
      await tauri.applyAgentRollbackPlan(
        rollbackPlan.id,
        expectedContext,
        rollbackPlan.riskFingerprint,
        true,
      );
      if (preview) clearConversionOperation(preview.report.workspaceKey);
      setPreview(null);
      window.dispatchEvent(new Event('ad:project-codex-runtime-changed'));
      window.dispatchEvent(new Event('ad:agent-workspace-changed'));
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusyOperation(null);
    }
  }

  const showInstallationControls =
    sourceInstallations.length > 1 || eligibleTargetInstallations.length > 1;
  const displayedError = error ?? activeOperation?.error ?? null;

  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(nextOpen) => {
          if (busyOperation === null && !dangerConfirmOpen) onOpenChange(nextOpen);
        }}
        title={t('agentConversion.title')}
        description={t('agentConversion.description')}
        size="lg"
        closeDisabled={busyOperation !== null}
        footer={
          <div data-testid="conversion-sticky-footer" className="[&>[role=status]]:mt-0">
            {(progress || applying) && (
              <AgentConversionProgress
                progress={progress ?? { phase: 'applying', current: 0 }}
                startedAt={
                  trackedOperation
                    ? new Date(trackedOperation.startedAt).getTime()
                    : operationStartedAt
                }
              />
            )}
            <div
              className={`flex items-center justify-between gap-3 ${progress || applying ? 'mt-3' : ''}`}
            >
              <Button
                type="button"
                variant="ghost"
                onClick={() => onOpenChange(false)}
                disabled={busyOperation !== null}
              >
                {t('agentConversion.close')}
              </Button>
              <div className="flex gap-2">
                {receiptCanRollback(receipt) && (
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => void rollback()}
                    disabled={busy}
                  >
                    <RotateCcw className="h-3.5 w-3.5" />
                    {t('agentConversion.rollback')}
                  </Button>
                )}
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void runPreview()}
                  disabled={busy || !source || !target}
                >
                  {busyOperation === 'preview'
                    ? t('agentConversion.previewing')
                    : t('agentConversion.preview')}
                </Button>
                {preview?.plan && !receipt && activeOperation?.status !== 'failed' && (
                  <Button type="button" onClick={() => void applyConversion()} disabled={busy}>
                    {busyOperation === 'apply' || applying
                      ? t('agentConversion.applying')
                      : preview.safeSubset
                        ? t('agentConversion.applySafeSubset')
                        : t('agentConversion.apply')}
                  </Button>
                )}
              </div>
            </div>
          </div>
        }
      >
        <AgentConversionOptions
          scope={scope}
          activeProjectPath={activeProjectPath}
          busy={busy}
          showInstallationControls={showInstallationControls}
          sourceInstallations={sourceInstallations}
          targetInstallations={eligibleTargetInstallations}
          sourceId={sourceId}
          targetId={targetId}
          targetModel={targetModel}
          permissionPreset={permissionPreset}
          profileId={profileId}
          inheritBaseConfig={inheritBaseConfig}
          onScopeChange={(nextScope) => {
            setScope(nextScope);
            resetDecisions();
            resetResult();
          }}
          onSourceChange={(id) => {
            setSourceId(id);
            resetDecisions();
            resetResult();
          }}
          onTargetChange={(id) => {
            setTargetId(id);
            resetDecisions();
            resetResult();
          }}
          onTargetModelChange={(value) => {
            setTargetModel(value);
            resetResult();
          }}
          onPermissionPresetChange={(value) => {
            setPermissionPreset(value);
            resetResult();
          }}
          onProfileIdChange={(value) => {
            setProfileId(value);
            resetResult();
          }}
        />

        <p className="mt-3 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
          {t('agentConversion.sourceUnchanged')}
        </p>
        {displayedError && (
          <div
            role="alert"
            className="mt-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
          >
            {displayedError}
          </div>
        )}

        {preview && <AgentConversionArtifacts preview={preview} />}

        {preview &&
          !preview.plan &&
          !preview.safeSubset &&
          (preview.summary.requiresInput > 0 || preview.summary.conflicts > 0) && (
            <div className="mt-3 rounded-md border border-warning/50 bg-warning/10 p-3">
              <p className="text-xs text-foreground">
                {t('agentConversion.safeSubsetHint', {
                  count: preview.report.residuals.length,
                })}
              </p>
              <Button
                type="button"
                variant="outline"
                className="mt-2"
                disabled={busy}
                onClick={() => {
                  setSafeSubset(true);
                  void runPreview(true);
                }}
              >
                {t('agentConversion.previewSafeSubset')}
              </Button>
            </div>
          )}

        {activeOperation?.report && <AgentConversionReport report={activeOperation.report} />}
      </Dialog>
      <AgentConversionRiskDialog
        open={dangerConfirmOpen}
        projectPath={scope === 'project' ? activeProjectPath : null}
        targetPaths={[
          ...new Set(
            preview?.artifacts
              .filter((artifact) => artifact.risk === 'dangerous')
              .flatMap((artifact) => (artifact.target ? [artifact.target.location.path] : [])) ??
              [],
          ),
        ]}
        busy={busy}
        onCancel={() => setDangerConfirmOpen(false)}
        onConfirm={() => void submitConversion()}
      />
    </>
  );
}

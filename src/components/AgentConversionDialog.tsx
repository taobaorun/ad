import { type ComponentPropsWithoutRef, useEffect, useMemo, useRef, useState } from 'react';
import { ArrowRightLeft, ChevronDown, RotateCcw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import {
  AgentContextSchema,
  type AgentInstallation,
  type ConversionProgressEvent,
  type ConversionRoutePreview,
  type OperationReceipt,
  type PlanAcknowledgement,
} from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';
import { useAgents } from '@/store/agents';
import { useProjects } from '@/store/projects';
import { useUiState } from '@/store/ui';

import { Button } from './ui/button';
import { Dialog } from './ui/dialog';
import { AgentConversionArtifacts } from './AgentConversionArtifacts';
import {
  AgentConversionProgress,
  type ConversionOperationProgress,
} from './AgentConversionProgress';
import { AgentConversionRiskDialog } from './AgentConversionRiskDialog';

const CLAUDE_TO_CODEX_ROUTE = {
  sourceAgentId: 'claude-code',
  targetAgentId: 'codex',
} as const;

type ConversionScope = 'user' | 'project';
type PermissionPreset = '' | 'on_request_workspace_write' | 'never_danger_full_access';
type BusyOperation = 'preview' | 'apply' | 'rollback' | null;

function receiptCanRollback(receipt: OperationReceipt | null): boolean {
  return receipt?.status === 'complete' || receipt?.status === 'partial_failure';
}

function notifyAgentWorkspaceChanged() {
  window.dispatchEvent(new Event('ad:project-codex-runtime-changed'));
  window.dispatchEvent(new Event('ad:agent-workspace-changed'));
}

export function AgentConversionButton() {
  const { t } = useTranslation();
  const installations = useAgents((state) => state.installations);
  const activeProjectPath = useUiState((state) => state.activeProjectPath);
  const projects = useProjects((state) => state.projects);
  const [open, setOpen] = useState(false);
  const [preferProjectScope, setPreferProjectScope] = useState(false);
  const activeProject = projects.find((project) => project.path === activeProjectPath);
  const sourceInstallations = installations.filter(
    (installation) => installation.agentId === CLAUDE_TO_CODEX_ROUTE.sourceAgentId,
  );
  const targetInstallations = installations.filter(
    (installation) => installation.agentId === CLAUDE_TO_CODEX_ROUTE.targetAgentId,
  );

  useEffect(() => {
    const openForProject = () => {
      setPreferProjectScope(true);
      setOpen(true);
    };
    window.addEventListener('ad:open-project-conversion', openForProject);
    return () => window.removeEventListener('ad:open-project-conversion', openForProject);
  }, []);

  if (sourceInstallations.length === 0 || targetInstallations.length === 0) return null;

  return (
    <>
      <button
        type="button"
        onClick={() => {
          setPreferProjectScope(false);
          setOpen(true);
        }}
        className="inline-flex h-7 items-center gap-1.5 rounded-md border border-border px-2 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
        aria-label={t('agentConversion.open')}
        title={t('agentConversion.open')}
      >
        <ArrowRightLeft className="h-3.5 w-3.5" aria-hidden="true" />
        {t('agentConversion.shortLabel')}
      </button>
      <AgentConversionDialog
        open={open}
        onOpenChange={setOpen}
        sourceInstallations={sourceInstallations}
        targetInstallations={targetInstallations}
        activeProjectPath={activeProjectPath}
        inheritBaseConfig={activeProject?.inheritBaseConfig ?? true}
        preferProjectScope={preferProjectScope}
      />
    </>
  );
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

function AgentConversionDialog({
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
  const [confirmedSkillIds, setConfirmedSkillIds] = useState<string[]>([]);
  const [preview, setPreview] = useState<ConversionRoutePreview | null>(null);
  const [receipt, setReceipt] = useState<OperationReceipt | null>(null);
  const [busyOperation, setBusyOperation] = useState<BusyOperation>(null);
  const [progress, setProgress] = useState<ConversionOperationProgress | null>(null);
  const [operationStartedAt, setOperationStartedAt] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [dangerConfirmOpen, setDangerConfirmOpen] = useState(false);
  const previewRequestRef = useRef(0);
  const activeProjectPathRef = useRef(activeProjectPath);
  activeProjectPathRef.current = activeProjectPath;
  const busy = busyOperation !== null;
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
    setConfirmedSkillIds([]);
    setPreview(null);
    setReceipt(null);
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
    setReceipt(null);
    setError(null);
    setDangerConfirmOpen(false);
  }

  function resetDecisions() {
    setTargetModel('');
    setPermissionPreset('');
    setProfileId('');
    setConfirmedSkillIds([]);
  }

  async function runPreview(skillIds = confirmedSkillIds) {
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
    setReceipt(null);
    try {
      const [sourceContext, targetContext] =
        scope === 'project' && activeProjectPath
          ? await Promise.all([
              tauri.resolveAgentContext(source.id, activeProjectPath),
              tauri.resolveAgentContext(target.id, activeProjectPath),
            ])
          : [
              AgentContextSchema.parse({ installationId: source.id }),
              AgentContextSchema.parse({ installationId: target.id }),
            ];
      if (!isCurrentRequest()) return;
      const result = await tauri.previewClaudeToCodexRoute(
        sourceContext,
        targetContext,
        {
          ...(targetModel ? { targetModel } : {}),
          ...(permissionPreset ? { permissionPreset } : {}),
          ...(skillIds.length > 0 ? { confirmedSkillIds: skillIds } : {}),
          ...(scope === 'project' && profileId ? { profileId } : {}),
          ...(scope === 'project' ? { inheritBaseConfig } : {}),
        },
        (event: ConversionProgressEvent) => {
          if (isCurrentRequest()) setProgress(event);
        },
      );
      if (!isCurrentRequest()) return;
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
    if (!preview?.plan) return;
    const requestedScope = scope;
    const requestedProjectPath = activeProjectPath;
    const isCurrentApply = () =>
      requestedScope !== 'project' || requestedProjectPath === activeProjectPathRef.current;
    setDangerConfirmOpen(false);
    setBusyOperation('apply');
    setOperationStartedAt(Date.now());
    setProgress({ phase: 'applying', current: 0 });
    setError(null);
    try {
      const acknowledgements: PlanAcknowledgement[] = preview.plan.requiredAcknowledgements.map(
        (requirement) => ({
          code: requirement.code,
          accepted: true,
        }),
      );
      const result = await tauri.applyConversionPlan(preview.plan.id, acknowledgements);
      if (isCurrentApply()) setReceipt(result);
      if (result.status !== 'compensated') {
        notifyAgentWorkspaceChanged();
      }
    } catch (caught) {
      if (isCurrentApply()) setError(formatAgentError(caught));
    } finally {
      setProgress(null);
      setBusyOperation(null);
    }
  }

  async function rollback() {
    if (!receipt || !window.confirm(t('agentConversion.rollbackConfirm'))) return;
    setBusyOperation('rollback');
    setError(null);
    try {
      await tauri.rollbackAgentReceipt(receipt.id, true);
      setReceipt(null);
      setPreview(null);
      notifyAgentWorkspaceChanged();
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusyOperation(null);
    }
  }

  function confirmSkill(logicalId: string) {
    const next = [...new Set([...confirmedSkillIds, logicalId])];
    setConfirmedSkillIds(next);
    void runPreview(next);
  }

  const showInstallationControls =
    sourceInstallations.length > 1 || eligibleTargetInstallations.length > 1;

  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(nextOpen) => {
          if (!busy && !dangerConfirmOpen) onOpenChange(nextOpen);
        }}
        title={t('agentConversion.title')}
        description={t('agentConversion.description')}
        size="lg"
        closeDisabled={busy}
        footer={
          <div data-testid="conversion-sticky-footer" className="[&>[role=status]]:mt-0">
            {progress && (
              <AgentConversionProgress progress={progress} startedAt={operationStartedAt} />
            )}
            <div className={`flex items-center justify-between gap-3 ${progress ? 'mt-3' : ''}`}>
              <Button
                type="button"
                variant="ghost"
                onClick={() => onOpenChange(false)}
                disabled={busy}
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
                {preview?.plan && !receipt && (
                  <Button type="button" onClick={() => void applyConversion()} disabled={busy}>
                    {busyOperation === 'apply'
                      ? t('agentConversion.applying')
                      : t('agentConversion.apply')}
                  </Button>
                )}
              </div>
            </div>
          </div>
        }
      >
        <div>
          <label
            htmlFor="conversion-scope"
            className="mb-1 block text-xs font-medium text-muted-foreground"
          >
            {t('agentConversion.scope')}
          </label>
          <ConversionSelect
            id="conversion-scope"
            value={scope}
            disabled={busy}
            onChange={(event) => {
              setScope(event.target.value as ConversionScope);
              resetDecisions();
              resetResult();
            }}
          >
            <option value="user">{t('agentConversion.scopeUser')}</option>
            <option value="project" disabled={!activeProjectPath}>
              {t('agentConversion.scopeProject')}
            </option>
          </ConversionSelect>
          <p className="mt-1 text-xs text-muted-foreground">
            {scope === 'project' && activeProjectPath ? (
              <>
                {t('agentConversion.scopeProjectHint')}
                <span className="ml-1 break-all font-mono text-foreground">
                  {activeProjectPath}
                </span>
              </>
            ) : (
              t('agentConversion.scopeUserHint')
            )}
          </p>
          {scope === 'project' && activeProjectPath && (
            <p className="mt-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
              {t('agentConversion.projectRuntimeHint')}
            </p>
          )}
        </div>

        {showInstallationControls && (
          <details className="mt-3 rounded-md border border-border px-3 py-2">
            <summary className="cursor-pointer text-xs font-medium">
              {t('agentConversion.advancedInstances')}
            </summary>
            <p className="mt-2 text-xs text-muted-foreground">
              {t('agentConversion.instanceHint')}
            </p>
            <div className="mt-2 grid gap-3 sm:grid-cols-2">
              <InstallationSelect
                id="conversion-source"
                label={t('agentConversion.sourceInstance')}
                installations={sourceInstallations}
                value={sourceId}
                disabled={busy}
                onChange={(id) => {
                  setSourceId(id);
                  resetDecisions();
                  resetResult();
                }}
              />
              <InstallationSelect
                id="conversion-target"
                label={t('agentConversion.targetInstance')}
                installations={eligibleTargetInstallations}
                value={targetId}
                disabled={busy}
                onChange={(id) => {
                  setTargetId(id);
                  resetDecisions();
                  resetResult();
                }}
              />
            </div>
          </details>
        )}

        <div className="mt-3 rounded-md border border-border p-3">
          <h3 className="text-xs font-semibold">{t('agentConversion.decisions')}</h3>
          <div className="mt-2 grid gap-3 sm:grid-cols-2">
            <div>
              <label
                htmlFor="conversion-model"
                className="mb-1 block text-xs font-medium text-muted-foreground"
              >
                {t('agentConversion.codexModel')}
              </label>
              <input
                id="conversion-model"
                value={targetModel}
                disabled={busy}
                onChange={(event) => {
                  setTargetModel(event.target.value);
                  resetResult();
                }}
                placeholder={t('agentConversion.codexModelPlaceholder')}
                className="h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-sm"
              />
              <p className="mt-1 text-xs text-muted-foreground">
                {t('agentConversion.codexModelHint')}
              </p>
            </div>
            <div>
              <label
                htmlFor="conversion-permissions"
                className="mb-1 block text-xs font-medium text-muted-foreground"
              >
                {t('agentConversion.codexPermissions')}
              </label>
              <ConversionSelect
                id="conversion-permissions"
                value={permissionPreset}
                disabled={busy}
                onChange={(event) => {
                  setPermissionPreset(event.target.value as PermissionPreset);
                  resetResult();
                }}
              >
                <option value="">{t('agentConversion.permissionsPreserve')}</option>
                <option value="on_request_workspace_write">
                  {t('agentConversion.permissionsSafe')}
                </option>
                <option value="never_danger_full_access">
                  {t('agentConversion.permissionsBypass')}
                </option>
              </ConversionSelect>
              <p
                className={`mt-1 text-xs ${
                  permissionPreset === 'never_danger_full_access'
                    ? 'text-destructive'
                    : 'text-muted-foreground'
                }`}
              >
                {permissionPreset === 'never_danger_full_access'
                  ? t('agentConversion.permissionsDangerHint')
                  : t('agentConversion.permissionsHint')}
              </p>
            </div>
            {scope === 'project' && (
              <div className="sm:col-span-2">
                <label
                  htmlFor="conversion-profile"
                  className="mb-1 block text-xs font-medium text-muted-foreground"
                >
                  {t('agentConversion.profileId')}
                </label>
                <input
                  id="conversion-profile"
                  value={profileId}
                  disabled={busy || !inheritBaseConfig}
                  onChange={(event) => {
                    setProfileId(event.target.value);
                    resetResult();
                  }}
                  placeholder={t('agentConversion.profileIdPlaceholder')}
                  className="h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-sm"
                />
                <p className="mt-1 text-xs text-muted-foreground">
                  {inheritBaseConfig
                    ? t('agentConversion.profileIdHint')
                    : t('agentConversion.profileDisabledHint')}
                </p>
              </div>
            )}
          </div>
        </div>

        <p className="mt-3 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
          {t('agentConversion.sourceUnchanged')}
        </p>
        {error && (
          <div
            role="alert"
            className="mt-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
          >
            {error}
          </div>
        )}

        {preview && (
          <AgentConversionArtifacts
            preview={preview}
            confirmedSkillIds={confirmedSkillIds}
            busy={busy}
            onConfirmSkill={confirmSkill}
          />
        )}

        {receipt && (
          <div
            role={receipt.status === 'complete' ? 'status' : 'alert'}
            className={`mt-4 rounded-md border p-3 text-sm ${
              receipt.status === 'complete'
                ? 'border-success/40 bg-success/10 text-foreground'
                : 'border-destructive/40 bg-destructive/10 text-destructive'
            }`}
          >
            <div className="font-medium">
              {receipt.status === 'complete'
                ? t('agentConversion.applied')
                : receipt.status === 'compensated'
                  ? t('agentConversion.compensated')
                  : t('agentConversion.partialFailure')}
            </div>
            {receipt.message && <div className="mt-1 text-xs">{receipt.message}</div>}
            <div className="mt-1 text-xs">
              {t('agentConversion.backupCount', { count: receipt.backupPaths.length })}
            </div>
          </div>
        )}
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

interface InstallationSelectProps {
  id: string;
  label: string;
  installations: AgentInstallation[];
  value: AgentInstallation['id'] | null;
  disabled: boolean;
  onChange: (id: AgentInstallation['id']) => void;
}

type ConversionSelectProps = Omit<ComponentPropsWithoutRef<'select'>, 'className'>;

function ConversionSelect({ children, disabled, ...props }: ConversionSelectProps) {
  return (
    <span className="relative block">
      <select
        {...props}
        disabled={disabled}
        className="h-9 w-full appearance-none rounded-md border border-input bg-background bg-none px-2 pr-8 text-sm disabled:cursor-not-allowed disabled:opacity-50"
      >
        {children}
      </select>
      <ChevronDown
        aria-hidden="true"
        className={`pointer-events-none absolute right-2 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground ${
          disabled ? 'opacity-50' : ''
        }`}
      />
    </span>
  );
}

function InstallationSelect({
  id,
  label,
  installations,
  value,
  disabled,
  onChange,
}: InstallationSelectProps) {
  return (
    <div>
      <label htmlFor={id} className="mb-1 block text-xs font-medium text-muted-foreground">
        {label}
      </label>
      <ConversionSelect
        id={id}
        value={value ?? ''}
        disabled={disabled}
        onChange={(event) => {
          const selected = installations.find(
            (installation) => installation.id === event.target.value,
          );
          if (selected) onChange(selected.id);
        }}
      >
        {installations.map((installation) => (
          <option key={installation.id} value={installation.id}>
            {installation.rootPath}
          </option>
        ))}
      </ConversionSelect>
    </div>
  );
}

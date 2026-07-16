import { useEffect, useMemo, useState } from 'react';
import { ArrowRightLeft, RotateCcw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import {
  AgentContextSchema,
  type AgentInstallation,
  type ConversionRoutePreview,
  type OperationReceipt,
  type PlanAcknowledgement,
} from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';
import { useAgents } from '@/store/agents';
import { useUiState } from '@/store/ui';

import { Button } from './ui/button';
import { Dialog } from './ui/dialog';
import { AgentConversionArtifacts } from './AgentConversionArtifacts';
import { AgentConversionRiskDialog } from './AgentConversionRiskDialog';

const CLAUDE_TO_CODEX_ROUTE = {
  sourceAgentId: 'claude-code',
  targetAgentId: 'codex',
} as const;

type ConversionScope = 'user' | 'project';
type PermissionPreset = '' | 'on_request_workspace_write' | 'never_danger_full_access';

export function AgentConversionButton() {
  const { t } = useTranslation();
  const installations = useAgents((state) => state.installations);
  const activeProjectPath = useUiState((state) => state.activeProjectPath);
  const [open, setOpen] = useState(false);
  const sourceInstallations = installations.filter(
    (installation) => installation.agentId === CLAUDE_TO_CODEX_ROUTE.sourceAgentId,
  );
  const targetInstallations = installations.filter(
    (installation) => installation.agentId === CLAUDE_TO_CODEX_ROUTE.targetAgentId,
  );

  if (sourceInstallations.length === 0 || targetInstallations.length === 0) return null;

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
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
}

function AgentConversionDialog({
  open,
  onOpenChange,
  sourceInstallations,
  targetInstallations,
  activeProjectPath,
}: AgentConversionDialogProps) {
  const { t } = useTranslation();
  const [sourceId, setSourceId] = useState(sourceInstallations[0]?.id ?? null);
  const [targetId, setTargetId] = useState(targetInstallations[0]?.id ?? null);
  const [scope, setScope] = useState<ConversionScope>('user');
  const [targetModel, setTargetModel] = useState('');
  const [permissionPreset, setPermissionPreset] = useState<PermissionPreset>('');
  const [confirmedSkillIds, setConfirmedSkillIds] = useState<string[]>([]);
  const [preview, setPreview] = useState<ConversionRoutePreview | null>(null);
  const [receipt, setReceipt] = useState<OperationReceipt | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dangerConfirmOpen, setDangerConfirmOpen] = useState(false);

  useEffect(() => {
    if (!sourceInstallations.some((installation) => installation.id === sourceId)) {
      setSourceId(sourceInstallations[0]?.id ?? null);
    }
  }, [sourceId, sourceInstallations]);
  useEffect(() => {
    if (!targetInstallations.some((installation) => installation.id === targetId)) {
      setTargetId(targetInstallations[0]?.id ?? null);
    }
  }, [targetId, targetInstallations]);
  useEffect(() => {
    if (!activeProjectPath && scope === 'project') setScope('user');
    setTargetModel('');
    setPermissionPreset('');
    setConfirmedSkillIds([]);
    setPreview(null);
    setReceipt(null);
    setError(null);
  }, [activeProjectPath, scope]);

  const source = useMemo(
    () => sourceInstallations.find((installation) => installation.id === sourceId),
    [sourceId, sourceInstallations],
  );
  const target = useMemo(
    () => targetInstallations.find((installation) => installation.id === targetId),
    [targetId, targetInstallations],
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
    setConfirmedSkillIds([]);
  }

  async function runPreview(skillIds = confirmedSkillIds) {
    if (!source || !target) return;
    setBusy(true);
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
      const result = await tauri.previewClaudeToCodexRoute(sourceContext, targetContext, {
        ...(targetModel ? { targetModel } : {}),
        ...(permissionPreset ? { permissionPreset } : {}),
        ...(skillIds.length > 0 ? { confirmedSkillIds: skillIds } : {}),
      });
      setPreview(result);
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusy(false);
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
    setBusy(true);
    setError(null);
    try {
      const acknowledgements: PlanAcknowledgement[] = preview.plan.requiredAcknowledgements.map(
        (requirement) => ({
          code: requirement.code,
          accepted: true,
        }),
      );
      const result = await tauri.applyConversionPlan(preview.plan.id, acknowledgements);
      setReceipt(result);
      setDangerConfirmOpen(false);
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusy(false);
    }
  }

  async function rollback() {
    if (!receipt || !window.confirm(t('agentConversion.rollbackConfirm'))) return;
    setBusy(true);
    setError(null);
    try {
      await tauri.rollbackAgentReceipt(receipt.id, true);
      setReceipt(null);
      setPreview(null);
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusy(false);
    }
  }

  function confirmSkill(logicalId: string) {
    const next = [...new Set([...confirmedSkillIds, logicalId])];
    setConfirmedSkillIds(next);
    void runPreview(next);
  }

  const showInstallationControls = sourceInstallations.length > 1 || targetInstallations.length > 1;

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
        footer={
          <div className="flex items-center justify-between gap-3">
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={busy}
            >
              {t('agentConversion.close')}
            </Button>
            <div className="flex gap-2">
              {receipt && (
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
                {busy && !preview ? t('agentConversion.previewing') : t('agentConversion.preview')}
              </Button>
              {preview?.plan && !receipt && (
                <Button type="button" onClick={() => void applyConversion()} disabled={busy}>
                  {busy ? t('agentConversion.applying') : t('agentConversion.apply')}
                </Button>
              )}
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
          <select
            id="conversion-scope"
            value={scope}
            disabled={busy}
            onChange={(event) => {
              setScope(event.target.value as ConversionScope);
              resetDecisions();
              resetResult();
            }}
            className="h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
          >
            <option value="user">{t('agentConversion.scopeUser')}</option>
            <option value="project" disabled={!activeProjectPath}>
              {t('agentConversion.scopeProject')}
            </option>
          </select>
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
                installations={targetInstallations}
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
              <select
                id="conversion-permissions"
                value={permissionPreset}
                disabled={busy}
                onChange={(event) => {
                  setPermissionPreset(event.target.value as PermissionPreset);
                  resetResult();
                }}
                className="h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
              >
                <option value="">{t('agentConversion.permissionsPreserve')}</option>
                <option value="on_request_workspace_write">
                  {t('agentConversion.permissionsSafe')}
                </option>
                <option value="never_danger_full_access">
                  {t('agentConversion.permissionsBypass')}
                </option>
              </select>
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
            role="status"
            className="mt-4 rounded-md border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm text-emerald-800 dark:text-emerald-200"
          >
            <div className="font-medium">{t('agentConversion.applied')}</div>
            <div className="mt-1 text-xs">
              {t('agentConversion.backupCount', { count: receipt.backupPaths.length })}
            </div>
          </div>
        )}
      </Dialog>
      <AgentConversionRiskDialog
        open={dangerConfirmOpen}
        projectPath={scope === 'project' ? activeProjectPath : null}
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
      <select
        id={id}
        value={value ?? ''}
        disabled={disabled}
        onChange={(event) => {
          const selected = installations.find(
            (installation) => installation.id === event.target.value,
          );
          if (selected) onChange(selected.id);
        }}
        className="h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
      >
        {installations.map((installation) => (
          <option key={installation.id} value={installation.id}>
            {installation.rootPath}
          </option>
        ))}
      </select>
    </div>
  );
}

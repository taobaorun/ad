import { useEffect, useMemo, useState } from 'react';
import { ArrowRightLeft, RotateCcw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { formatAgentError } from '@/lib/agentErrors';
import {
  AgentContextSchema,
  type AgentInstallation,
  type ConversionRoutePreview,
  type OperationReceipt,
} from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';
import { useAgents } from '@/store/agents';

import { Button } from './ui/button';
import { Dialog } from './ui/dialog';

const CLAUDE_TO_CODEX_ROUTE = {
  sourceAgentId: 'claude-code',
  targetAgentId: 'codex',
} as const;

export function AgentConversionButton() {
  const { t } = useTranslation();
  const installations = useAgents((state) => state.installations);
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
      />
    </>
  );
}

interface AgentConversionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  sourceInstallations: AgentInstallation[];
  targetInstallations: AgentInstallation[];
}

function AgentConversionDialog({
  open,
  onOpenChange,
  sourceInstallations,
  targetInstallations,
}: AgentConversionDialogProps) {
  const { t } = useTranslation();
  const [sourceId, setSourceId] = useState(sourceInstallations[0]?.id ?? null);
  const [targetId, setTargetId] = useState(targetInstallations[0]?.id ?? null);
  const [preview, setPreview] = useState<ConversionRoutePreview | null>(null);
  const [receipt, setReceipt] = useState<OperationReceipt | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
  }

  async function runPreview() {
    if (!source || !target) return;
    setBusy(true);
    setError(null);
    setReceipt(null);
    try {
      const result = await tauri.previewClaudeToCodexRoute(
        AgentContextSchema.parse({ installationId: source.id }),
        AgentContextSchema.parse({ installationId: target.id }),
      );
      setPreview(result);
    } catch (caught) {
      setError(formatAgentError(caught));
    } finally {
      setBusy(false);
    }
  }

  async function applyConversion() {
    if (!preview?.plan) return;
    setBusy(true);
    setError(null);
    try {
      const result = await tauri.applyConversionPlan(preview.plan.id, true);
      setReceipt(result);
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

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!busy) onOpenChange(nextOpen);
      }}
      title={t('agentConversion.title')}
      description={t('agentConversion.description')}
      size="lg"
      footer={
        <div className="flex items-center justify-between gap-3">
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
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
      <div className="grid gap-3 sm:grid-cols-2">
        <InstallationSelect
          id="conversion-source"
          label={t('agentConversion.source')}
          installations={sourceInstallations}
          value={sourceId}
          onChange={(id) => {
            setSourceId(id);
            resetResult();
          }}
        />
        <InstallationSelect
          id="conversion-target"
          label={t('agentConversion.target')}
          installations={targetInstallations}
          value={targetId}
          onChange={(id) => {
            setTargetId(id);
            resetResult();
          }}
        />
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
        <div className="mt-4">
          <h3 className="text-sm font-semibold">{t('agentConversion.artifacts')}</h3>
          <ul className="mt-2 divide-y divide-border overflow-hidden rounded-lg border border-border">
            {preview.artifacts.map((artifact) => (
              <li key={artifact.id} className="flex items-start justify-between gap-4 px-3 py-2.5">
                <div className="min-w-0">
                  <div className="truncate font-mono text-xs">{artifact.id}</div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    {artifact.kind} · {artifact.source.logicalId}
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">{artifact.message}</div>
                </div>
                <span className="shrink-0 rounded-full border border-border px-2 py-0.5 text-xs">
                  {t(`agentConversion.disposition.${artifact.disposition}`)}
                </span>
              </li>
            ))}
          </ul>
          {!preview.plan && (
            <p role="status" className="mt-3 text-sm text-muted-foreground">
              {t('agentConversion.noChanges')}
            </p>
          )}
        </div>
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
  );
}

interface InstallationSelectProps {
  id: string;
  label: string;
  installations: AgentInstallation[];
  value: AgentInstallation['id'] | null;
  onChange: (id: AgentInstallation['id']) => void;
}

function InstallationSelect({
  id,
  label,
  installations,
  value,
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

import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { tauri, type ActivationLogEntry } from '@/lib/tauri';
import type { OperationHistoryEntry } from '@/lib/agentTypes';
import { formatAgentError } from '@/lib/agentErrors';
import { profileFeaturesFor } from '@/lib/profileEditorRegistry';
import { Button } from './ui/button';
import { AlertTriangle, RotateCcw, RefreshCw } from 'lucide-react';
import { useAgents } from '@/store/agents';
import { useProfiles } from '@/store/profiles';
import { useUiState } from '@/store/ui';

export function HistoryPanel() {
  const { t } = useTranslation();
  const activeAgentId = useAgents((state) => state.activeAgentId);
  const activeContext = useAgents((state) => state.activeContext);
  const installations = useAgents((state) => state.installations);
  const activeProjectPath = useUiState((state) => state.activeProjectPath);
  const [operations, setOperations] = useState<OperationHistoryEntry[]>([]);
  const [activationEntries, setActivationEntries] = useState<ActivationLogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const loadAll = useProfiles((s) => s.loadAll);
  const showLegacyHistory = profileFeaturesFor(activeAgentId).legacyProjectTemplates;
  const historyInstallationId = useMemo(() => {
    if (!activeContext) return undefined;
    const activeInstallation = installations.find(
      (installation) => installation.id === activeContext.installationId,
    );
    return activeInstallation?.baseInstallationId ?? activeContext.installationId;
  }, [activeContext, installations]);
  const historyProjectPath =
    activeAgentId === 'codex' ? (activeProjectPath ?? undefined) : undefined;

  const refresh = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const [operationEntries, legacyEntries] = await Promise.all([
        tauri.listAgentOperationHistory(historyInstallationId, 50, historyProjectPath),
        showLegacyHistory ? tauri.readHistory(50) : Promise.resolve([]),
      ]);
      setOperations(operationEntries);
      setActivationEntries(legacyEntries);
    } catch (e) {
      setError(formatAgentError(e));
    } finally {
      setBusy(false);
    }
  }, [historyInstallationId, historyProjectPath, showLegacyHistory]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function onRollback(entry: OperationHistoryEntry) {
    if (!entry.receipt || !entry.receipt.rollback?.available) return;
    if (!window.confirm(t('history.operationRollbackConfirm'))) return;
    setBusy(true);
    setError(null);
    try {
      await tauri.rollbackAgentReceipt(entry.receipt.id, true);
      window.dispatchEvent(new Event('ad:project-codex-runtime-changed'));
      window.dispatchEvent(new Event('ad:agent-workspace-changed'));
      await refresh();
    } catch (e) {
      setError(formatAgentError(e));
      setBusy(false);
    }
  }

  async function onRestore(backup: string | null | undefined) {
    if (!backup) return;
    if (!window.confirm(t('history.restoreConfirm'))) return;
    setBusy(true);
    try {
      await tauri.restoreBackup(backup);
      await loadAll();
      await refresh();
    } catch (e) {
      setError(formatAgentError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-border p-3">
        <h2 className="text-sm font-semibold">{t('history.title')}</h2>
        <Button variant="ghost" size="sm" onClick={() => void refresh()} disabled={busy}>
          <RefreshCw className="h-4 w-4" />
          {t('history.refresh')}
        </Button>
      </div>
      {error && (
        <div className="bg-destructive/10 px-3 py-1.5 text-xs text-destructive">{error}</div>
      )}
      <div className="flex-1 space-y-5 overflow-y-auto p-3">
        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            {t('history.operationTitle')}
          </h3>
          {operations.length === 0 ? (
            <div className="text-sm text-muted-foreground">{t('history.operationEmpty')}</div>
          ) : (
            <ul className="space-y-1">
              {operations.map((entry) => {
                const receipt = entry.receipt;
                if (!receipt) {
                  const diagnostic = entry.diagnostic;
                  return (
                    <li
                      key={`diagnostic:${diagnostic?.source ?? entry.createdAt}`}
                      className="flex items-center gap-3 rounded border border-warning/40 bg-warning/5 px-3 py-2 text-sm"
                      title={diagnostic?.message}
                    >
                      <AlertTriangle className="h-4 w-4 shrink-0 text-warning" />
                      <span className="font-mono text-xs text-muted-foreground">
                        {new Date(entry.createdAt).toLocaleString()}
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="font-medium">
                          {t(`history.diagnostic.${diagnostic?.code ?? 'unreadable'}`)}
                        </div>
                        <div className="truncate text-xs text-muted-foreground">
                          {diagnostic?.source}
                        </div>
                      </div>
                    </li>
                  );
                }
                return (
                  <li
                    key={receipt.id}
                    className="flex items-center gap-3 rounded border border-border bg-card px-3 py-2 text-sm"
                  >
                    <span className="font-mono text-xs text-muted-foreground">
                      {new Date(entry.createdAt).toLocaleString()}
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="font-medium">
                        {receipt.appliedResources.map((resource) => resource.logicalId).join(', ') ||
                          t('history.noResources')}
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {t(`history.status.${receipt.status}`)}
                        {receipt.appliedResources.length > 0 &&
                          ` · ${receipt.appliedResources
                            .map((resource) => `${resource.kind}/${resource.scope}`)
                            .join(', ')}`}
                      </div>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={busy || !receipt.rollback?.available}
                      onClick={() => void onRollback(entry)}
                    >
                      <RotateCcw className="h-3.5 w-3.5" />
                      {t('history.rollback')}
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}
        </section>

        {showLegacyHistory && (
          <section>
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              {t('history.activationTitle')}
            </h3>
            {activationEntries.length === 0 ? (
              <div className="text-sm text-muted-foreground">{t('history.activationEmpty')}</div>
            ) : (
              <ul className="space-y-1">
                {activationEntries.map((e, idx) => (
                  <li
                    key={idx}
                    className="flex items-center gap-3 rounded border border-border bg-card px-3 py-2 text-sm"
                  >
                    <span className="font-mono text-xs text-muted-foreground">
                      {new Date(e.ts).toLocaleString()}
                    </span>
                    <span className="flex-1">
                      <span className="text-muted-foreground">{e.from ?? '—'}</span>
                      <span className="mx-2">→</span>
                      <span className="font-medium">{e.to}</span>
                    </span>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={!e.backupPath || busy}
                      onClick={() => void onRestore(e.backupPath)}
                      title={e.backupPath ?? t('history.noBackup')}
                    >
                      <RotateCcw className="h-3.5 w-3.5" />
                      {t('history.restore')}
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </section>
        )}
      </div>
    </div>
  );
}

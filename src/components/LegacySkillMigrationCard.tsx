import { useCallback, useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import { ArchiveRestore, AlertTriangle, CheckCircle2, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { tauri } from '@/lib/tauri';
import type {
  LegacySkillInventory,
  LegacySkillMigrationPlanView,
  LegacySkillMigrationReport,
} from '@/lib/legacySkillMigrationTypes';

export function LegacySkillMigrationCard({ projectPath }: { projectPath: string }) {
  const { t } = useTranslation();
  const [inventory, setInventory] = useState<LegacySkillInventory | null>(null);
  const [plan, setPlan] = useState<LegacySkillMigrationPlanView | null>(null);
  const [report, setReport] = useState<LegacySkillMigrationReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const next = await tauri.inspectLegacySkillState();
    setInventory(next);
  }, []);

  useEffect(() => {
    let current = true;
    void refresh().catch((reason) => {
      if (current) setError(String(reason));
    });
    return () => {
      current = false;
    };
  }, [refresh]);

  const project = useMemo(
    () =>
      inventory?.projects.find(
        (candidate) =>
          candidate.canonicalProjectPath === projectPath || candidate.projectPath === projectPath,
      ) ?? null,
    [inventory, projectPath],
  );
  const archive = useMemo(
    () =>
      inventory?.archives.find(
        (candidate) =>
          candidate.canonicalProjectPath === projectPath || candidate.projectPath === projectPath,
      ) ?? null,
    [inventory, projectPath],
  );

  async function previewMigration() {
    setBusy(true);
    setError(null);
    try {
      setPlan(await tauri.previewLegacyProjectSkillMigration(projectPath));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function closePreview() {
    const current = plan;
    setPlan(null);
    if (current) await tauri.cancelLegacyProjectSkillMigrationPlan(current.id).catch(() => false);
  }

  async function applyMigration() {
    if (!plan) return;
    setBusy(true);
    setError(null);
    try {
      const next = await tauri.applyLegacyProjectSkillMigrationPlan(plan);
      setReport(next);
      setPlan(null);
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function restoreState() {
    if (!archive || archive.status !== 'archived') return;
    setBusy(true);
    setError(null);
    try {
      const next = await tauri.restoreLegacyProjectSkillState(archive.receiptId);
      setReport(next);
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  if (!project && archive?.status !== 'archived' && !report && !error) return null;

  const status = project?.migrationStatus;
  const ready = status === 'ready_to_archive';
  const tone = ready || archive?.status === 'archived' ? 'var(--ds-accent)' : 'var(--ds-warning)';

  return (
    <section
      aria-label={t('legacySkillMigration.title')}
      className="mt-4 rounded-lg px-3.5 py-3"
      style={{
        border: '0.5px solid var(--ds-line-strong)',
        background: 'var(--ds-bg-card)',
      }}
    >
      <div className="flex items-start gap-2.5">
        {ready || archive?.status === 'archived' ? (
          <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" style={{ color: tone }} />
        ) : (
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" style={{ color: tone }} />
        )}
        <div className="min-w-0 flex-1">
          <div className="text-[12.5px] font-semibold" style={{ color: 'var(--ds-fg-1)' }}>
            {t('legacySkillMigration.title')}
          </div>
          <p className="mt-1 text-[11.5px] leading-5" style={{ color: 'var(--ds-fg-3)' }}>
            {archive?.status === 'archived' && !project
              ? t('legacySkillMigration.archived')
              : t(`legacySkillMigration.status.${status ?? 'blocked'}`)}
          </p>
          {report && (
            <div role="status" className="mt-1 text-[11.5px]" style={{ color: tone }}>
              {t(`legacySkillMigration.outcome.${report.outcome}`)}
            </div>
          )}
          {error && (
            <div role="alert" className="mt-1 break-all text-[11.5px] text-destructive">
              {error}
            </div>
          )}
        </div>
        {ready && (
          <ActionButton disabled={busy} onClick={() => void previewMigration()}>
            {busy ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin motion-reduce:animate-none" />
            ) : (
              <ArchiveRestore />
            )}
            {t('legacySkillMigration.review')}
          </ActionButton>
        )}
        {!project && archive?.status === 'archived' && (
          <ActionButton disabled={busy} onClick={() => void restoreState()}>
            {busy ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin motion-reduce:animate-none" />
            ) : (
              <ArchiveRestore />
            )}
            {t('legacySkillMigration.restore')}
          </ActionButton>
        )}
      </div>

      {plan && (
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="legacy-skill-migration-title"
          className="fixed inset-0 z-50 flex items-center justify-center bg-overlay/65 p-6"
        >
          <div
            className="w-full max-w-lg rounded-xl p-5 shadow-xl"
            style={{ background: 'var(--ds-bg-card)', border: '1px solid var(--ds-line-strong)' }}
          >
            <h2 id="legacy-skill-migration-title" className="text-base font-semibold">
              {t('legacySkillMigration.dialog.title')}
            </h2>
            <p className="mt-2 text-sm leading-6" style={{ color: 'var(--ds-fg-3)' }}>
              {t('legacySkillMigration.dialog.description')}
            </p>
            <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-3 gap-y-2 text-xs">
              <dt style={{ color: 'var(--ds-fg-4)' }}>{t('legacySkillMigration.dialog.state')}</dt>
              <dd className="min-w-0 break-all font-mono">{plan.stateId}</dd>
              <dt style={{ color: 'var(--ds-fg-4)' }}>
                {t('legacySkillMigration.dialog.evidence')}
              </dt>
              <dd>
                {t('legacySkillMigration.dialog.receipts', {
                  count: plan.migrationReceiptIds.length,
                })}
              </dd>
            </dl>
            <div className="mt-5 flex justify-end gap-2">
              <button
                type="button"
                className="rounded-md border border-border px-3 py-1.5 text-sm"
                disabled={busy}
                onClick={() => void closePreview()}
              >
                {t('common.cancel')}
              </button>
              <button
                type="button"
                className="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground disabled:opacity-50"
                disabled={busy}
                onClick={() => void applyMigration()}
              >
                {t('legacySkillMigration.dialog.apply')}
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function ActionButton({
  children,
  disabled,
  onClick,
}: {
  children: ReactNode;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium disabled:opacity-50 [&_svg]:h-3.5 [&_svg]:w-3.5"
      style={{ border: '0.5px solid var(--ds-line-strong)', color: 'var(--ds-fg-2)' }}
    >
      {children}
    </button>
  );
}

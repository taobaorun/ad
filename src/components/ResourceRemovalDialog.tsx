import { Dialog } from './ui/dialog';
import { Button } from './ui/button';
import type {
  CatalogResource,
  ResourceRemovalPlan,
  ResourceRemovalProgress,
} from '@/lib/resourceCatalogTypes';
import { useTranslation } from 'react-i18next';

interface Props {
  resource: CatalogResource | null;
  plan: ResourceRemovalPlan | null;
  progress: ResourceRemovalProgress | null;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}

export function ResourceRemovalDialog({
  resource,
  plan,
  progress,
  busy,
  error,
  onCancel,
  onConfirm,
}: Props) {
  const { t } = useTranslation();
  const open = resource !== null;
  const total = progress?.total ?? plan?.installations.length ?? 0;
  const completed = progress?.completed ?? 0;
  const percentage = total === 0 ? (busy ? 20 : 0) : Math.round((completed / total) * 100);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && !busy && onCancel()}
      closeDisabled={busy}
      title={t('resourceCenter.remove.title', { name: resource?.displayName ?? '' })}
      description={t('resourceCenter.remove.description')}
      footer={
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" disabled={busy} onClick={onCancel}>
            {t('common.cancel')}
          </Button>
          <Button
            type="button"
            variant="destructive"
            disabled={busy || !plan}
            data-dialog-initial-focus
            onClick={onConfirm}
          >
            {busy
              ? t('resourceCenter.remove.removing')
              : progress?.phase === 'partial_failure'
                ? t('resourceCenter.remove.retry')
                : t('resourceCenter.remove.confirm')}
          </Button>
        </div>
      }
    >
      {plan && (
        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-3">
            <Impact value={plan.affectedProjectCount} label={t('resourceCenter.remove.projects')} />
            <Impact value={plan.affectedAgentCount} label={t('resourceCenter.remove.agents')} />
          </div>
          {(busy || progress) && (
            <div aria-live="polite" className="space-y-2">
              <div className="flex justify-between text-xs text-muted-foreground">
                <span>{t(`resourceCenter.remove.phase.${progress?.phase ?? 'uninstalling'}`)}</span>
                <span>
                  {completed} / {total}
                </span>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full bg-primary transition-[width]"
                  style={{ width: `${percentage}%` }}
                />
              </div>
            </div>
          )}
          <div className="max-h-52 space-y-1 overflow-y-auto">
            {plan.installations.map((installation) => {
              const current =
                progress?.item?.installationId === installation.installationId
                  ? progress.item
                  : installation;
              return (
                <div
                  key={installation.installationId}
                  className="flex items-center justify-between gap-3 rounded-md bg-muted/50 px-3 py-2 text-xs"
                >
                  <span className="min-w-0 truncate" title={installation.projectPath}>
                    {installation.projectPath?.split('/').at(-1) ??
                      t('resourceCenter.remove.userScope')}
                  </span>
                  <span className="shrink-0 text-muted-foreground">
                    {current.agentId} · {t(`resourceCenter.remove.state.${current.state}`)}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
    </Dialog>
  );
}

function Impact({ value, label }: { value: number; label: string }) {
  return (
    <div className="rounded-lg border border-border p-3">
      <div className="text-xl font-semibold">{value}</div>
      <div className="text-xs text-muted-foreground">{label}</div>
    </div>
  );
}

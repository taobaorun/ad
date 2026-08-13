import { useTranslation } from 'react-i18next';
import type {
  CatalogSource,
  SourceRemovalPlan,
  SourceRemovalProgress,
} from '@/lib/resourceCatalogTypes';
import { Button } from './ui/button';
import { Dialog } from './ui/dialog';

interface Props {
  source: CatalogSource | null;
  plan: SourceRemovalPlan | null;
  progress: SourceRemovalProgress | null;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}

export function SourceRemovalDialog({
  source,
  plan,
  progress,
  busy,
  error,
  onCancel,
  onConfirm,
}: Props) {
  const { t } = useTranslation();
  const completed = progress?.completed ?? 0;
  const total = progress?.total ?? plan?.resources.length ?? 0;
  const percentage = total === 0 ? (busy ? 20 : 0) : Math.round((completed / total) * 100);
  return (
    <Dialog
      open={source !== null}
      onOpenChange={(next) => !next && !busy && onCancel()}
      closeDisabled={busy}
      title={t('resourceCenter.sourceRemove.title', { name: source?.displayName ?? '' })}
      description={t('resourceCenter.sourceRemove.description')}
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
              ? t('resourceCenter.sourceRemove.removing')
              : progress?.phase === 'partial_failure'
                ? t('resourceCenter.sourceRemove.retry')
                : t('resourceCenter.sourceRemove.confirm')}
          </Button>
        </div>
      }
    >
      {plan && (
        <div className="space-y-4">
          <div className="grid grid-cols-3 gap-3">
            <Impact
              value={plan.resources.length}
              label={t('resourceCenter.sourceRemove.resources')}
            />
            <Impact value={plan.affectedProjectCount} label={t('resourceCenter.remove.projects')} />
            <Impact value={plan.affectedAgentCount} label={t('resourceCenter.remove.agents')} />
          </div>
          {(busy || progress) && (
            <div aria-live="polite" className="space-y-2">
              <div className="flex justify-between text-xs text-muted-foreground">
                <span>
                  {t(`resourceCenter.sourceRemove.phase.${progress?.phase ?? 'uninstalling'}`)}
                </span>
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
            {plan.resources.map((resource) => {
              const current =
                progress?.item?.resourceId === resource.resourceId ? progress.item : resource;
              return (
                <div
                  key={resource.resourceId}
                  className="flex items-center justify-between gap-3 rounded-md bg-muted/50 px-3 py-2 text-xs"
                >
                  <span className="min-w-0 truncate">{resource.resourceName}</span>
                  <span className="shrink-0 text-muted-foreground">
                    {t(`resourceCenter.kind.${resource.kind}`)} ·{' '}
                    {t(`resourceCenter.remove.state.${current.state}`)}
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

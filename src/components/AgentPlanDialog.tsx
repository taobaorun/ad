import { useTranslation } from 'react-i18next';

import type { MutationPlanView } from '@/lib/agentTypes';

import { Button } from './ui/button';
import { Dialog } from './ui/dialog';

interface AgentPlanDialogProps {
  plan: MutationPlanView | null;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}

export function AgentPlanDialog({ plan, busy, error, onCancel, onConfirm }: AgentPlanDialogProps) {
  const { t } = useTranslation();

  return (
    <Dialog
      open={plan !== null}
      onOpenChange={(open) => {
        if (!open && !busy) onCancel();
      }}
      title={t('agentPlan.title')}
      description={t('agentPlan.description')}
      size="lg"
      footer={
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" onClick={onCancel} disabled={busy}>
            {t('agentPlan.cancel')}
          </Button>
          <Button type="button" onClick={onConfirm} disabled={busy || plan === null}>
            {busy ? t('agentPlan.applying') : t('agentPlan.apply')}
          </Button>
        </div>
      }
    >
      {error && (
        <div
          role="alert"
          className="mb-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
        >
          {error}
        </div>
      )}
      <ul className="divide-y divide-border" aria-label={t('agentPlan.changes')}>
        {plan?.changes.map((change) => (
          <li
            key={`${change.resource.kind}:${change.resource.scope}:${change.resource.logicalId}`}
            className="py-3"
          >
            <div className="flex items-center justify-between gap-4">
              <code className="min-w-0 truncate text-xs">{change.resource.logicalId}</code>
              <span className="shrink-0 text-xs text-muted-foreground">
                {t(`agentPlan.operation.${change.kind}`)}
              </span>
            </div>
            <div className="mt-1 text-xs text-muted-foreground">
              {change.resource.scope} · {change.resource.kind}
            </div>
          </li>
        ))}
      </ul>
    </Dialog>
  );
}

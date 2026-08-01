import { useTranslation } from 'react-i18next';
import { Activity, Braces, LockKeyhole, ShieldAlert, Target } from 'lucide-react';
import type { ReactElement, ReactNode } from 'react';

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
  const acknowledgements = plan?.requiredAcknowledgements ?? [];
  const changes = plan?.changes ?? [];
  const dangerous = acknowledgements.some((requirement) => requirement.risk === 'dangerous');
  const impacts = Array.from(
    new Map(
      changes
        .flatMap((change) => change.activationImpact ?? [])
        .map((impact) => [`${impact.kind}:${impact.summaryKey}`, impact]),
    ).values(),
  );

  return (
    <Dialog
      open={plan !== null}
      onOpenChange={(open) => {
        if (!open && !busy) onCancel();
      }}
      title={t('agentPlan.title')}
      description={t('agentPlan.description')}
      size="lg"
      closeDisabled={busy}
      footer={
        <div className="flex justify-end gap-2">
          <Button
            type="button"
            variant="outline"
            onClick={onCancel}
            disabled={busy}
            data-dialog-initial-focus
          >
            {t('agentPlan.cancel')}
          </Button>
          <Button
            type="button"
            variant={dangerous ? 'destructive' : 'default'}
            onClick={onConfirm}
            disabled={busy || plan === null}
          >
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
      {plan && (
        <div className="space-y-4 text-sm">
          <section
            aria-labelledby="agent-plan-risk"
            className={`flex gap-3 rounded-md border p-3 ${
              dangerous
                ? 'border-destructive/50 bg-destructive/10'
                : 'border-warning/40 bg-warning/10'
            }`}
          >
            <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
            <div>
              <h3 id="agent-plan-risk" className="font-medium">
                {dangerous ? t('agentPlan.risk.dangerous') : t('agentPlan.risk.confirmation')}
              </h3>
              <p className="mt-1 text-xs text-muted-foreground">
                {dangerous
                  ? t('agentPlan.risk.dangerousDescription')
                  : t('agentPlan.risk.confirmationDescription')}
              </p>
            </div>
          </section>

          <PlanSection icon={<Activity />} title={t('agentPlan.impact')}>
            {impacts.length === 0 ? (
              <p className="text-xs text-muted-foreground">{t('agentPlan.noActivationImpact')}</p>
            ) : (
              <ul className="list-disc space-y-1 pl-4 text-xs">
                {impacts.map((impact) => (
                  <li key={`${impact.kind}:${impact.summaryKey}`}>{t(impact.summaryKey)}</li>
                ))}
              </ul>
            )}
          </PlanSection>

          <PlanSection icon={<LockKeyhole />} title={t('agentPlan.approval')}>
            <p className="text-xs text-muted-foreground">
              {acknowledgements.length > 0
                ? t('agentPlan.approvalRequired', {
                    count: acknowledgements.length,
                  })
                : t('agentPlan.approvalBound')}
            </p>
          </PlanSection>

          <PlanSection icon={<Target />} title={t('agentPlan.targets')}>
            <ul className="divide-y divide-border" aria-label={t('agentPlan.changes')}>
              {changes.map((change) => (
                <li
                  key={`${change.resource.kind}:${change.resource.scope}:${change.resource.logicalId}`}
                  className="py-2 first:pt-0 last:pb-0"
                >
                  <div className="flex items-center justify-between gap-4">
                    <span className="min-w-0 truncate text-xs font-medium">
                      {change.target?.display ?? change.resource.logicalId}
                    </span>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {t(`agentPlan.operation.${change.kind}`)}
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          </PlanSection>

          <details className="rounded-md border border-border p-3">
            <summary className="flex cursor-pointer list-none items-center gap-2 font-medium">
              <Braces className="h-4 w-4" aria-hidden="true" />
              {t('agentPlan.technicalDetails')}
            </summary>
            <ul className="mt-3 space-y-2">
              {changes.map((change) => (
                <li
                  key={`technical:${change.target?.id ?? change.resource.logicalId}`}
                  className="break-all font-mono text-[11px] text-muted-foreground"
                >
                  {change.resource.logicalId} · {change.scope} · {change.resource.kind}
                </li>
              ))}
            </ul>
          </details>
        </div>
      )}
    </Dialog>
  );
}

function PlanSection({
  icon,
  title,
  children,
}: {
  icon: ReactElement;
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="rounded-md border border-border p-3">
      <h3 className="mb-2 flex items-center gap-2 font-medium">
        <span className="[&>svg]:h-4 [&>svg]:w-4" aria-hidden="true">
          {icon}
        </span>
        {title}
      </h3>
      {children}
    </section>
  );
}

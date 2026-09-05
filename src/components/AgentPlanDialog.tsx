import { useTranslation } from 'react-i18next';
import { Activity, Braces, ShieldAlert, Target } from 'lucide-react';
import type { ReactElement, ReactNode } from 'react';

import type { MutationPlanView } from '@/lib/agentTypes';

import { Button } from './ui/button';
import { Dialog } from './ui/dialog';
import { OperationProgress } from './OperationProgress';

export interface AgentPlanProgress {
  phase: 'installing' | 'removing' | 'updating' | 'applying' | 'refreshing';
  startedAt: number;
}

interface AgentPlanDialogProps {
  plan: MutationPlanView | null;
  description?: string;
  busy: boolean;
  error: string | null;
  progress?: AgentPlanProgress | null;
  onCancel: () => void;
  onConfirm: () => void;
}

export function AgentPlanDialog({
  plan,
  description,
  busy,
  error,
  progress = null,
  onCancel,
  onConfirm,
}: AgentPlanDialogProps) {
  const { t } = useTranslation();
  const acknowledgements = plan?.requiredAcknowledgements ?? [];
  const changes = plan?.changes ?? [];
  const dangerous = acknowledgements.some((requirement) => requirement.risk === 'dangerous');
  const userScope = Boolean(plan && changes.some((change) => change.scope === 'user'));
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
      description={description}
      size="lg"
      closeDisabled={busy}
      footer={
        busy && progress ? undefined : (
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
              {busy ? t('agentPlan.applying') : error ? t('agentPlan.retry') : t('agentPlan.apply')}
            </Button>
          </div>
        )
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
      {busy && progress ? (
        <OperationProgress
          label={t(`agentPlan.progress.${progress.phase}`)}
          startedAt={progress.startedAt}
        />
      ) : plan ? (
        <div className="space-y-4 text-sm">
          {dangerous && (
            <section
              aria-labelledby="agent-plan-risk"
              className="flex gap-3 rounded-md border border-destructive/50 bg-destructive/10 p-3"
            >
              <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <div>
                <h3 id="agent-plan-risk" className="font-medium">
                  {t('agentPlan.risk.dangerous')}
                </h3>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t('agentPlan.risk.dangerousDescription')}
                </p>
              </div>
            </section>
          )}

          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
            <span className="font-medium text-foreground">{plan.agentId}</span>
            <span>{t(`agentPlan.context.${userScope ? 'userScope' : 'projectScope'}`)}</span>
            {plan.context.projectPath && (
              <span className="break-all">{plan.context.projectPath}</span>
            )}
          </div>

          {impacts.length > 0 && (
            <PlanSection icon={<Activity />} title={t('agentPlan.impact')}>
              <ul className="list-disc space-y-1 pl-4 text-xs">
                {impacts.map((impact) => (
                  <li key={`${impact.kind}:${impact.summaryKey}`}>{t(impact.summaryKey)}</li>
                ))}
              </ul>
            </PlanSection>
          )}

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
            <p className="mt-3 break-all text-xs text-muted-foreground">
              {t('agentPlan.context.installation')}: {plan.context.installationId}
            </p>
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
      ) : null}
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

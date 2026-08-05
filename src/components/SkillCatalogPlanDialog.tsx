import { useTranslation } from 'react-i18next';
import { ShieldAlert } from 'lucide-react';
import type { SkillCatalogPlanView } from '@/lib/skillCatalogTypes';
import { Button } from './ui/button';
import { Dialog } from './ui/dialog';

interface SkillCatalogPlanDialogProps {
  plan: SkillCatalogPlanView | null;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function SkillCatalogPlanDialog({
  plan,
  busy,
  onCancel,
  onConfirm,
}: SkillCatalogPlanDialogProps) {
  const { t } = useTranslation();
  const planKind = plan?.rollbackOf ? 'rollback' : plan?.action;
  const payload = plan?.binding ?? plan?.artifact ?? plan?.currentBinding ?? plan?.currentArtifact;
  const impact = payload?.activationImpact;
  const executableCount = impact
    ? impact.hooks.length +
      impact.mcp.length +
      impact.commands.length +
      impact.scripts.length +
      impact.binaries.length
    : 0;
  return (
    <Dialog
      open={plan !== null}
      onOpenChange={(open) => !open && !busy && onCancel()}
      closeDisabled={busy}
      title={planKind ? t(`settings.skills.plan.${planKind}.title`) : undefined}
      description={planKind ? t(`settings.skills.plan.${planKind}.description`) : undefined}
      footer={
        <div className="flex justify-end gap-2">
          <Button type="button" size="sm" variant="outline" disabled={busy} onClick={onCancel}>
            {t('common.cancel')}
          </Button>
          <Button
            type="button"
            size="sm"
            variant={plan?.action === 'remove' ? 'destructive' : 'default'}
            disabled={busy || !plan || plan.applicability === 'blocked'}
            onClick={onConfirm}
          >
            {busy ? t('settings.skills.plan.applying') : t('settings.skills.plan.confirm')}
          </Button>
        </div>
      }
    >
      {plan && (
        <div className="space-y-3 text-xs">
          <div className="rounded-md border border-border bg-muted/30 p-3">
            <div className="font-medium">{plan.displayName}</div>
            <div className="mt-1 break-all font-mono text-[10px] text-muted-foreground">
              {plan.sourceId}
            </div>
          </div>
          {payload && (
            <div>
              <div className="font-medium">{t('settings.skills.plan.sourceBinding')}</div>
              <div className="mt-1 font-mono text-[10px] text-muted-foreground">
                {'stableRoot' in payload ? payload.stableRoot : payload.artifactId}
              </div>
              <div className="mt-1 text-muted-foreground">
                {t('settings.skills.plan.skills', { count: payload.skills.length })}
              </div>
            </div>
          )}
          {plan.affectedWorkspaces.length > 0 && (
            <div className="rounded-md border border-primary/30 bg-primary/5 p-3 text-muted-foreground">
              {t('settings.skills.plan.sharedImpact', { count: plan.affectedWorkspaces.length })}
            </div>
          )}
          {plan.blockingIssues.length > 0 && (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3">
              <div className="font-medium">{t('settings.skills.plan.blocked')}</div>
              <ul className="mt-1 list-disc space-y-1 pl-4 text-muted-foreground">
                {plan.blockingIssues.map((issue, index) => (
                  <li key={`${issue.code}:${index}`}>{issue.message}</li>
                ))}
              </ul>
            </div>
          )}
          {executableCount > 0 && (
            <div className="flex gap-2 rounded-md border border-warning/40 bg-warning/10 p-3">
              <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
              <div>
                <div className="font-medium">{t('settings.skills.plan.executionTitle')}</div>
                <div className="mt-1 text-muted-foreground">
                  {t('settings.skills.plan.executionDescription', { count: executableCount })}
                </div>
              </div>
            </div>
          )}
          <div className="rounded-md border border-primary/30 bg-primary/5 p-3 text-muted-foreground">
            {plan.action === 'remove'
              ? t('settings.skills.plan.removeReferences')
              : t('settings.skills.plan.sharedSource')}
          </div>
        </div>
      )}
    </Dialog>
  );
}

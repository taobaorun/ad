import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';

import { Button } from './ui/button';

interface AgentConversionRiskDialogProps {
  open: boolean;
  projectPath: string | null;
  targetPaths: string[];
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function AgentConversionRiskDialog({
  open,
  projectPath,
  targetPaths,
  busy,
  onCancel,
  onConfirm,
}: AgentConversionRiskDialogProps) {
  const { t } = useTranslation();
  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busy) {
        event.stopImmediatePropagation();
        onCancel();
      }
    };
    document.addEventListener('keydown', handleKeyDown, true);
    return () => document.removeEventListener('keydown', handleKeyDown, true);
  }, [busy, onCancel, open]);
  if (!open) return null;

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-overlay/70 p-4">
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="dangerous-conversion-title"
        aria-describedby="dangerous-conversion-description"
        className="w-full max-w-md rounded-lg border border-destructive/50 bg-background p-5 shadow-xl"
      >
        <h2 id="dangerous-conversion-title" className="text-lg font-semibold text-destructive">
          {t('agentConversion.dangerDialog.title')}
        </h2>
        <p id="dangerous-conversion-description" className="mt-2 text-sm text-muted-foreground">
          {t('agentConversion.dangerDialog.description')}
        </p>
        {projectPath && (
          <p className="mt-3 break-all rounded bg-muted px-3 py-2 font-mono text-xs">
            {projectPath}
          </p>
        )}
        <pre className="mt-3 overflow-x-auto rounded-md border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive">
          {'approval_policy = "never"\nsandbox_mode = "danger-full-access"'}
        </pre>
        <div className="mt-3">
          <div className="text-xs font-medium">{t('agentConversion.dangerDialog.targets')}</div>
          <ul className="mt-1 space-y-1">
            {targetPaths.map((path) => (
              <li key={path} className="break-all font-mono text-xs text-muted-foreground">
                {path}
              </li>
            ))}
          </ul>
        </div>
        <p className="mt-3 text-xs text-muted-foreground">
          {t('agentConversion.dangerDialog.sourceUnchanged')}
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button type="button" variant="outline" disabled={busy} onClick={onCancel}>
            {t('agentConversion.dangerDialog.cancel')}
          </Button>
          <Button type="button" variant="destructive" disabled={busy} onClick={onConfirm}>
            {busy ? t('agentConversion.applying') : t('agentConversion.dangerDialog.confirm')}
          </Button>
        </div>
      </div>
    </div>
  );
}

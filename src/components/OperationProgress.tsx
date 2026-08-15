import { useEffect, useState } from 'react';
import { LoaderCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';

interface OperationProgressProps {
  label: string;
  startedAt: number;
  hint?: string;
}

export function OperationProgress({ label, startedAt, hint }: OperationProgressProps) {
  const { t } = useTranslation();
  const [elapsedSeconds, setElapsedSeconds] = useState(() => elapsedSince(startedAt));

  useEffect(() => {
    const updateElapsed = () => setElapsedSeconds(elapsedSince(startedAt));
    updateElapsed();
    const interval = window.setInterval(updateElapsed, 1000);
    return () => window.clearInterval(interval);
  }, [startedAt]);

  return (
    <div
      role="status"
      aria-live="polite"
      aria-busy="true"
      className="rounded-md border border-border bg-muted/40 px-3 py-3"
    >
      <div className="flex items-start gap-2.5">
        <LoaderCircle
          className="mt-0.5 h-4 w-4 shrink-0 animate-spin text-primary motion-reduce:animate-none"
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
            <span className="text-xs font-medium text-foreground">{label}</span>
            <span className="whitespace-nowrap font-mono text-[11px] text-muted-foreground">
              {t('operationProgress.elapsed', { seconds: elapsedSeconds })}
            </span>
          </div>
          {hint && <p className="mt-1 text-xs text-muted-foreground">{hint}</p>}
        </div>
      </div>
    </div>
  );
}

function elapsedSince(startedAt: number): number {
  return Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
}

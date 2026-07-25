import { useEffect, useState } from 'react';
import { LoaderCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ConversionProgressEvent } from '@/lib/agentTypes';

export type ConversionOperationProgress =
  | ConversionProgressEvent
  | {
      phase: 'resolving_contexts' | 'applying';
      current: 0;
    };

interface AgentConversionProgressProps {
  progress: ConversionOperationProgress;
  startedAt: number;
}

export function AgentConversionProgress({ progress, startedAt }: AgentConversionProgressProps) {
  const { t } = useTranslation();
  const [elapsedSeconds, setElapsedSeconds] = useState(() =>
    Math.max(0, Math.floor((Date.now() - startedAt) / 1000)),
  );

  useEffect(() => {
    const updateElapsed = () => {
      setElapsedSeconds(Math.max(0, Math.floor((Date.now() - startedAt) / 1000)));
    };
    updateElapsed();
    const interval = window.setInterval(updateElapsed, 1000);
    return () => window.clearInterval(interval);
  }, [startedAt]);

  const applying = progress.phase === 'applying';
  const details =
    'item' in progress && progress.item
      ? progress.total !== undefined
        ? t('agentConversion.progress.itemCount', {
            item: progress.item,
            current: progress.current,
            total: progress.total,
          })
        : progress.item
      : null;

  return (
    <div
      role="status"
      aria-live="polite"
      aria-busy="true"
      className="mt-3 rounded-md border border-border bg-muted/40 px-3 py-2.5"
    >
      <div className="flex items-start gap-2.5">
        <LoaderCircle
          className="mt-0.5 h-4 w-4 shrink-0 animate-spin text-primary"
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
            <span className="text-xs font-medium text-foreground">
              {applying
                ? t('agentConversion.applying')
                : t(`agentConversion.progress.phase.${progress.phase}`)}
            </span>
            <span className="whitespace-nowrap font-mono text-[11px] text-muted-foreground">
              {t('agentConversion.progress.elapsed', { seconds: elapsedSeconds })}
            </span>
          </div>
          {details && <div className="mt-1 break-all font-mono text-xs">{details}</div>}
          {!applying && (
            <p className="mt-1 text-xs text-muted-foreground">
              {t('agentConversion.progress.slowHint')}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

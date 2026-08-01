import { useTranslation } from 'react-i18next';

import type { ConversionReport } from '@/lib/agentOperationReports';

interface AgentConversionReportProps {
  report: ConversionReport;
}

export function AgentConversionReport({ report }: AgentConversionReportProps) {
  const { t } = useTranslation();
  const receipt = report.receipt;
  const successful = report.outcome === 'changed' || report.outcome === 'no_change';

  return (
    <div
      role={successful ? 'status' : 'alert'}
      className={`mt-4 rounded-md border p-3 text-sm ${
        successful
          ? 'border-success/40 bg-success/10 text-foreground'
          : 'border-warning/40 bg-warning/10 text-foreground'
      }`}
    >
      <div className="font-medium">
        {report.outcome === 'changed'
          ? t('agentConversion.applied')
          : report.outcome === 'no_change'
            ? t('agentConversion.report.noChange')
            : receipt?.status === 'compensated'
              ? t('agentConversion.compensated')
              : t('agentConversion.report.partialApplied')}
      </div>
      {receipt?.message && <div className="mt-1 text-xs">{receipt.message}</div>}
      {report.residuals.length > 0 && (
        <div className="mt-1 text-xs">
          {t('agentConversion.report.residualCount', { count: report.residuals.length })}
        </div>
      )}
      {receipt && (
        <div className="mt-1 text-xs">
          {t('agentConversion.backupCount', { count: receipt.backupPaths.length })}
        </div>
      )}
    </div>
  );
}

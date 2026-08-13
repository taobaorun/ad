import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';

import type { ConversionArtifact, ConversionRoutePreview } from '@/lib/agentTypes';

interface AgentConversionArtifactsProps {
  preview: ConversionRoutePreview;
}

export function AgentConversionArtifacts({ preview }: AgentConversionArtifactsProps) {
  const { t } = useTranslation();
  const groups = useMemo(() => groupArtifacts(preview.artifacts), [preview.artifacts]);
  const summary = preview.summary;

  return (
    <section className="mt-4" aria-labelledby="conversion-artifacts-title">
      <h3 id="conversion-artifacts-title" className="text-sm font-semibold">
        {t('agentConversion.artifacts')}
      </h3>
      <dl className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-5">
        <SummaryItem label={t('agentConversion.summary.automatic')} value={summary.automatic} />
        <SummaryItem
          label={t('agentConversion.summary.requiresInput')}
          value={summary.requiresInput}
        />
        <SummaryItem label={t('agentConversion.summary.unsupported')} value={summary.unsupported} />
        <SummaryItem label={t('agentConversion.summary.conflicts')} value={summary.conflicts} />
        <SummaryItem label={t('agentConversion.summary.dangerous')} value={summary.dangerous} />
      </dl>

      <div className="mt-3 space-y-3">
        {groups.map(([kind, artifacts]) => (
          <section key={kind} className="overflow-hidden rounded-lg border border-border">
            <h4 className="border-b border-border bg-muted/40 px-3 py-2 text-xs font-semibold uppercase tracking-wide">
              {t(`agentConversion.kind.${kind}`)}
            </h4>
            <ul className="divide-y divide-border">
              {artifacts.map((artifact) => (
                <ArtifactItem key={artifact.id} artifact={artifact} />
              ))}
            </ul>
          </section>
        ))}
      </div>

      {!preview.plan && (
        <p role="status" className="mt-3 text-sm text-muted-foreground">
          {t('agentConversion.noChanges', {
            count: summary.requiresInput + summary.unsupported + summary.conflicts,
          })}
        </p>
      )}
    </section>
  );
}

function SummaryItem({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-md border border-border bg-muted/20 px-2.5 py-2">
      <dt className="text-[11px] text-muted-foreground">{label}</dt>
      <dd className="mt-0.5 text-lg font-semibold tabular-nums">{value}</dd>
    </div>
  );
}

interface ArtifactItemProps {
  artifact: ConversionArtifact;
}

function ArtifactItem({ artifact }: ArtifactItemProps) {
  const { t } = useTranslation();

  return (
    <li className="px-3 py-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="break-all font-mono text-xs">{artifact.id}</div>
          <div className="mt-1 text-xs text-muted-foreground">{artifactMessage(artifact, t)}</div>
        </div>
        <span
          className={`shrink-0 rounded-full border px-2 py-0.5 text-xs ${
            artifact.risk === 'dangerous'
              ? 'border-destructive/50 text-destructive'
              : 'border-border'
          }`}
        >
          {t(`agentConversion.disposition.${artifact.disposition}`)}
        </span>
      </div>
      <dl className="mt-2 grid gap-1.5 text-xs">
        <PathRow label={t('agentConversion.sourcePath')} path={artifact.source.location.path} />
        <PathRow
          label={t('agentConversion.targetPath')}
          path={artifact.target?.location.path ?? t('agentConversion.manualTarget')}
        />
      </dl>
    </li>
  );
}

function PathRow({ label, path }: { label: string; path: string }) {
  return (
    <div className="grid min-w-0 grid-cols-[5rem_1fr] gap-2">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="break-all font-mono text-foreground">{path}</dd>
    </div>
  );
}

function groupArtifacts(artifacts: ConversionArtifact[]) {
  const groups = new Map<ConversionArtifact['kind'], ConversionArtifact[]>();
  for (const artifact of artifacts) {
    const group = groups.get(artifact.kind) ?? [];
    group.push(artifact);
    groups.set(artifact.kind, group);
  }
  return [...groups.entries()];
}

function artifactMessage(artifact: ConversionArtifact, t: ReturnType<typeof useTranslation>['t']) {
  if (artifact.detailCode) {
    return t(`agentConversion.pluginDetail.${artifact.detailCode}`);
  }
  if (artifact.resolution) {
    return t(`agentConversion.artifactMessage.resolution.${artifact.resolution.kind}`);
  }
  if (artifact.kind === 'rules' && artifact.itemCount !== undefined) {
    return t('agentConversion.artifactMessage.permissionRules', {
      count: artifact.itemCount,
    });
  }
  return t(`agentConversion.artifactMessage.disposition.${artifact.disposition}`);
}

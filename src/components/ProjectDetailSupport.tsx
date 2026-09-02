import type { CSSProperties, ReactNode } from 'react';
import { LoaderCircle, Repeat } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RuntimeInspection } from '@/hooks/useProjectCodexRuntimeInspection';
import type { ProfileFile } from '@/lib/profileSchema';

export function ProjectCodexRuntimeCard({
  inheritBaseConfig,
  runtimeStatus,
  saving,
  onInheritanceChange,
  onPreview,
  onRetry,
}: {
  inheritBaseConfig: boolean;
  runtimeStatus: RuntimeInspection;
  saving: boolean;
  onInheritanceChange: (inheritBaseConfig: boolean) => void;
  onPreview: () => void;
  onRetry: () => void;
}) {
  const { t } = useTranslation();
  const { state, runtime, error } = runtimeStatus;
  const checking = state === 'checking' || state === 'idle';
  const failed = state === 'error';
  const policyPending =
    runtime !== null && runtime.desiredInheritBaseConfig !== runtime.appliedInheritBaseConfig;
  const needsPreview = state === 'loaded' && (runtime === null || runtime.needsRefresh);
  let stateText = t('projectCodexRuntime.ready', { count: runtime?.pluginCount ?? 0 });
  if (checking) stateText = t('projectCodexRuntime.checking');
  else if (failed) stateText = t('projectCodexRuntime.checkFailed');
  else if (runtime === null || !runtime.prepared) stateText = t('projectCodexRuntime.notPrepared');
  else if (policyPending) stateText = t('projectCodexRuntime.policyPending');
  else if (!runtime.fresh) stateText = t('projectCodexRuntime.diskDrift');
  else if (runtime.authMode !== 'shared_file') stateText = t('projectCodexRuntime.authBlocked');

  return (
    <section
      aria-labelledby="project-codex-runtime-title"
      aria-busy={checking || saving}
      className="mt-4 rounded-lg border border-border bg-muted/30 p-3"
    >
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <h2 id="project-codex-runtime-title" className="text-sm font-semibold">
            {t('projectCodexRuntime.title')}
          </h2>
          <p id="project-codex-inheritance-help" className="mt-1 text-xs text-muted-foreground">
            {t('projectCodexRuntime.inheritanceHint')}
          </p>
        </div>
        <label className="inline-flex min-h-9 shrink-0 cursor-pointer items-center gap-2 text-xs font-medium">
          <span>{t('projectCodexRuntime.inheritBaseConfig')}</span>
          <input
            type="checkbox"
            role="switch"
            checked={inheritBaseConfig}
            disabled={checking || saving}
            aria-describedby="project-codex-inheritance-help"
            onChange={(event) => onInheritanceChange(event.target.checked)}
            className="h-4 w-4 accent-primary"
          />
        </label>
      </div>
      <div className="mt-3 flex flex-col gap-2 border-t border-border pt-3 sm:flex-row sm:items-center sm:justify-between">
        <div role={failed ? 'alert' : 'status'} aria-live="polite" className="min-w-0 text-xs">
          <div className="flex items-center gap-2 font-medium">
            {(checking || saving) && (
              <LoaderCircle
                className="h-3.5 w-3.5 animate-spin motion-reduce:animate-none"
                aria-hidden="true"
              />
            )}
            <span>{stateText}</span>
          </div>
          {runtime && (
            <p className="mt-1 text-muted-foreground">
              {t('projectCodexRuntime.policySummary', {
                desired: t(
                  runtime.desiredInheritBaseConfig
                    ? 'projectCodexRuntime.inherited'
                    : 'projectCodexRuntime.isolated',
                ),
                applied: t(
                  runtime.appliedInheritBaseConfig
                    ? 'projectCodexRuntime.inherited'
                    : 'projectCodexRuntime.isolated',
                ),
              })}
            </p>
          )}
          {error && <p className="mt-1 break-all text-destructive">{error}</p>}
        </div>
        {failed ? (
          <button type="button" onClick={onRetry} className="self-start sm:self-auto" style={dsBtn}>
            {t('projectCodexRuntime.retry')}
          </button>
        ) : needsPreview ? (
          <button
            type="button"
            onClick={onPreview}
            className="self-start sm:self-auto"
            style={dsBtn}
          >
            {policyPending
              ? t('projectCodexRuntime.previewApply')
              : t('projectCodexRuntime.preview')}
          </button>
        ) : null}
      </div>
    </section>
  );
}

export function TemplateBreadcrumb({
  initializedFrom,
  onSwitchTemplate,
}: {
  initializedFrom: ProfileFile | null;
  onSwitchTemplate: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div
      className="mt-6 flex items-center justify-between gap-3 rounded-lg px-3.5 py-2"
      style={{ border: '0.5px solid var(--ds-line)', background: 'var(--ds-bg-inset)' }}
    >
      <div
        className="flex min-w-0 items-center gap-2.5 text-[12.5px]"
        style={{ color: 'var(--ds-fg-3)' }}
      >
        {initializedFrom ? (
          <>
            <span
              className="h-2 w-2 shrink-0 rounded-full"
              style={{ background: initializedFrom.color }}
            />
            <span className="truncate">
              {t('detail.initializedFrom', { name: initializedFrom.displayName })}
            </span>
          </>
        ) : (
          <span className="italic">{t('detail.noTemplateYet')}</span>
        )}
      </div>
      <button type="button" onClick={onSwitchTemplate} style={dsBtn}>
        <Repeat className="h-3.5 w-3.5" />
        {t('detail.switchTemplate')}
      </button>
    </div>
  );
}

export function StatusPill({
  ok,
  warn,
  children,
}: {
  ok?: boolean;
  warn?: boolean;
  children: ReactNode;
}) {
  let color = 'var(--ds-fg-2)';
  let bg = 'var(--ds-bg-soft)';
  let borderColor = 'var(--ds-line)';
  if (ok) {
    color = 'var(--ds-ok)';
    bg = 'rgb(var(--color-success) / 0.1)';
    borderColor = 'rgb(var(--color-success) / 0.35)';
  }
  if (warn) {
    color = 'var(--ds-warning)';
    bg = 'var(--ds-warning-soft)';
    borderColor = 'rgb(var(--color-warning) / 0.35)';
  }
  return (
    <span
      className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-[5px] px-2 py-[3px] font-mono text-[11.5px]"
      style={{ background: bg, border: `0.5px solid ${borderColor}`, color }}
    >
      <span className="h-[5px] w-[5px] shrink-0 rounded-full bg-current opacity-85" />
      {children}
    </span>
  );
}

export function KbdChip({
  children,
  className = '',
  style,
}: {
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <span
      className={`inline-flex h-[18px] min-w-[18px] shrink-0 items-center justify-center whitespace-nowrap rounded-[5px] px-[5px] font-mono text-[10.5px] ${className}`}
      style={{
        background: 'var(--ds-bg-soft)',
        border: '0.5px solid var(--ds-line)',
        color: 'var(--ds-fg-3)',
        boxShadow: 'inset 0 -1px 0 rgb(var(--color-text-primary) / 0.12)',
        ...style,
      }}
    >
      {children}
    </span>
  );
}

export function TabButton({
  id,
  panelId,
  active,
  onClick,
  children,
}: {
  id: string;
  panelId: string;
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      id={id}
      type="button"
      role="tab"
      aria-selected={active}
      aria-controls={panelId}
      tabIndex={active ? 0 : -1}
      onClick={onClick}
      className="ad-motion-press-subtle -mb-px border-b-2 bg-transparent px-4 py-2 text-[13px] font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      style={{
        color: active ? 'var(--ds-accent)' : 'var(--ds-fg-3)',
        borderBottomColor: active ? 'var(--ds-accent)' : 'transparent',
      }}
    >
      {children}
    </button>
  );
}

const dsBtn: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 7,
  height: 30,
  padding: '0 11px',
  borderRadius: 7,
  fontFamily: 'inherit',
  fontSize: 12.5,
  fontWeight: 500,
  border: '0.5px solid var(--ds-line-strong)',
  background: 'var(--ds-bg-card)',
  color: 'var(--ds-fg-2)',
  cursor: 'pointer',
};

import type { ComponentPropsWithoutRef } from 'react';
import { ChevronDown } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { AgentInstallation } from '@/lib/agentTypes';

export type ConversionScope = 'user' | 'project';
export type PermissionPreset = '' | 'on_request_workspace_write' | 'never_danger_full_access';

interface AgentConversionOptionsProps {
  scope: ConversionScope;
  activeProjectPath: string | null;
  busy: boolean;
  showInstallationControls: boolean;
  sourceInstallations: AgentInstallation[];
  targetInstallations: AgentInstallation[];
  sourceId: AgentInstallation['id'] | null;
  targetId: AgentInstallation['id'] | null;
  targetModel: string;
  permissionPreset: PermissionPreset;
  profileId: string;
  inheritBaseConfig: boolean;
  onScopeChange: (scope: ConversionScope) => void;
  onSourceChange: (id: AgentInstallation['id']) => void;
  onTargetChange: (id: AgentInstallation['id']) => void;
  onTargetModelChange: (value: string) => void;
  onPermissionPresetChange: (value: PermissionPreset) => void;
  onProfileIdChange: (value: string) => void;
}

export function AgentConversionOptions({
  scope,
  activeProjectPath,
  busy,
  showInstallationControls,
  sourceInstallations,
  targetInstallations,
  sourceId,
  targetId,
  targetModel,
  permissionPreset,
  profileId,
  inheritBaseConfig,
  onScopeChange,
  onSourceChange,
  onTargetChange,
  onTargetModelChange,
  onPermissionPresetChange,
  onProfileIdChange,
}: AgentConversionOptionsProps) {
  const { t } = useTranslation();

  return (
    <>
      <div>
        <label
          htmlFor="conversion-scope"
          className="mb-1 block text-xs font-medium text-muted-foreground"
        >
          {t('agentConversion.scope')}
        </label>
        <ConversionSelect
          id="conversion-scope"
          value={scope}
          disabled={busy}
          onChange={(event) => onScopeChange(event.target.value as ConversionScope)}
        >
          <option value="user">{t('agentConversion.scopeUser')}</option>
          <option value="project" disabled={!activeProjectPath}>
            {t('agentConversion.scopeProject')}
          </option>
        </ConversionSelect>
        <p className="mt-1 text-xs text-muted-foreground">
          {scope === 'project' && activeProjectPath ? (
            <>
              {t('agentConversion.scopeProjectHint')}
              <span className="ml-1 break-all font-mono text-foreground">{activeProjectPath}</span>
            </>
          ) : (
            t('agentConversion.scopeUserHint')
          )}
        </p>
        {scope === 'project' && activeProjectPath && (
          <p className="mt-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
            {t('agentConversion.projectRuntimeHint')}
          </p>
        )}
      </div>

      {showInstallationControls && (
        <details className="mt-3 rounded-md border border-border px-3 py-2">
          <summary className="cursor-pointer text-xs font-medium">
            {t('agentConversion.advancedInstances')}
          </summary>
          <p className="mt-2 text-xs text-muted-foreground">{t('agentConversion.instanceHint')}</p>
          <div className="mt-2 grid gap-3 sm:grid-cols-2">
            <InstallationSelect
              id="conversion-source"
              label={t('agentConversion.sourceInstance')}
              installations={sourceInstallations}
              value={sourceId}
              disabled={busy}
              onChange={onSourceChange}
            />
            <InstallationSelect
              id="conversion-target"
              label={t('agentConversion.targetInstance')}
              installations={targetInstallations}
              value={targetId}
              disabled={busy}
              onChange={onTargetChange}
            />
          </div>
        </details>
      )}

      <div className="mt-3 rounded-md border border-border p-3">
        <h3 className="text-xs font-semibold">{t('agentConversion.decisions')}</h3>
        <div className="mt-2 grid gap-3 sm:grid-cols-2">
          <div>
            <label
              htmlFor="conversion-model"
              className="mb-1 block text-xs font-medium text-muted-foreground"
            >
              {t('agentConversion.codexModel')}
            </label>
            <input
              id="conversion-model"
              value={targetModel}
              disabled={busy}
              onChange={(event) => onTargetModelChange(event.target.value)}
              placeholder={t('agentConversion.codexModelPlaceholder')}
              className="h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-sm"
            />
            <p className="mt-1 text-xs text-muted-foreground">
              {t('agentConversion.codexModelHint')}
            </p>
          </div>
          <div>
            <label
              htmlFor="conversion-permissions"
              className="mb-1 block text-xs font-medium text-muted-foreground"
            >
              {t('agentConversion.codexPermissions')}
            </label>
            <ConversionSelect
              id="conversion-permissions"
              value={permissionPreset}
              disabled={busy}
              onChange={(event) => onPermissionPresetChange(event.target.value as PermissionPreset)}
            >
              <option value="">{t('agentConversion.permissionsPreserve')}</option>
              <option value="on_request_workspace_write">
                {t('agentConversion.permissionsSafe')}
              </option>
              <option value="never_danger_full_access">
                {t('agentConversion.permissionsBypass')}
              </option>
            </ConversionSelect>
            <p
              className={`mt-1 text-xs ${
                permissionPreset === 'never_danger_full_access'
                  ? 'text-destructive'
                  : 'text-muted-foreground'
              }`}
            >
              {permissionPreset === 'never_danger_full_access'
                ? t('agentConversion.permissionsDangerHint')
                : t('agentConversion.permissionsHint')}
            </p>
          </div>
          {scope === 'project' && (
            <div className="sm:col-span-2">
              <label
                htmlFor="conversion-profile"
                className="mb-1 block text-xs font-medium text-muted-foreground"
              >
                {t('agentConversion.profileId')}
              </label>
              <input
                id="conversion-profile"
                value={profileId}
                disabled={busy || !inheritBaseConfig}
                onChange={(event) => onProfileIdChange(event.target.value)}
                placeholder={t('agentConversion.profileIdPlaceholder')}
                className="h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-sm"
              />
              <p className="mt-1 text-xs text-muted-foreground">
                {inheritBaseConfig
                  ? t('agentConversion.profileIdHint')
                  : t('agentConversion.profileDisabledHint')}
              </p>
            </div>
          )}
        </div>
      </div>
    </>
  );
}

type ConversionSelectProps = Omit<ComponentPropsWithoutRef<'select'>, 'className'>;

function ConversionSelect({ children, disabled, ...props }: ConversionSelectProps) {
  return (
    <span className="relative block">
      <select
        {...props}
        disabled={disabled}
        className="h-9 w-full appearance-none rounded-md border border-input bg-background bg-none px-2 pr-8 text-sm disabled:cursor-not-allowed disabled:opacity-50"
      >
        {children}
      </select>
      <ChevronDown
        aria-hidden="true"
        className={`pointer-events-none absolute right-2 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground ${
          disabled ? 'opacity-50' : ''
        }`}
      />
    </span>
  );
}

interface InstallationSelectProps {
  id: string;
  label: string;
  installations: AgentInstallation[];
  value: AgentInstallation['id'] | null;
  disabled: boolean;
  onChange: (id: AgentInstallation['id']) => void;
}

function InstallationSelect({
  id,
  label,
  installations,
  value,
  disabled,
  onChange,
}: InstallationSelectProps) {
  return (
    <div>
      <label htmlFor={id} className="mb-1 block text-xs font-medium text-muted-foreground">
        {label}
      </label>
      <ConversionSelect
        id={id}
        value={value ?? ''}
        disabled={disabled}
        onChange={(event) => {
          const selected = installations.find(
            (installation) => installation.id === event.target.value,
          );
          if (selected) onChange(selected.id);
        }}
      >
        {installations.map((installation) => (
          <option key={installation.id} value={installation.id}>
            {installation.rootPath}
          </option>
        ))}
      </ConversionSelect>
    </div>
  );
}

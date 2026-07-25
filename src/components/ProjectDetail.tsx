/**
 * Project detail pane — per-project config model (v0.4).
 *
 * Author: taobaorun
 *
 * Layout:
 *   1. Header — name / path / status pills / remove button
 *   2. Breadcrumb — "initialized from template <name>" + Switch template
 *   3. ProjectConfigEditor — three tabs (Shared / Local / Env), Save = sync
 *
 * Profile editing (template editing) lives in the right drawer, opened from
 * ⌘K command palette ("manage templates" / "edit template <name>"). It is
 * intentionally not reachable from this pane any more.
 */

import {
  lazy,
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';
import { useUiState } from '@/store/ui';
import { useProjects } from '@/store/projects';
import { useProfiles } from '@/store/profiles';
import { useAgents } from '@/store/agents';
import { useUiSettings } from '@/store/uiSettings';
import { tauri } from '@/lib/tauri';
import { LoaderCircle, Trash2, Repeat, SquareTerminal, X as XIcon } from 'lucide-react';
import type { ProcessObservation } from '@/lib/agentTypes';
import { formatAgentError, formatAgentErrorMessage } from '@/lib/agentErrors';
import { capabilityAllows } from '@/lib/agentCapabilities';
import { profileFeaturesFor } from '@/lib/profileEditorRegistry';
import {
  isProjectRuntimeLaunchUnavailable,
  resolveBaseProjectContext,
  resolveProjectAgentContext,
} from '@/lib/projectCodexRuntime';
import type { Project, ProjectStatus } from '@/lib/projectTypes';
import type { ProfileFile } from '@/lib/profileSchema';
import {
  type RuntimeInspection,
  useProjectCodexRuntimeInspection,
} from '@/hooks/useProjectCodexRuntimeInspection';
import { SwitchTemplateDialog } from './SwitchTemplateDialog';
import { AgentCollectionPanel } from './AgentCollectionPanel';

const AgentSettingsEditor = lazy(() =>
  import('./AgentSettingsEditor').then((module) => ({ default: module.AgentSettingsEditor })),
);

function EditorSkeleton() {
  return (
    <div
      className="h-full w-full rounded-lg"
      style={{ background: 'var(--ds-bg-inset)', border: '0.5px solid var(--ds-line)' }}
    />
  );
}

export function ProjectDetail() {
  const { t } = useTranslation();
  const activePath = useUiState((s) => s.activeProjectPath);
  const projects = useProjects((s) => s.projects);
  const project = projects.find((p) => p.path === activePath) ?? null;

  if (!project) {
    return (
      <div
        className="flex h-full items-center justify-center text-sm"
        style={{ color: 'var(--ds-fg-4)' }}
      >
        {t('detail.selectPrefix')}
        <KbdChip className="mx-1">⌘1</KbdChip>
        {t('detail.selectSuffix')}
      </div>
    );
  }
  return <Detail project={project} key={project.path} />;
}

function Detail({ project }: { project: Project }) {
  const { t } = useTranslation();
  const profiles = useProfiles((s) => s.profiles);
  const removeProject = useProjects((s) => s.removeProject);
  const reloadProjects = useProjects((s) => s.loadAll);
  const setCodexConfigInheritance = useProjects((s) => s.setCodexConfigInheritance);

  const switchOpen = useUiState((s) => s.switchTemplateOpen);
  const openSwitchTemplate = useUiState((s) => s.openSwitchTemplate);
  const closeSwitchTemplate = useUiState((s) => s.closeSwitchTemplate);

  const [status, setStatus] = useState<ProjectStatus | null>(null);
  const [editorReloadKey, setEditorReloadKey] = useState(0);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [launching, setLaunching] = useState(false);
  const [processes, setProcesses] = useState<ProcessObservation[]>([]);
  const [savingInheritance, setSavingInheritance] = useState(false);
  const launchingRef = useRef(false);
  const terminal = useUiSettings((s) => s.terminal);
  const activeContext = useAgents((s) => s.activeContext);
  const installations = useAgents((s) => s.installations);
  const activeAgentId = useAgents((s) => s.activeAgentId);
  const activeCapabilities = useAgents((s) => s.activeCapabilities);
  const profileFeatures = profileFeaturesFor(activeAgentId);
  const [activeTab, setActiveTab] = useState<'settings' | 'skills'>('settings');

  useEffect(() => {
    const reloadWorkspace = () => setEditorReloadKey((key) => key + 1);
    window.addEventListener('ad:agent-workspace-changed', reloadWorkspace);
    return () => window.removeEventListener('ad:agent-workspace-changed', reloadWorkspace);
  }, []);

  const baseProjectContext = useMemo(
    () => resolveBaseProjectContext(activeContext, installations, project.path),
    [activeContext, installations, project.path],
  );
  const [runtimeInspection, setRuntimeInspection] = useProjectCodexRuntimeInspection(
    baseProjectContext,
    activeAgentId,
  );
  const codexRuntime = runtimeInspection.runtime;
  const projectContext = useMemo(
    () => resolveProjectAgentContext(baseProjectContext, activeAgentId, codexRuntime),
    [activeAgentId, baseProjectContext, codexRuntime],
  );
  const settingsAvailable = capabilityAllows(activeCapabilities, 'settings', 'inspect', 'project');
  const launchSupported = capabilityAllows(
    activeCapabilities,
    'terminal_launch',
    'launch',
    'project',
  );
  const processDetectionSupported = capabilityAllows(
    activeCapabilities,
    'process_detection',
    'detect',
    'project',
  );

  useEffect(() => {
    if (!projectContext || !processDetectionSupported) {
      setProcesses([]);
      return;
    }
    void tauri
      .detectAgentProcesses(projectContext)
      .then(setProcesses)
      .catch(() => setProcesses([]));
  }, [processDetectionSupported, projectContext]);

  useEffect(() => {
    void (async () => {
      try {
        setStatus(await tauri.getProjectStatus(project.path));
      } catch {
        setStatus(null);
      }
    })();
  }, [project.path, project.lastApplied]);

  const initializedFrom = profiles.find((p) => p.id === project.currentProfileId) ?? null;

  const customMissing = terminal.backend === 'custom' && terminal.customCommand.trim() === '';
  const backendLabel = t(`terminal.backend.${terminal.backend}`);
  const runtimeLaunchUnavailable = isProjectRuntimeLaunchUnavailable(
    activeAgentId,
    project.inheritBaseConfig,
    runtimeInspection,
  );
  const launchUnavailable =
    customMissing || projectContext === null || !launchSupported || runtimeLaunchUnavailable;
  const openTitle = launchUnavailable
    ? t('terminal.openTooltipDisabled')
    : t('terminal.openTooltip', { backend: backendLabel, path: project.path });

  async function openTerminal() {
    if (launchingRef.current || launchUnavailable || !projectContext) return;
    launchingRef.current = true;
    setLaunching(true);
    setLaunchError(null);
    try {
      await tauri.openInTerminal({
        context: projectContext,
        backend: terminal.backend,
        customTemplate: terminal.customCommand || undefined,
      });
    } catch (err) {
      setLaunchError(formatAgentErrorMessage(err));
    } finally {
      // 1.5s cooldown — Ghostty `open -na` returns immediately but the new
      // window/tab takes a beat to actually appear; without this, an
      // impatient double-click spawns two tabs.
      setTimeout(() => {
        launchingRef.current = false;
        setLaunching(false);
      }, 1500);
    }
  }

  async function updateCodexInheritance(inheritBaseConfig: boolean) {
    if (savingInheritance) return;
    setSavingInheritance(true);
    try {
      await setCodexConfigInheritance(project.path, inheritBaseConfig);
      window.dispatchEvent(new Event('ad:project-codex-runtime-changed'));
    } catch (error) {
      setRuntimeInspection((current) => ({
        state: 'error',
        runtime: current.runtime,
        error: formatAgentError(error),
      }));
    } finally {
      setSavingInheritance(false);
    }
  }

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden"
      style={{ background: 'rgb(var(--background))' }}
    >
      <div
        className="flex-shrink-0"
        style={{ width: '100%', maxWidth: 1400, margin: '0 auto', padding: '32px 40px 0' }}
      >
        {/* Header */}
        <div
          style={{
            display: 'flex',
            alignItems: 'flex-start',
            justifyContent: 'space-between',
            gap: 16,
          }}
        >
          <div style={{ minWidth: 0, flex: 1 }}>
            <h1
              style={{
                fontSize: 24,
                fontWeight: 600,
                letterSpacing: '-0.02em',
                color: 'rgb(var(--foreground))',
                margin: 0,
              }}
            >
              {project.displayName}
            </h1>
            <div className="mt-1.5 font-mono text-[12.5px]" style={{ color: 'var(--ds-fg-3)' }}>
              {project.path}
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginTop: 14 }}>
              {status && (
                <>
                  <StatusPill ok={!status.gitDirty && status.isGitRepo} warn={status.gitDirty}>
                    {status.isGitRepo
                      ? status.gitDirty
                        ? t('detail.gitDirty')
                        : t('detail.gitClean')
                      : t('detail.notARepo')}
                  </StatusPill>
                  {status.isGitRepo && status.gitignoreExcludesSettingsLocal === false && (
                    <StatusPill warn>{t('detail.settingsLocalNotIgnored')}</StatusPill>
                  )}
                  {processes.length > 0 && (
                    <StatusPill ok>
                      {t('agentWorkspace.runningProcesses', { count: processes.length })}
                    </StatusPill>
                  )}
                </>
              )}
              {codexRuntime && (
                <StatusPill ok={codexRuntime.fresh} warn={!codexRuntime.fresh}>
                  {codexRuntime.fresh
                    ? t('projectCodexRuntime.ready', { count: codexRuntime.pluginCount })
                    : t('projectCodexRuntime.needsRefresh')}
                </StatusPill>
              )}
            </div>
            {codexRuntime && (
              <div
                className="mt-2 break-all font-mono text-[11px]"
                style={{ color: 'var(--ds-fg-4)' }}
              >
                {t('projectCodexRuntime.home')}: {codexRuntime.runtimeHome}
                {codexRuntime.profileId
                  ? ` · ${t('projectCodexRuntime.profile')}: ${codexRuntime.profileId}`
                  : ''}
              </div>
            )}
          </div>
          <div style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <button
              type="button"
              onClick={() => void openTerminal()}
              disabled={launchUnavailable || launching}
              title={openTitle}
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 6,
                height: 30,
                padding: '0 11px',
                borderRadius: 7,
                fontSize: 12.5,
                fontWeight: 500,
                border: '0.5px solid var(--ds-line-strong)',
                background: 'var(--ds-bg-card)',
                color: launchUnavailable ? 'var(--ds-fg-4)' : 'var(--ds-fg-2)',
                cursor: launchUnavailable ? 'not-allowed' : 'pointer',
                opacity: launching ? 0.6 : 1,
              }}
            >
              <SquareTerminal className="h-3.5 w-3.5" />
              {t('terminal.openInTerminal')}
            </button>
            <button
              type="button"
              onClick={() => {
                if (window.confirm(t('detail.removeConfirm', { name: project.displayName }))) {
                  void removeProject(project.path);
                }
              }}
              title={t('detail.removeTitle')}
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                justifyContent: 'center',
                width: 30,
                height: 30,
                borderRadius: 7,
                background: 'transparent',
                border: 0,
                color: 'var(--ds-fg-4)',
                cursor: 'pointer',
              }}
              onMouseEnter={(e) => {
                (e.currentTarget as HTMLElement).style.background = 'var(--ds-danger-soft)';
                (e.currentTarget as HTMLElement).style.color = 'var(--ds-danger)';
              }}
              onMouseLeave={(e) => {
                (e.currentTarget as HTMLElement).style.background = 'transparent';
                (e.currentTarget as HTMLElement).style.color = 'var(--ds-fg-4)';
              }}
            >
              <Trash2 className="h-4 w-4" />
            </button>
          </div>
        </div>

        {launchError && (
          <div
            role="alert"
            style={{
              marginTop: 12,
              padding: '10px 14px',
              borderRadius: 8,
              background: 'var(--ds-danger-soft)',
              border: '0.5px solid var(--ds-danger)',
              color: 'var(--ds-danger)',
              display: 'flex',
              alignItems: 'flex-start',
              gap: 10,
              fontSize: 12.5,
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontWeight: 600 }}>{t('terminal.launchFailed')}</div>
              <div
                className="font-mono"
                style={{ marginTop: 2, fontSize: 11.5, opacity: 0.85, wordBreak: 'break-all' }}
              >
                {launchError}
              </div>
            </div>
            <button
              type="button"
              onClick={() => setLaunchError(null)}
              aria-label={t('terminal.dismissLaunchError')}
              style={{
                background: 'transparent',
                border: 0,
                color: 'inherit',
                cursor: 'pointer',
                padding: 2,
              }}
            >
              <XIcon className="h-3.5 w-3.5" />
            </button>
          </div>
        )}

        {activeAgentId === 'codex' && (
          <ProjectCodexRuntimeCard
            inheritBaseConfig={project.inheritBaseConfig}
            runtimeStatus={runtimeInspection}
            saving={savingInheritance}
            onInheritanceChange={(inherit) => void updateCodexInheritance(inherit)}
            onPreview={() => window.dispatchEvent(new Event('ad:open-project-conversion'))}
            onRetry={() => window.dispatchEvent(new Event('ad:project-codex-runtime-changed'))}
          />
        )}

        {/* Breadcrumb: initialized from template + Switch */}
        {profileFeatures.legacyProjectTemplates && (
          <TemplateBreadcrumb
            initializedFrom={initializedFrom}
            onSwitchTemplate={openSwitchTemplate}
          />
        )}
      </div>

      {/* Tab bar */}
      <div
        className="flex-shrink-0"
        style={{ width: '100%', maxWidth: 1400, margin: '0 auto', padding: '16px 40px 0' }}
      >
        <div className="flex gap-0" style={{ borderBottom: '1px solid var(--ds-line)' }}>
          <TabButton active={activeTab === 'settings'} onClick={() => setActiveTab('settings')}>
            {t('agentWorkspace.tabs.settings')}
          </TabButton>
          <TabButton active={activeTab === 'skills'} onClick={() => setActiveTab('skills')}>
            {t('agentWorkspace.tabs.resources')}
          </TabButton>
        </div>
      </div>

      {/* Tab content — fills remaining vertical space */}
      <div
        className="min-h-0 flex-1"
        style={{ width: '100%', maxWidth: 1400, margin: '0 auto', padding: '0 40px 40px' }}
      >
        {activeTab === 'settings' && projectContext && settingsAvailable ? (
          <div className="h-full pt-5">
            <Suspense fallback={<EditorSkeleton />}>
              <AgentSettingsEditor key={editorReloadKey} context={projectContext} />
            </Suspense>
          </div>
        ) : activeTab === 'settings' ? (
          <div
            role="status"
            className="flex h-full items-center justify-center text-sm"
            style={{ color: 'var(--ds-fg-4)' }}
          >
            {t('agentWorkspace.settingsUnavailable')}
          </div>
        ) : (
          <div
            className="h-full pt-2"
            style={{
              border: '0.5px solid var(--ds-line)',
              borderTop: 'none',
              borderRadius: '0 0 8px 8px',
            }}
          >
            {projectContext ? (
              <AgentCollectionPanel
                key={editorReloadKey}
                context={projectContext}
                capabilities={activeCapabilities}
              />
            ) : (
              <div
                role="status"
                className="flex h-full items-center justify-center text-sm"
                style={{ color: 'var(--ds-fg-4)' }}
              >
                {t('agentWorkspace.resourcesUnavailable')}
              </div>
            )}
          </div>
        )}
      </div>

      {profileFeatures.legacyProjectTemplates && (
        <SwitchTemplateDialog
          open={switchOpen}
          projectPath={project.path}
          currentProfileId={project.currentProfileId ?? null}
          onOpenChange={(v) => {
            if (!v) closeSwitchTemplate();
            else openSwitchTemplate();
          }}
          onApplied={() => {
            void reloadProjects();
            setEditorReloadKey((k) => k + 1);
          }}
        />
      )}
    </div>
  );
}

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
  if (checking) {
    stateText = t('projectCodexRuntime.checking');
  } else if (failed) {
    stateText = t('projectCodexRuntime.checkFailed');
  } else if (runtime === null || !runtime.prepared) {
    stateText = t('projectCodexRuntime.notPrepared');
  } else if (policyPending) {
    stateText = t('projectCodexRuntime.policyPending');
  } else if (!runtime.fresh) {
    stateText = t('projectCodexRuntime.diskDrift');
  } else if (runtime.authMode !== 'shared_file') {
    stateText = t('projectCodexRuntime.authBlocked');
  }

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
              <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
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

function TemplateBreadcrumb({
  initializedFrom,
  onSwitchTemplate,
}: {
  initializedFrom: ProfileFile | null;
  onSwitchTemplate: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div
      style={{
        marginTop: 24,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: 12,
        borderRadius: 8,
        border: '0.5px solid var(--ds-line)',
        background: 'var(--ds-bg-inset)',
        padding: '8px 14px',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          fontSize: 12.5,
          color: 'var(--ds-fg-3)',
          minWidth: 0,
        }}
      >
        {initializedFrom ? (
          <>
            <span
              style={{
                width: 8,
                height: 8,
                borderRadius: '50%',
                background: initializedFrom.color,
                flexShrink: 0,
              }}
            />
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {t('detail.initializedFrom', { name: initializedFrom.displayName })}
            </span>
          </>
        ) : (
          <span style={{ fontStyle: 'italic' }}>{t('detail.noTemplateYet')}</span>
        )}
      </div>
      <button type="button" onClick={onSwitchTemplate} style={dsBtn}>
        <Repeat className="h-3.5 w-3.5" />
        {t('detail.switchTemplate')}
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Shared primitives (header pills, command-key chip)
// ---------------------------------------------------------------------------

function StatusPill({ ok, warn, children }: { ok?: boolean; warn?: boolean; children: ReactNode }) {
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
      className="font-mono"
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 5,
        fontSize: 11.5,
        padding: '3px 8px 3px 7px',
        borderRadius: 5,
        background: bg,
        border: `0.5px solid ${borderColor}`,
        color,
        whiteSpace: 'nowrap',
      }}
    >
      <span
        style={{
          width: 5,
          height: 5,
          borderRadius: '50%',
          background: 'currentColor',
          opacity: 0.85,
          flexShrink: 0,
        }}
      />
      {children}
    </span>
  );
}

function KbdChip({
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
      className={`inline-flex items-center justify-center font-mono ${className}`}
      style={{
        height: 18,
        minWidth: 18,
        padding: '0 5px',
        borderRadius: 5,
        background: 'var(--ds-bg-soft)',
        border: '0.5px solid var(--ds-line)',
        color: 'var(--ds-fg-3)',
        boxShadow: 'inset 0 -1px 0 rgb(var(--color-text-primary) / 0.12)',
        fontSize: 10.5,
        whiteSpace: 'nowrap',
        flexShrink: 0,
        ...style,
      }}
    >
      {children}
    </span>
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

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        padding: '8px 16px',
        fontSize: 13,
        fontWeight: 500,
        color: active ? 'var(--ds-accent)' : 'var(--ds-fg-3)',
        background: 'transparent',
        border: 'none',
        borderBottom: active ? '2px solid var(--ds-accent)' : '2px solid transparent',
        cursor: 'pointer',
        marginBottom: -1,
      }}
    >
      {children}
    </button>
  );
}

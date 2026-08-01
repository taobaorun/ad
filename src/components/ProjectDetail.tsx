/**
 * Unified Project Agent Workspace for Settings, Skills, Plugins, and History.
 *
 * Author: taobaorun
 *
 * Backend-signed inventory and action descriptors are the only source of
 * project mutation capabilities. Legacy profile controls remain visible only
 * for Agents that still advertise them.
 *
 * Profile editing (template editing) lives in the right drawer, opened from
 * ⌘K command palette ("manage templates" / "edit template <name>"). It is
 * intentionally not reachable from this pane any more.
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiState } from '@/store/ui';
import { useProjects } from '@/store/projects';
import { useProfiles } from '@/store/profiles';
import { useAgents } from '@/store/agents';
import { useUiSettings } from '@/store/uiSettings';
import { tauri } from '@/lib/tauri';
import { Trash2, SquareTerminal, X as XIcon } from 'lucide-react';
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
import { useProjectCodexRuntimeInspection } from '@/hooks/useProjectCodexRuntimeInspection';
import { handleProjectWorkspaceTabKeyDown } from '@/lib/projectWorkspaceTabs';
import { SwitchTemplateDialog } from './SwitchTemplateDialog';
import { ProjectWorkspacePanels, type ProjectWorkspaceTab } from './ProjectWorkspacePanels';
import {
  KbdChip,
  ProjectCodexRuntimeCard,
  StatusPill,
  TabButton,
  TemplateBreadcrumb,
} from './ProjectDetailSupport';
import { LegacySkillMigrationCard } from './LegacySkillMigrationCard';

export { ProjectCodexRuntimeCard } from './ProjectDetailSupport';

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
  const [activeTab, setActiveTab] = useState<ProjectWorkspaceTab>('settings');

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

        {activeAgentId === 'claude-code' && <LegacySkillMigrationCard projectPath={project.path} />}

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
        <div
          role="tablist"
          aria-label={t('agentWorkspace.tabs.label')}
          className="flex gap-0"
          style={{ borderBottom: '1px solid var(--ds-line)' }}
          onKeyDown={handleProjectWorkspaceTabKeyDown}
        >
          <TabButton
            id="project-workspace-tab-settings"
            panelId="project-workspace-panel-settings"
            active={activeTab === 'settings'}
            onClick={() => setActiveTab('settings')}
          >
            {t('agentWorkspace.tabs.settings')}
          </TabButton>
          <TabButton
            id="project-workspace-tab-resources"
            panelId="project-workspace-panel-resources"
            active={activeTab === 'resources'}
            onClick={() => setActiveTab('resources')}
          >
            {t('agentWorkspace.tabs.resources')}
          </TabButton>
          <TabButton
            id="project-workspace-tab-history"
            panelId="project-workspace-panel-history"
            active={activeTab === 'history'}
            onClick={() => setActiveTab('history')}
          >
            {t('agentWorkspace.tabs.history')}
          </TabButton>
        </div>
      </div>

      <ProjectWorkspacePanels
        activeTab={activeTab}
        reloadKey={editorReloadKey}
        context={projectContext}
        settingsAvailable={settingsAvailable}
        capabilities={activeCapabilities}
        onOpenHistory={() => setActiveTab('history')}
      />

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

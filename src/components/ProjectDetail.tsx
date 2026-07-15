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

import { lazy, Suspense, useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiState } from '@/store/ui';
import { useProjects } from '@/store/projects';
import { useProfiles } from '@/store/profiles';
import { useAgents } from '@/store/agents';
import { useUiSettings } from '@/store/uiSettings';
import { tauri } from '@/lib/tauri';
import { Trash2, Repeat, SquareTerminal, X as XIcon } from 'lucide-react';
import type { Project, ProjectStatus } from '@/lib/projectTypes';
import type { ProfileFile } from '@/lib/profileSchema';
import { SwitchTemplateDialog } from './SwitchTemplateDialog';
import { ProjectSkills } from './ProjectSkills';

// ProjectConfigEditor pulls in CodeMirror + the layered settings editor;
// lazy-loading it keeps the App entry chunk small. The Suspense fallback is
// an unlabeled muted box — a short flash is better than text that hints at
// trouble.
const ProjectConfigEditor = lazy(() =>
  import('./ProjectConfigEditor').then((m) => ({ default: m.ProjectConfigEditor })),
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
      <div className="flex h-full items-center justify-center text-sm" style={{ color: 'var(--ds-fg-4)' }}>
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

  const switchOpen = useUiState((s) => s.switchTemplateOpen);
  const openSwitchTemplate = useUiState((s) => s.openSwitchTemplate);
  const closeSwitchTemplate = useUiState((s) => s.closeSwitchTemplate);

  const [status, setStatus] = useState<ProjectStatus | null>(null);
  const [editorReloadKey, setEditorReloadKey] = useState(0);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [launching, setLaunching] = useState(false);
  const launchingRef = useRef(false);
  const terminal = useUiSettings((s) => s.terminal);
  const activeContext = useAgents((s) => s.activeContext);
  const [activeTab, setActiveTab] = useState<'settings' | 'skills'>('settings');

  useEffect(() => {
    void (async () => {
      try { setStatus(await tauri.getProjectStatus(project.path)); }
      catch { setStatus(null); }
    })();
  }, [project.path, project.lastApplied]);

  const initializedFrom = profiles.find((p) => p.id === project.currentProfileId) ?? null;

  const customMissing = terminal.backend === 'custom' && terminal.customCommand.trim() === '';
  const backendLabel = t(`terminal.backend.${terminal.backend}`);
  const launchUnavailable = customMissing || activeContext === null;
  const openTitle = launchUnavailable
    ? t('terminal.openTooltipDisabled')
    : t('terminal.openTooltip', { backend: backendLabel, path: project.path });

  async function openTerminal() {
    if (launchingRef.current || launchUnavailable || !activeContext) return;
    launchingRef.current = true;
    setLaunching(true);
    setLaunchError(null);
    try {
      await tauri.openInTerminal({
        context: { ...activeContext, projectPath: project.path },
        backend: terminal.backend,
        customTemplate: terminal.customCommand || undefined,
      });
    } catch (err) {
      setLaunchError(err instanceof Error ? err.message : String(err));
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

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden"
      style={{ background: 'hsl(var(--background))' }}
    >
      <div
        className="flex-shrink-0"
        style={{ width: '100%', maxWidth: 1400, margin: '0 auto', padding: '32px 40px 0' }}
      >
        {/* Header */}
        <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 16 }}>
          <div style={{ minWidth: 0, flex: 1 }}>
            <h1 style={{ fontSize: 24, fontWeight: 600, letterSpacing: '-0.02em', color: 'hsl(var(--foreground))', margin: 0 }}>
              {project.displayName}
            </h1>
            <div className="font-mono text-[12.5px] mt-1.5" style={{ color: 'var(--ds-fg-3)' }}>
              {project.path}
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginTop: 14 }}>
              {status && (
                <>
                  <StatusPill ok={!status.gitDirty && status.isGitRepo} warn={status.gitDirty}>
                    {status.isGitRepo
                      ? (status.gitDirty ? t('detail.gitDirty') : t('detail.gitClean'))
                      : t('detail.notARepo')}
                  </StatusPill>
                  {status.claudeDirExists && (
                    <StatusPill ok>{t('detail.claudeDirPresent')}</StatusPill>
                  )}
                  {status.isGitRepo && status.gitignoreExcludesSettingsLocal === false && (
                    <StatusPill warn>{t('detail.settingsLocalNotIgnored')}</StatusPill>
                  )}
                </>
              )}
            </div>
          </div>
          <div style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <button
              type="button"
              onClick={() => void openTerminal()}
              disabled={launchUnavailable || launching}
              title={openTitle}
              style={{
                display: 'inline-flex', alignItems: 'center', gap: 6,
                height: 30, padding: '0 11px', borderRadius: 7,
                fontSize: 12.5, fontWeight: 500,
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
                display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
                width: 30, height: 30, borderRadius: 7,
                background: 'transparent', border: 0, color: 'var(--ds-fg-4)', cursor: 'pointer',
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
              marginTop: 12, padding: '10px 14px', borderRadius: 8,
              background: 'var(--ds-danger-soft)', border: '0.5px solid var(--ds-danger)',
              color: 'var(--ds-danger)',
              display: 'flex', alignItems: 'flex-start', gap: 10, fontSize: 12.5,
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontWeight: 600 }}>{t('terminal.launchFailed')}</div>
              <div className="font-mono" style={{ marginTop: 2, fontSize: 11.5, opacity: 0.85, wordBreak: 'break-all' }}>
                {launchError}
              </div>
            </div>
            <button
              type="button"
              onClick={() => setLaunchError(null)}
              aria-label="dismiss"
              style={{ background: 'transparent', border: 0, color: 'inherit', cursor: 'pointer', padding: 2 }}
            >
              <XIcon className="h-3.5 w-3.5" />
            </button>
          </div>
        )}

        {/* Breadcrumb: initialized from template + Switch */}
        <TemplateBreadcrumb
          initializedFrom={initializedFrom}
          onSwitchTemplate={openSwitchTemplate}
        />
      </div>

      {/* Tab bar */}
      <div
        className="flex-shrink-0"
        style={{ width: '100%', maxWidth: 1400, margin: '0 auto', padding: '16px 40px 0' }}
      >
        <div className="flex gap-0" style={{ borderBottom: '1px solid var(--ds-line)' }}>
          <TabButton active={activeTab === 'settings'} onClick={() => setActiveTab('settings')}>
            Settings
          </TabButton>
          <TabButton active={activeTab === 'skills'} onClick={() => setActiveTab('skills')}>
            Skills
          </TabButton>
        </div>
      </div>

      {/* Tab content — fills remaining vertical space */}
      <div
        className="flex-1 min-h-0"
        style={{ width: '100%', maxWidth: 1400, margin: '0 auto', padding: '0 40px 40px' }}
      >
        {activeTab === 'settings' ? (
          <div className="h-full pt-5">
            <Suspense fallback={<EditorSkeleton />}>
              <ProjectConfigEditor key={editorReloadKey} projectPath={project.path} />
            </Suspense>
          </div>
        ) : (
          <div className="h-full pt-2" style={{ border: '0.5px solid var(--ds-line)', borderTop: 'none', borderRadius: '0 0 8px 8px' }}>
            <ProjectSkills projectPath={project.path} />
          </div>
        )}
      </div>

      <SwitchTemplateDialog
        open={switchOpen}
        projectPath={project.path}
        currentProfileId={project.currentProfileId ?? null}
        onOpenChange={(v) => { if (!v) closeSwitchTemplate(); else openSwitchTemplate(); }}
        onApplied={() => {
          void reloadProjects();
          setEditorReloadKey((k) => k + 1);
        }}
      />
    </div>
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
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, fontSize: 12.5, color: 'var(--ds-fg-3)', minWidth: 0 }}>
        {initializedFrom ? (
          <>
            <span style={{ width: 8, height: 8, borderRadius: '50%', background: initializedFrom.color, flexShrink: 0 }} />
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
  if (ok) { color = 'var(--ds-ok)'; bg = 'rgba(21,128,61,0.06)'; borderColor = 'rgba(21,128,61,0.18)'; }
  if (warn) { color = 'var(--ds-warning)'; bg = 'var(--ds-warning-soft)'; borderColor = 'rgba(194,65,12,0.18)'; }

  return (
    <span
      className="font-mono"
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 5,
        fontSize: 11.5,
        padding: '3px 8px 3px 7px',
        borderRadius: 5,
        background: bg,
        border: `0.5px solid ${borderColor}`,
        color,
        whiteSpace: 'nowrap',
      }}
    >
      <span style={{ width: 5, height: 5, borderRadius: '50%', background: 'currentColor', opacity: 0.85, flexShrink: 0 }} />
      {children}
    </span>
  );
}

function KbdChip({ children, className = '', style }: { children: ReactNode; className?: string; style?: CSSProperties }) {
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
        boxShadow: 'inset 0 -1px 0 rgba(0,0,0,0.06)',
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
  display: 'inline-flex', alignItems: 'center', gap: 7,
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

function TabButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        padding: '8px 16px',
        fontSize: 13,
        fontWeight: 500,
        color: active ? 'var(--ds-clay)' : 'var(--ds-fg-3)',
        background: 'transparent',
        border: 'none',
        borderBottom: active ? '2px solid var(--ds-clay)' : '2px solid transparent',
        cursor: 'pointer',
        marginBottom: -1,
      }}
    >
      {children}
    </button>
  );
}

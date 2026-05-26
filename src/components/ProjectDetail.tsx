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

import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiState } from '@/store/ui';
import { useProjects } from '@/store/projects';
import { useProfiles } from '@/store/profiles';
import { tauri } from '@/lib/tauri';
import { Trash2, Repeat } from 'lucide-react';
import type { Project, ProjectStatus } from '@/lib/projectTypes';
import type { ProfileFile } from '@/lib/profileSchema';
import { ProjectConfigEditor } from './ProjectConfigEditor';
import { SwitchTemplateDialog } from './SwitchTemplateDialog';

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

  useEffect(() => {
    void (async () => {
      try { setStatus(await tauri.getProjectStatus(project.path)); }
      catch { setStatus(null); }
    })();
  }, [project.path, project.lastApplied]);

  const initializedFrom = profiles.find((p) => p.id === project.currentProfileId) ?? null;

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

        {/* Breadcrumb: initialized from template + Switch */}
        <TemplateBreadcrumb
          initializedFrom={initializedFrom}
          onSwitchTemplate={openSwitchTemplate}
        />
      </div>

      {/* Inline project config editor — fills remaining vertical space */}
      <div
        className="flex-1 min-h-0"
        style={{ width: '100%', maxWidth: 1400, margin: '0 auto', padding: '20px 40px 40px' }}
      >
        <ProjectConfigEditor key={editorReloadKey} projectPath={project.path} />
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

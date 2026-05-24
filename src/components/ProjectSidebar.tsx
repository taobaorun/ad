import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useProjects } from '@/store/projects';
import { useProfiles } from '@/store/profiles';
import { useUiState } from '@/store/ui';
import { tauri } from '@/lib/tauri';
import type { Project, ProjectStatus } from '@/lib/projectTypes';
import { Plus } from 'lucide-react';

export function ProjectSidebar() {
  const { t } = useTranslation();
  const projects = useProjects((s) => s.projects);
  const detected = useProjects((s) => s.detected);
  const loadAll = useProjects((s) => s.loadAll);
  const refreshDetected = useProjects((s) => s.refreshDetected);
  const openDetectedModal = useProjects((s) => s.openDetectedModal);

  const activeProjectPath = useUiState((s) => s.activeProjectPath);
  const setActiveProject = useUiState((s) => s.setActiveProject);
  const openPalette = useUiState((s) => s.openPalette);

  useEffect(() => {
    void (async () => {
      await loadAll();
      try { await refreshDetected(); } catch { /* backend logs */ }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (projects.length === 0) return;
    if (activeProjectPath && projects.some((p) => p.path === activeProjectPath)) return;
    setActiveProject(projects[0]?.path ?? null);
  }, [projects, activeProjectPath, setActiveProject]);

  const unaddedDetected = detected.filter((d) => !d.alreadyAdded);

  return (
    <div
      className="flex h-full flex-col"
      style={{ background: 'var(--ds-bg-sidebar)', borderRight: '0.5px solid var(--ds-line)' }}
    >
      {/* Header */}
      <div
        className="flex items-center gap-2 px-4 py-3.5"
        style={{ borderBottom: '0.5px solid var(--ds-line)' }}
      >
        <span
          className="text-[11px] font-medium uppercase tracking-widest"
          style={{ color: 'var(--ds-fg-4)' }}
        >
          {t('sidebar.projects')}
        </span>
        <KbdChip>⌘1-8</KbdChip>
        <button
          type="button"
          onClick={() => openPalette('add ')}
          title={t('sidebar.addProjectTitle')}
          aria-label={t('sidebar.addProject')}
          className="ml-auto inline-flex items-center gap-1 rounded-md px-2 py-0.5 text-[11px] font-medium"
          style={{
            border: '0.5px solid var(--ds-line-strong)',
            background: 'var(--ds-bg-card)',
            color: 'var(--ds-fg-2)',
            fontFamily: 'var(--font-mono, ui-monospace)',
          }}
        >
          <Plus className="h-2.5 w-2.5" />
          新建
        </button>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto px-2 py-2" style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
        {projects.length === 0 ? (
          <div className="flex h-full items-center justify-center px-4 text-center text-xs" style={{ color: 'var(--ds-fg-4)' }}>
            {t('sidebar.noProjectsPrefix')}
            <KbdChip className="mx-1">⌘T</KbdChip>
            {t('sidebar.noProjectsSuffix')}
          </div>
        ) : (
          projects.map((p, idx) => (
            <ProjectRow
              key={p.path}
              project={p}
              shortcutIdx={idx}
              active={p.path === activeProjectPath}
              onSelect={() => setActiveProject(p.path)}
            />
          ))
        )}
      </div>

      {/* Footer */}
      <div
        className="flex items-center justify-between px-3.5 py-2.5 text-xs"
        style={{ borderTop: '0.5px solid var(--ds-line)', color: 'var(--ds-fg-3)' }}
      >
        {unaddedDetected.length > 0 ? (
          <>
            <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
                <path d="M8 2l1.2 3.6 3.6 1.2-3.6 1.2L8 11.6 6.8 8 3.2 6.8 6.8 5.6 8 2z" fill="currentColor" opacity="0.7"/>
              </svg>
              {t('sidebar.detectedCount', { count: unaddedDetected.length })}
            </span>
            <button
              type="button"
              onClick={() => void openDetectedModal()}
              className="rounded px-1.5 py-0.5 text-xs font-medium hover:opacity-80"
              style={{ color: 'var(--ds-accent)', background: 'transparent', border: 0, cursor: 'pointer' }}
            >
              {t('sidebar.review')}
            </button>
          </>
        ) : (
          <span className="text-[11px]" style={{ color: 'var(--ds-fg-4)' }}>
            {projects.length} 个项目
          </span>
        )}
      </div>
    </div>
  );
}

function ProjectRow({
  project,
  shortcutIdx,
  active,
  onSelect,
}: {
  project: Project;
  shortcutIdx: number;
  active: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  const profiles = useProfiles((s) => s.profiles);
  const profile = profiles.find((p) => p.id === project.currentProfileId);

  const [status, setStatus] = useState<ProjectStatus | null>(null);
  useEffect(() => {
    void (async () => {
      try { setStatus(await tauri.getProjectStatus(project.path)); }
      catch { setStatus(null); }
    })();
  }, [project.path, project.lastApplied]);

  const shortcutLabel = shortcutIdx < 9 ? `⌘${shortcutIdx + 1}` : '';
  const missing = status?.exists === false;
  const dirty = status?.gitDirty === true;

  // Status text: git · last layer · time ago
  let gitStatus = '';
  if (status) {
    if (!status.isGitRepo) gitStatus = t('detail.notARepo');
    else if (dirty) gitStatus = '△ dirty';
    else gitStatus = '✓ clean';
  }
  const lastLayer = project.lastApplied?.layers?.[0] ?? null;
  const lastTime = project.lastApplied?.timestamp
    ? relativeTime(project.lastApplied.timestamp)
    : null;

  const dotColor = profile?.color ?? 'var(--ds-fg-5)';

  return (
    <button
      type="button"
      onClick={onSelect}
      className="block w-full rounded-lg text-left transition-colors"
      style={{
        padding: '10px 12px',
        background: active
          ? 'var(--ds-bg-card)'
          : 'transparent',
        boxShadow: active
          ? '0 0 0 0.5px var(--ds-line-strong), 0 1px 2px rgba(0,0,0,0.04)'
          : undefined,
        opacity: missing ? 0.5 : 1,
        display: 'grid',
        gridTemplateColumns: '22px 1fr 10px',
        gridTemplateRows: 'auto auto auto',
        columnGap: 6,
        rowGap: 3,
        alignItems: 'center',
      }}
      onMouseEnter={(e) => {
        if (!active) (e.currentTarget as HTMLElement).style.background = 'var(--ds-bg-card-hov)';
      }}
      onMouseLeave={(e) => {
        if (!active) (e.currentTarget as HTMLElement).style.background = 'transparent';
      }}
    >
      {/* Row 1: num / name / dot */}
      <span
        className="font-mono text-[10.5px]"
        style={{ color: 'var(--ds-fg-4)', gridRow: 1, gridColumn: 1 }}
      >
        {shortcutLabel}
      </span>
      <span
        className="truncate text-[13px] font-medium"
        style={{ color: 'hsl(var(--foreground))', gridRow: 1, gridColumn: 2 }}
      >
        {project.displayName}
      </span>
      <span
        style={{
          width: 7, height: 7, borderRadius: '50%',
          background: dotColor,
          gridRow: 1, gridColumn: 3,
          justifySelf: 'end', display: 'block',
          flexShrink: 0,
        }}
        aria-label={profile ? t('sidebar.profileAria', { name: profile.displayName }) : undefined}
      />

      {/* Row 2: path */}
      <div
        className="truncate font-mono text-[11px]"
        style={{ color: 'var(--ds-fg-4)', gridRow: 2, gridColumn: '2 / 4' }}
      >
        {shortenPath(project.path)}
      </div>

      {/* Row 3: meta */}
      <div
        className="font-mono text-[10.5px]"
        style={{
          color: dirty ? 'var(--ds-warning)' : 'var(--ds-fg-3)',
          gridRow: 3, gridColumn: '2 / 4',
          display: 'flex', alignItems: 'center', gap: 6,
          whiteSpace: 'nowrap', overflow: 'hidden',
          marginTop: 2,
        }}
      >
        {missing ? (
          <span style={{ color: 'var(--ds-danger)' }}>{t('sidebar.pathNotFound')}</span>
        ) : (
          <>
            {gitStatus && <span>{gitStatus}</span>}
            {lastLayer && <><span style={{ color: 'var(--ds-fg-5)' }}>·</span><span style={{ color: 'var(--ds-fg-3)' }}>{lastLayer}</span></>}
            {lastTime && <><span style={{ color: 'var(--ds-fg-5)' }}>·</span><span style={{ color: 'var(--ds-fg-3)' }}>{lastTime}</span></>}
          </>
        )}
      </div>
    </button>
  );
}

function KbdChip({ children, className = '' }: { children: React.ReactNode; className?: string }) {
  return (
    <span
      className={`inline-flex items-center justify-center rounded font-mono text-[10.5px] ${className}`}
      style={{
        height: 18,
        minWidth: 18,
        padding: '0 5px',
        background: 'var(--ds-bg-soft)',
        border: '0.5px solid var(--ds-line)',
        color: 'var(--ds-fg-3)',
        boxShadow: 'inset 0 -1px 0 rgba(0,0,0,0.06)',
        whiteSpace: 'nowrap',
        flexShrink: 0,
      }}
    >
      {children}
    </span>
  );
}

function shortenPath(p: string): string {
  return p.replace(/^\/Users\/[^/]+/, '~');
}

function relativeTime(iso: string): string {
  const ts = new Date(iso).getTime();
  if (Number.isNaN(ts)) return iso;
  const diff = Math.max(0, Date.now() - ts);
  const m = 60_000, h = 60 * m, d = 24 * h;
  if (diff < m) return '刚刚';
  if (diff < h) return `${Math.floor(diff / m)}m ago`;
  if (diff < d) return `${Math.floor(diff / h)}h ago`;
  if (diff < 7 * d) return `${Math.floor(diff / d)}d ago`;
  return iso.slice(0, 10);
}

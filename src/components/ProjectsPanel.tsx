import { useEffect, useState } from 'react';
import { useProjects } from '@/store/projects';
import { useProfiles } from '@/store/profiles';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { AddProjectInput } from './AddProjectInput';
import { ProjectDropZone } from './ProjectDropZone';
import { ApplyDialog } from './ApplyDialog';
import { Sparkles, Trash2, Pencil, Check, X } from 'lucide-react';
import type { Project } from '@/lib/projectTypes';

/**
 * The Projects panel — right column of the main window (M4).
 *
 * Header: panel title + "✨ N detected" review banner.
 * Body: AddProjectInput + ProjectDropZone (two ways to add) + project list.
 * Project rows: profile color dot + display name + last-applied summary +
 *   [Apply] / [⋯] menu. Double-click to rename. Greyed if path missing.
 */
export function ProjectsPanel() {
  const projects = useProjects((s) => s.projects);
  const detected = useProjects((s) => s.detected);
  const loadAll = useProjects((s) => s.loadAll);
  const refreshDetected = useProjects((s) => s.refreshDetected);
  const openDetectedModal = useProjects((s) => s.openDetectedModal);

  const [applyTarget, setApplyTarget] = useState<Project | null>(null);

  useEffect(() => {
    void (async () => {
      await loadAll();
      // Probe scan for the banner — non-blocking.
      try {
        await refreshDetected();
      } catch {
        // ignore — backend logs the error
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const unaddedDetected = detected.filter((d) => !d.alreadyAdded);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-border p-3">
        <h2 className="text-sm font-semibold">Projects</h2>
        <span className="text-xs text-muted-foreground">{projects.length} tracked</span>
      </div>

      <div className="flex flex-col gap-2 border-b border-border p-3">
        <AddProjectInput />
        <ProjectDropZone />
      </div>

      {unaddedDetected.length > 0 && (
        <button
          type="button"
          onClick={() => void openDetectedModal()}
          className="flex items-center gap-2 border-b border-clay/40 bg-clay/5 px-3 py-2 text-left text-xs text-foreground transition-colors hover:bg-clay/10"
        >
          <Sparkles className="h-4 w-4 text-clay" />
          <span className="flex-1">
            <strong>{unaddedDetected.length}</strong> projects detected from scan roots.
          </span>
          <span className="font-medium text-clay">Review →</span>
        </button>
      )}

      <div className="flex-1 overflow-auto">
        {projects.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 px-6 text-center text-sm text-muted-foreground">
            <div>No projects tracked yet.</div>
            <div className="text-xs">
              Use the input above, drop a folder, or{' '}
              <button
                type="button"
                onClick={() => void openDetectedModal()}
                className="text-clay underline"
              >
                review detected projects
              </button>
              .
            </div>
          </div>
        ) : (
          <ul className="divide-y divide-border">
            {projects.map((p) => (
              <ProjectRow key={p.path} project={p} onApply={() => setApplyTarget(p)} />
            ))}
          </ul>
        )}
      </div>

      {applyTarget && (
        <ApplyDialog
          project={applyTarget}
          onClose={() => setApplyTarget(null)}
        />
      )}
    </div>
  );
}

function ProjectRow({
  project,
  onApply,
}: {
  project: Project;
  onApply: () => void;
}) {
  const removeProject = useProjects((s) => s.removeProject);
  const renameProject = useProjects((s) => s.renameProject);
  const profiles = useProfiles((s) => s.profiles);

  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(project.displayName);
  const [statusText, setStatusText] = useState<string | null>(null);

  // Probe path existence to grey out missing rows. Cheap — a single stat.
  // Also compute status banner text.
  useEffect(() => {
    void (async () => {
      try {
        const status = await (await import('@/lib/tauri')).tauri.getProjectStatus(project.path);
        if (!status.exists) {
          setStatusText('not found');
        } else if (project.lastApplied) {
          const profile = profiles.find((p) => p.id === project.lastApplied!.profileId);
          const layers = project.lastApplied.layers.join('+');
          const ago = relativeTime(project.lastApplied.timestamp);
          setStatusText(`${profile?.displayName ?? project.lastApplied.profileId} · ${layers} · ${ago}`);
        } else {
          setStatusText('never applied');
        }
      } catch {
        setStatusText(null);
      }
    })();
  }, [project.path, project.lastApplied, profiles]);

  const profile = profiles.find((p) => p.id === project.currentProfileId);
  const dotColor = profile?.color ?? '#D1CFC5';
  const missing = statusText === 'not found';

  async function commitRename() {
    const trimmed = name.trim();
    if (trimmed === '' || trimmed === project.displayName) {
      setEditing(false);
      setName(project.displayName);
      return;
    }
    try {
      await renameProject(project.path, trimmed);
    } catch {
      setName(project.displayName);
    }
    setEditing(false);
  }

  return (
    <li
      className={
        'flex items-center gap-3 px-3 py-2 ' +
        (missing ? 'opacity-50' : 'hover:bg-muted/40')
      }
    >
      <span
        className="block h-2.5 w-2.5 flex-shrink-0 rounded-full"
        style={{ background: dotColor }}
        aria-label="Current profile color"
      />
      <div className="min-w-0 flex-1">
        {editing ? (
          <div className="flex items-center gap-1">
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') void commitRename();
                if (e.key === 'Escape') {
                  setEditing(false);
                  setName(project.displayName);
                }
              }}
              className="h-7 max-w-xs"
            />
            <Button size="sm" variant="ghost" onClick={() => void commitRename()}>
              <Check className="h-3.5 w-3.5" />
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setEditing(false);
                setName(project.displayName);
              }}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>
        ) : (
          <button
            type="button"
            onDoubleClick={() => setEditing(true)}
            className="block w-full text-left"
          >
            <div className="truncate text-sm font-medium">{project.displayName}</div>
            <div className="truncate font-mono text-[11px] text-muted-foreground">
              {project.path}
            </div>
          </button>
        )}
        {!editing && statusText && (
          <div className={'truncate text-[11px] ' + (missing ? 'text-rust' : 'text-muted-foreground')}>
            {statusText}
          </div>
        )}
      </div>

      {!editing && (
        <>
          <Button size="sm" variant="ghost" onClick={() => setEditing(true)} aria-label="Rename">
            <Pencil className="h-3.5 w-3.5" />
          </Button>
          <Button
            size="sm"
            onClick={onApply}
            disabled={missing}
            title={missing ? 'Project directory not found' : undefined}
          >
            Apply
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              if (window.confirm(`Remove "${project.displayName}" from AD?\n(Files in the project are not touched.)`)) {
                void removeProject(project.path);
              }
            }}
            aria-label="Remove from AD"
          >
            <Trash2 className="h-3.5 w-3.5 text-muted-foreground" />
          </Button>
        </>
      )}
    </li>
  );
}

/** "5m ago" / "2h ago" / "3d ago" / "2026-04-01" for older. */
function relativeTime(iso: string): string {
  const t = new Date(iso).getTime();
  const now = Date.now();
  const diff = Math.max(0, now - t);
  const m = 60 * 1000;
  const h = 60 * m;
  const d = 24 * h;
  if (diff < m) return 'just now';
  if (diff < h) return `${Math.floor(diff / m)}m ago`;
  if (diff < d) return `${Math.floor(diff / h)}h ago`;
  if (diff < 7 * d) return `${Math.floor(diff / d)}d ago`;
  return iso.slice(0, 10);
}

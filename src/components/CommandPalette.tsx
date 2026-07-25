/**
 * ⌘K command palette (M4).
 *
 * Single source of truth for actions: apply / switch project / edit
 * profile / add project / review detected / open history / advanced.
 * UI buttons elsewhere call the same handlers — palette is the
 * canonical entry, the rest of the UI is its mirror.
 *
 * Commands are generated dynamically from `useProfiles` + `useProjects`
 * + `useUiState` (see ExecPlan D10). No separate registry store.
 */

import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { useUiState } from '@/store/ui';
import { useProfiles } from '@/store/profiles';
import { useProjects } from '@/store/projects';
import { useAgents } from '@/store/agents';
import { useUiSettings } from '@/store/uiSettings';
import { tauri } from '@/lib/tauri';
import { formatAgentErrorMessage } from '@/lib/agentErrors';
import { usePathAutocomplete } from '@/lib/pathAutocomplete';
import type { ApplyOptions } from '@/lib/projectTypes';
import { capabilityAllows } from '@/lib/agentCapabilities';
import { profileFeaturesFor } from '@/lib/profileEditorRegistry';

type Group = 'APPLY' | 'SWITCH' | 'EDIT' | 'ADD' | 'OTHER';

interface Command {
  group: Group;
  id: string;
  icon: string;
  label: string;
  desc?: string;
  run: () => void | Promise<void>;
}

const GROUP_ORDER: Group[] = ['APPLY', 'SWITCH', 'EDIT', 'ADD', 'OTHER'];

export function CommandPalette() {
  const { t } = useTranslation();
  const open = useUiState((s) => s.paletteOpen);
  const prefill = useUiState((s) => s.palettePrefill);
  const close = useUiState((s) => s.closePalette);
  const openPalette = useUiState((s) => s.openPalette);
  const setActiveProject = useUiState((s) => s.setActiveProject);
  const openEditDrawer = useUiState((s) => s.openEditDrawer);
  const openSwitchTemplate = useUiState((s) => s.openSwitchTemplate);
  const setView = useProfiles((s) => s.setView);
  const setImportOpen = useProfiles((s) => s.setImportOpen);
  const openDetectedModal = useProjects((s) => s.openDetectedModal);
  const reloadProjects = useProjects((s) => s.loadAll);
  const profiles = useProfiles((s) => s.profiles);
  const projects = useProjects((s) => s.projects);
  const activePath = useUiState((s) => s.activeProjectPath);
  const activeProject = projects.find((p) => p.path === activePath) ?? null;
  const activeContext = useAgents((s) => s.activeContext);
  const activeAgentId = useAgents((s) => s.activeAgentId);
  const activeCapabilities = useAgents((s) => s.activeCapabilities);
  const terminal = useUiSettings((s) => s.terminal);
  const profileFeatures = profileFeaturesFor(activeAgentId);

  const [term, setTerm] = useState<string>('');
  const [activeIdx, setActiveIdx] = useState<number>(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const addMode = term.startsWith('add ') && term.slice(4).length > 0;
  const addPath = addMode ? term.slice(4) : '';
  const { candidates: pathCandidates, completion: pathCompletion } = usePathAutocomplete(addPath);

  useEffect(() => {
    if (open) {
      setTerm(prefill);
      setActiveIdx(0);
      window.setTimeout(() => inputRef.current?.focus(), 30);
    }
  }, [open, prefill]);

  async function addProjectFromPath(rawPath: string) {
    const trimmed = rawPath.trim();
    if (!trimmed) return;
    const normalized = trimmed.length > 1 && trimmed.endsWith('/') ? trimmed.slice(0, -1) : trimmed;
    try {
      const project = await useProjects.getState().addProject(normalized);
      setActiveProject(project.path);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  }

  // Compute what the term should become if the user presses Tab in add mode.
  // Backend returns absolute paths; we map the matched segment back into the
  // user's prefix namespace (e.g. preserves `~/`).
  function nextTabValue(): string | null {
    if (!addMode || pathCompletion === '') return null;
    const userLastSlash = addPath.lastIndexOf('/');
    const userParent = userLastSlash >= 0 ? addPath.slice(0, userLastSlash + 1) : '';
    const userPartial = userLastSlash >= 0 ? addPath.slice(userLastSlash + 1) : addPath;
    let suffix: string;
    if (pathCompletion.endsWith('/')) {
      const trimmed = pathCompletion.slice(0, -1);
      suffix = trimmed.slice(trimmed.lastIndexOf('/') + 1) + '/';
    } else {
      suffix = pathCompletion.slice(pathCompletion.lastIndexOf('/') + 1);
    }
    if (suffix.length <= userPartial.length || !suffix.startsWith(userPartial)) {
      return null;
    }
    return 'add ' + userParent + suffix;
  }

  const commands = useMemo<Command[]>(() => {
    const list: Command[] = [];

    // APPLY: apply each profile to active project (single-layer "local" default)
    if (activeProject && profileFeatures.legacyProjectTemplates) {
      profiles.forEach((pf) => {
        list.push({
          group: 'APPLY',
          id: `apply-${pf.id}`,
          icon: '↻',
          label: t('palette.applyTo', {
            profile: pf.displayName,
            project: activeProject.displayName,
          }),
          desc: 'local',
          run: async () => {
            const opts: ApplyOptions = {
              layers: ['local'],
              resolutions: {},
              overwriteDirtyWarningAcked: false,
            };
            try {
              const r = await tauri.applyProfileToProject(pf.id, activeProject.path, opts);
              if (r.kind === 'applied') await reloadProjects();
              else if (r.kind === 'needsResolution') {
                // surface conflicts in the detail pane by selecting + nudging user
                alert(t('palette.conflictsAlert', { layer: r.layer }));
              } else {
                alert(t('palette.gitDirtyAlert', { message: r.message }));
              }
            } catch (e) {
              alert(e instanceof Error ? e.message : String(e));
            }
          },
        });
      });
    }

    // SWITCH: jump to any project (except current)
    projects.forEach((p, idx) => {
      if (p.path === activePath) return;
      list.push({
        group: 'SWITCH',
        id: `switch-${p.path}`,
        icon: '→',
        label: t('palette.switchTo', { project: p.displayName }),
        desc: idx < 9 ? `⌘${idx + 1}` : p.path,
        run: () => setActiveProject(p.path),
      });
    });

    // SWITCH-TEMPLATE: open the dialog for the active project
    if (activeProject) {
      if (profileFeatures.legacyProjectTemplates) {
        list.push({
          group: 'APPLY',
          id: 'switch-template',
          icon: '↻',
          label: t('palette.switchTemplate', { project: activeProject.displayName }),
          run: () => openSwitchTemplate(),
        });
      }

      // OPEN IN TERMINAL: use the active Agent adapter's launch recipe.
      const customMissing = terminal.backend === 'custom' && terminal.customCommand.trim() === '';
      if (
        !customMissing &&
        activeContext &&
        capabilityAllows(activeCapabilities, 'terminal_launch', 'launch', 'project')
      ) {
        const backendLabel = t(`terminal.backend.${terminal.backend}`);
        list.push({
          group: 'APPLY',
          id: 'open-in-terminal',
          icon: '▶',
          label: t('terminal.paletteEntry', { project: activeProject.displayName }),
          desc: t('terminal.paletteSubtitle', { backend: backendLabel, path: activeProject.path }),
          run: async () => {
            try {
              await tauri.openInTerminal({
                context: { ...activeContext, projectPath: activeProject.path },
                backend: terminal.backend,
                customTemplate: terminal.customCommand || undefined,
              });
            } catch (e) {
              alert(`${t('terminal.launchFailed')}: ${formatAgentErrorMessage(e)}`);
            }
          },
        });
      }
    }

    // EDIT: the legacy drawer only understands Claude Code profile payloads.
    if (profileFeatures.legacyProjectTemplates) {
      profiles.forEach((pf) => {
        list.push({
          group: 'EDIT',
          id: `edit-${pf.id}`,
          icon: '✎',
          label: t('palette.editTemplate', { name: pf.displayName }),
          run: () => openEditDrawer(pf.id),
        });
      });
    }

    // ADD
    list.push({
      group: 'ADD',
      id: 'add-path',
      icon: '+',
      label: t('palette.addProjectFromPath'),
      desc: '⌘T',
      // Re-open with `add ` prefill — click handler closes first, openPalette
      // immediately re-opens with the new prefill, useEffect[open,prefill]
      // resets term, and the autocomplete UI takes over.
      run: () => openPalette('add '),
    });
    list.push({
      group: 'ADD',
      id: 'add-detected',
      icon: '✨',
      label: t('palette.reviewDetected'),
      run: () => {
        void openDetectedModal();
      },
    });
    if (profileFeatures.legacyImport) {
      list.push({
        group: 'ADD',
        id: 'add-import',
        icon: '⬇',
        label: t('palette.importProfile'),
        run: () => setImportOpen(true),
      });
    }

    // OTHER
    list.push({
      group: 'OTHER',
      id: 'open-history',
      icon: '⏱',
      label: t('palette.openHistory'),
      run: () => setView('history'),
    });

    return list;
  }, [
    t,
    profiles,
    projects,
    activeProject,
    activePath,
    activeContext,
    activeCapabilities,
    profileFeatures,
    terminal,
    setActiveProject,
    openEditDrawer,
    openSwitchTemplate,
    setView,
    setImportOpen,
    openDetectedModal,
    reloadProjects,
    openPalette,
  ]);

  const filtered = useMemo(() => {
    const q = term.trim().toLowerCase();
    if (q === '') return commands;
    return commands.filter((c) =>
      (c.label + ' ' + c.group + ' ' + (c.desc ?? '')).toLowerCase().includes(q),
    );
  }, [commands, term]);

  // Group filtered commands while preserving GROUP_ORDER.
  const grouped = useMemo(() => {
    const buckets: Record<Group, Command[]> = {
      APPLY: [],
      SWITCH: [],
      EDIT: [],
      ADD: [],
      OTHER: [],
    };
    filtered.forEach((c) => buckets[c.group].push(c));
    const flat: Command[] = [];
    GROUP_ORDER.forEach((g) => buckets[g].forEach((c) => flat.push(c)));
    return { buckets, flat };
  }, [filtered]);

  const navMax = addMode ? pathCandidates.length : grouped.flat.length;

  useEffect(() => {
    if (activeIdx >= navMax) setActiveIdx(Math.max(0, navMax - 1));
  }, [navMax, activeIdx]);

  if (!open) return null;

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActiveIdx((i) => Math.min(i + 1, Math.max(0, navMax - 1)));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActiveIdx((i) => Math.max(i - 1, 0));
    } else if (e.key === 'Tab' && addMode) {
      e.preventDefault();
      const next = nextTabValue();
      if (next !== null) {
        setTerm(next);
        setActiveIdx(0);
      }
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (addMode) {
        const picked = pathCandidates[activeIdx] ?? addPath;
        close();
        void addProjectFromPath(picked);
        return;
      }
      const cmd = grouped.flat[activeIdx];
      if (cmd) {
        close();
        void cmd.run();
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }

  return (
    <div
      className="fixed inset-0 z-[100] flex items-start justify-center bg-overlay/65 pt-20 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div className="flex max-h-[500px] w-[560px] max-w-[calc(100%-48px)] flex-col overflow-hidden rounded-xl border border-border bg-background shadow-2xl">
        <div className="flex items-center gap-3 border-b border-border px-4 py-3">
          <span className="font-mono text-xs text-muted-foreground">⌘K</span>
          <input
            ref={inputRef}
            value={term}
            onChange={(e) => {
              setTerm(e.target.value);
              setActiveIdx(0);
            }}
            onKeyDown={onKeyDown}
            placeholder={t('palette.placeholder')}
            className="flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
          />
        </div>

        <div className="overflow-y-auto">
          {addMode ? (
            pathCandidates.length === 0 ? (
              <div className="px-4 py-8 text-center text-sm text-muted-foreground">
                {t('palette.noMatchingDirs')}{' '}
                <kbd className="mx-1 rounded border border-border bg-muted px-1 font-mono">↵</kbd>{' '}
                {t('palette.pressToAddAsIs', { path: addPath })}
              </div>
            ) : (
              <div>
                <div className="px-4 pb-1 pt-3 font-mono text-[10px] uppercase tracking-wider text-muted-foreground">
                  {t('palette.addProjectHeader')}
                </div>
                {pathCandidates.map((cand, idx) => {
                  const active = idx === activeIdx;
                  return (
                    <button
                      key={cand}
                      type="button"
                      onClick={() => {
                        close();
                        void addProjectFromPath(cand);
                      }}
                      onMouseEnter={() => setActiveIdx(idx)}
                      className={
                        'flex w-full items-center gap-3 px-4 py-2 text-left font-mono text-sm ' +
                        (active ? 'bg-muted text-foreground' : 'text-foreground hover:bg-muted/50')
                      }
                    >
                      <span className="w-4 text-center text-xs text-muted-foreground">›</span>
                      <span className="flex-1 truncate">{cand}</span>
                    </button>
                  );
                })}
              </div>
            )
          ) : grouped.flat.length === 0 ? (
            <div className="px-4 py-8 text-center text-sm text-muted-foreground">
              {t('palette.noMatches')}
            </div>
          ) : (
            GROUP_ORDER.map((g) => {
              const items = grouped.buckets[g];
              if (items.length === 0) return null;
              return (
                <div key={g}>
                  <div className="px-4 pb-1 pt-3 font-mono text-[10px] uppercase tracking-wider text-muted-foreground">
                    {t(`palette.groups.${g.toLowerCase()}`)}
                  </div>
                  {items.map((cmd) => {
                    const flatIdx = grouped.flat.indexOf(cmd);
                    const active = flatIdx === activeIdx;
                    return (
                      <button
                        key={cmd.id}
                        type="button"
                        onClick={() => {
                          close();
                          void cmd.run();
                        }}
                        onMouseEnter={() => setActiveIdx(flatIdx)}
                        className={
                          'flex w-full items-center gap-3 px-4 py-2 text-left text-sm ' +
                          (active
                            ? 'bg-muted text-foreground'
                            : 'text-foreground hover:bg-muted/50')
                        }
                      >
                        <span className="w-4 text-center font-mono text-xs text-muted-foreground">
                          {cmd.icon}
                        </span>
                        <span className="flex-1 truncate">{cmd.label}</span>
                        {cmd.desc && (
                          <span className="ml-auto font-mono text-[11px] text-muted-foreground">
                            {cmd.desc}
                          </span>
                        )}
                      </button>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>

        <div className="flex items-center gap-4 border-t border-border bg-muted/30 px-4 py-1.5 font-mono text-[10px] text-muted-foreground">
          <span>
            <kbd className="mr-1 rounded border border-border bg-background px-1">↑↓</kbd>
            {t('palette.kbd.navigate')}
          </span>
          {addMode && (
            <span>
              <kbd className="mr-1 rounded border border-border bg-background px-1">tab</kbd>
              {t('palette.kbd.complete')}
            </span>
          )}
          <span>
            <kbd className="mr-1 rounded border border-border bg-background px-1">↵</kbd>
            {addMode ? t('palette.kbd.add') : t('palette.kbd.run')}
          </span>
          <span>
            <kbd className="mr-1 rounded border border-border bg-background px-1">esc</kbd>
            {t('palette.kbd.close')}
          </span>
        </div>
      </div>
    </div>
  );
}

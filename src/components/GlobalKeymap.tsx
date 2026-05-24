/**
 * Mount-only component that wires global keyboard shortcuts to UI
 * state actions. Renders nothing.
 *
 * Shortcuts (M4 + ExecPlan D6):
 *   ⌘K        toggle command palette
 *   ⌘1-9      jump to project N
 *   ⌘T        open palette with `add ` prefill
 *   ⌘P        open palette with `apply ` prefill
 *   ⌘E        edit current project's profile
 *   ⌘⇧K       toggle sidebar collapse
 *   esc       close palette / drawer
 *
 * `mod+enter` is owned by `ProjectDetail` (apply) — registered there
 * because it depends on local apply state.
 */

import { useMemo } from 'react';
import { useShallow } from 'zustand/shallow';
import { useGlobalKeymap, type ShortcutMap } from '@/lib/keymap';
import { useUiState } from '@/store/ui';
import { useProjects } from '@/store/projects';

export function GlobalKeymap() {
  const {
    openPalette,
    closePalette,
    closeEditDrawer,
    setActiveProject,
    toggleSidebar,
    openEditDrawer,
  } = useUiState(
    useShallow((s) => ({
      openPalette: s.openPalette,
      closePalette: s.closePalette,
      closeEditDrawer: s.closeEditDrawer,
      setActiveProject: s.setActiveProject,
      toggleSidebar: s.toggleSidebar,
      openEditDrawer: s.openEditDrawer,
    })),
  );
  const projects = useProjects((s) => s.projects);

  const shortcuts = useMemo<ShortcutMap>(() => {
    const m: ShortcutMap = {
      'mod+k': () => {
        if (useUiState.getState().paletteOpen) closePalette();
        else openPalette();
      },
      'mod+shift+k': () => toggleSidebar(),
      'mod+t': () => openPalette('add '),
      'mod+p': () => openPalette('apply '),
      'mod+e': () => {
        const path = useUiState.getState().activeProjectPath;
        const project = projects.find((p) => p.path === path);
        const id = project?.currentProfileId;
        if (id) openEditDrawer(id);
      },
      esc: () => {
        const s = useUiState.getState();
        if (s.paletteOpen) closePalette();
        else if (s.editingProfileId) closeEditDrawer();
      },
    };
    for (let i = 0; i < Math.min(9, projects.length); i++) {
      const idx = i;
      m['mod+' + (idx + 1)] = () => {
        const p = projects[idx];
        if (p) setActiveProject(p.path);
      };
    }
    return m;
  }, [projects, openPalette, closePalette, closeEditDrawer, setActiveProject, toggleSidebar, openEditDrawer]);

  useGlobalKeymap(shortcuts);
  return null;
}

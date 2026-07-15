/**
 * Mount-only component that wires global keyboard shortcuts to UI
 * state actions. Renders nothing.
 *
 * Shortcuts:
 *   ⌘K        toggle command palette
 *   ⌘1-9      jump to project N
 *   ⌘T        open palette with `add ` prefill
 *   ⌘P        open palette with `apply ` prefill
 *   ⌘E        edit the template the current project was initialized from
 *   ⌘⇧K       toggle sidebar collapse
 *   esc       close palette / drawer
 */

import { useMemo } from 'react';
import { useShallow } from 'zustand/shallow';
import { useGlobalKeymap, type ShortcutMap } from '@/lib/keymap';
import { useUiState } from '@/store/ui';
import { useProjects } from '@/store/projects';
import { useAgents } from '@/store/agents';
import { profileFeaturesFor } from '@/lib/profileEditorRegistry';

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
  const activeAgentId = useAgents((s) => s.activeAgentId);
  const legacyProjectTemplates = profileFeaturesFor(activeAgentId).legacyProjectTemplates;

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
        if (!legacyProjectTemplates) return;
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
  }, [
    projects,
    legacyProjectTemplates,
    openPalette,
    closePalette,
    closeEditDrawer,
    setActiveProject,
    toggleSidebar,
    openEditDrawer,
  ]);

  useGlobalKeymap(shortcuts);
  return null;
}

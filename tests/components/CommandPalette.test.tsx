import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import { CommandPalette } from '@/components/CommandPalette';
import i18n from '@/i18n';
import { useAgents } from '@/store/agents';
import { useProfiles } from '@/store/profiles';
import { useProjects } from '@/store/projects';
import { useUiState } from '@/store/ui';
import { useUiSettings } from '@/store/uiSettings';

const projects = [
  {
    path: '/work/ad',
    displayName: 'ad',
    addedAt: '2026-08-01T08:00:00Z',
    inheritBaseConfig: false,
  },
  {
    path: '/work/sofampy',
    displayName: 'sofampy',
    addedAt: '2026-08-02T08:00:00Z',
    inheritBaseConfig: false,
  },
];

describe('CommandPalette', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    useUiState.setState({
      activeProjectPath: '/work/ad',
      workspaceMode: 'projects',
      sidebarCollapsed: false,
      paletteOpen: false,
      palettePrefill: '',
      editingProfileId: null,
      switchTemplateOpen: false,
    });
    useProjects.setState({
      projects,
      scanRoots: [],
      loading: false,
      detectedModalOpen: false,
      detected: [],
    });
    useProfiles.setState({
      profiles: [],
      activeId: null,
      selectedId: null,
      view: 'editor',
      importOpen: false,
      toasts: [],
      loading: false,
      agentId: 'codex',
    });
    useAgents.setState({
      agents: [],
      installations: [],
      activeContext: null,
      activeAgentId: 'codex',
      capabilitiesByAgent: { codex: [] },
      activeCapabilities: [],
      loading: false,
    });
    useUiSettings.setState({
      darkMode: true,
      showLegacyActivation: false,
      terminal: { backend: 'ghostty', customCommand: '' },
    });
  });

  it('exposes one dialog, combobox, listbox, and selected option while navigating', async () => {
    useUiState.setState({ paletteOpen: true });
    render(<CommandPalette />);

    const dialog = await screen.findByRole('dialog', { name: 'Type an action or project…' });
    const search = within(dialog).getByRole('combobox', {
      name: 'Type an action or project…',
    });
    const listbox = within(dialog).getByRole('listbox', {
      name: 'Type an action or project…',
    });
    await waitFor(() => expect(search).toHaveFocus());

    const options = within(listbox).getAllByRole('option');
    expect(options[0]).toHaveAttribute('aria-selected', 'true');
    expect(search).toHaveAttribute('aria-activedescendant', options[0]?.id);

    fireEvent.keyDown(search, { key: 'ArrowDown' });
    expect(options[1]).toHaveAttribute('aria-selected', 'true');
    expect(search).toHaveAttribute('aria-activedescendant', options[1]?.id);
  });

  it('closes instantly on Escape and returns focus to the invoking control', async () => {
    const trigger = document.createElement('button');
    trigger.textContent = 'Open palette';
    document.body.append(trigger);
    trigger.focus();

    render(<CommandPalette />);
    act(() => useUiState.getState().openPalette());

    const search = await screen.findByRole('combobox', {
      name: 'Type an action or project…',
    });
    await waitFor(() => expect(search).toHaveFocus());
    fireEvent.keyDown(search, { key: 'Escape' });

    expect(useUiState.getState().paletteOpen).toBe(false);
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(trigger).toHaveFocus();
    trigger.remove();
  });

  it('reopens immediately without retaining a closing surface', async () => {
    render(<CommandPalette />);

    act(() => useUiState.getState().openPalette());
    await screen.findByRole('dialog');
    act(() => useUiState.getState().closePalette());
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

    act(() => useUiState.getState().openPalette());
    const search = await screen.findByRole('combobox');
    await waitFor(() => expect(search).toHaveFocus());
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
  });
});

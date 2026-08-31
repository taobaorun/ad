import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { Titlebar } from '@/App';
import i18n from '@/i18n';

vi.mock('@/components/AdvancedSettings', () => ({
  AdvancedSettingsButton: () => <button type="button">Advanced settings</button>,
}));
vi.mock('@/components/AgentProfilesDialog', () => ({
  AgentProfilesButton: () => <button type="button">Profiles</button>,
}));
vi.mock('@/components/AgentConversionButton', () => ({
  AgentConversionButton: () => <button type="button">Convert</button>,
}));
vi.mock('@/components/AgentSelector', () => ({
  AgentSelector: () => (
    <select aria-label="Agent">
      <option>Codex</option>
    </select>
  ),
}));

describe('Titlebar workspace controls', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
  });

  it('hides project configuration controls in Harness mode', () => {
    render(<Titlebar resourceMode onOpenPalette={vi.fn()} dark onToggleDark={vi.fn()} />);

    expect(screen.getByRole('button', { name: 'Advanced settings' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Profiles' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Convert' })).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Agent' })).not.toBeInTheDocument();
  });

  it('keeps project configuration controls in project mode', () => {
    render(<Titlebar onOpenPalette={vi.fn()} dark onToggleDark={vi.fn()} />);

    expect(screen.getByRole('button', { name: 'Profiles' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Convert' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Agent' })).toBeInTheDocument();
  });
});

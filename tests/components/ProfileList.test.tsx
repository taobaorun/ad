import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useProfiles } from '@/store/profiles';
import { ProfileList } from '@/components/ProfileList';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => []),
}));

describe('<ProfileList />', () => {
  beforeEach(() => {
    useProfiles.setState({
      profiles: [],
      activeId: null,
      selectedId: null,
      view: 'editor',
      importOpen: false,
      toasts: [],
      loading: false,
    });
  });

  it('renders the empty state when there are no profiles', () => {
    render(<ProfileList />);
    expect(screen.getByText(/no profiles yet/i)).toBeInTheDocument();
  });

  it('renders rows for each profile and marks the active one', () => {
    useProfiles.setState({
      profiles: [
        {
          id: 'a',
          displayName: 'Alpha',
          description: null,
          color: '#FF0000',
          createdAt: '2026-05-13T22:55:00Z',
          updatedAt: '2026-05-13T22:55:00Z',
          settings: { env: {} },
        },
        {
          id: 'b',
          displayName: 'Beta',
          description: null,
          color: '#00FF00',
          createdAt: '2026-05-13T22:55:00Z',
          updatedAt: '2026-05-13T22:55:00Z',
          settings: { env: {} },
        },
      ],
      activeId: 'b',
      selectedId: 'a',
    });
    render(<ProfileList />);
    expect(screen.getByText('Alpha')).toBeInTheDocument();
    expect(screen.getByText('Beta')).toBeInTheDocument();
    expect(screen.getByText(/active/i)).toBeInTheDocument();
  });

  it('opens the import dialog when the import button is clicked', async () => {
    render(<ProfileList />);
    await userEvent.click(screen.getByLabelText(/import/i));
    expect(useProfiles.getState().importOpen).toBe(true);
  });
});

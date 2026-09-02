import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { ProfileEditDrawer } from '@/components/ProfileEditDrawer';
import i18n from '@/i18n';
import { useUiState } from '@/store/ui';

vi.mock('@/components/ProfileEditor', () => ({
  ProfileEditor: ({ profileId }: { profileId: string }) => <div>Editor {profileId}</div>,
}));

describe('ProfileEditDrawer motion lifecycle', () => {
  let frames: Array<(timestamp: number) => void>;

  beforeEach(async () => {
    await i18n.changeLanguage('en');
    frames = [];
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {});
    useUiState.setState({ editingProfileId: null });
  });

  it('starts closed, opens from the right, and retains content through exit', () => {
    useUiState.setState({ editingProfileId: 'profile-a' });
    render(<ProfileEditDrawer />);

    const drawer = screen.getByRole('complementary', { hidden: true });
    expect(drawer).toHaveAttribute('data-open', 'false');
    expect(screen.getByText('Editor profile-a')).toBeInTheDocument();

    act(() => frames.shift()?.(0));
    expect(drawer).toHaveAttribute('data-open', 'true');

    act(() => useUiState.getState().closeEditDrawer());
    expect(drawer).toHaveAttribute('data-open', 'false');
    expect(screen.getByText('Editor profile-a')).toBeInTheDocument();

    fireEvent.transitionEnd(drawer, { propertyName: 'transform' });
    expect(screen.queryByText('Editor profile-a')).not.toBeInTheDocument();
  });

  it('reopens during close without clearing the new editor', () => {
    useUiState.setState({ editingProfileId: 'profile-a' });
    render(<ProfileEditDrawer />);
    const drawer = screen.getByRole('complementary', { hidden: true });
    act(() => frames.shift()?.(0));

    act(() => useUiState.getState().closeEditDrawer());
    act(() => useUiState.getState().openEditDrawer('profile-b'));
    act(() => frames.shift()?.(16));
    fireEvent.transitionEnd(drawer, { propertyName: 'transform' });

    expect(drawer).toHaveAttribute('data-open', 'true');
    expect(screen.getByText('Editor profile-b')).toBeInTheDocument();
  });
});

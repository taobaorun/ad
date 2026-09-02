import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ActivateToast } from '@/components/ActivateToast';
import i18n from '@/i18n';
import { useProfiles } from '@/store/profiles';

vi.mock('@/lib/notifications', () => ({
  notifyActivation: vi.fn().mockResolvedValue(undefined),
  RESTART_COMMAND: 'restart-agent',
}));

vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }));

describe('ActivateToast motion lifecycle', () => {
  let frames: Array<(timestamp: number) => void>;
  let cancelFrame: ReturnType<typeof vi.spyOn>;

  beforeEach(async () => {
    await i18n.changeLanguage('en');
    vi.useFakeTimers();
    frames = [];
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    cancelFrame = vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {});
    useProfiles.setState({
      toasts: [{ id: 1, profileName: 'Daily', detected: [] }],
    });
  });

  afterEach(() => vi.useRealTimers());

  it('enters on a frame and removes from the store only after manual exit', () => {
    render(<ActivateToast />);
    const toast = screen.getByRole('status');
    expect(toast).toHaveAttribute('data-visible', 'false');
    act(() => frames.shift()?.(0));
    expect(toast).toHaveAttribute('data-visible', 'true');

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(toast).toHaveAttribute('data-visible', 'false');
    expect(useProfiles.getState().toasts).toHaveLength(1);
    fireEvent.transitionEnd(toast, { propertyName: 'transform' });
    expect(useProfiles.getState().toasts).toHaveLength(0);
  });

  it('uses the same exit path when the eight-second lifetime elapses', () => {
    render(<ActivateToast />);
    const toast = screen.getByRole('status');
    act(() => frames.shift()?.(0));
    act(() => vi.advanceTimersByTime(8000));

    expect(toast).toHaveAttribute('data-visible', 'false');
    expect(useProfiles.getState().toasts).toHaveLength(1);
    fireEvent.transitionEnd(toast, { propertyName: 'opacity' });
    expect(useProfiles.getState().toasts).toHaveLength(0);
  });

  it('cancels the pending entrance frame when dismissed immediately', () => {
    render(<ActivateToast />);
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));

    expect(cancelFrame).toHaveBeenCalledOnce();
    expect(screen.getByRole('status')).toHaveAttribute('data-visible', 'false');
  });
});

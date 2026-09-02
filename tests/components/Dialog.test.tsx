import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { Dialog } from '@/components/ui/dialog';
import i18n from '@/i18n';

describe('Dialog motion lifecycle', () => {
  let frames: Array<(timestamp: number) => void>;

  beforeEach(async () => {
    await i18n.changeLanguage('en');
    frames = [];
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {});
  });

  it('retains the dialog through exit and restores focus after transition', () => {
    const trigger = document.createElement('button');
    trigger.textContent = 'Open';
    document.body.append(trigger);
    trigger.focus();
    const onOpenChange = vi.fn();
    const view = render(
      <Dialog open onOpenChange={onOpenChange} title="Review changes">
        <button type="button">Confirm</button>
      </Dialog>,
    );

    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveAttribute('data-open', 'false');
    act(() => frames.shift()?.(0));
    expect(dialog).toHaveAttribute('data-open', 'true');

    view.rerender(
      <Dialog open={false} onOpenChange={onOpenChange} title="Review changes">
        <button type="button">Confirm</button>
      </Dialog>,
    );
    expect(screen.getByRole('dialog', { hidden: true })).toHaveAttribute('data-open', 'false');
    expect(trigger).not.toHaveFocus();
    fireEvent.transitionEnd(dialog, { propertyName: 'transform' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    trigger.remove();
  });

  it('reverses a close when reopened before transition end', () => {
    const trigger = document.createElement('button');
    trigger.textContent = 'Open';
    document.body.append(trigger);
    trigger.focus();
    const onOpenChange = vi.fn();
    const view = render(
      <Dialog open onOpenChange={onOpenChange} title="Review changes">
        <button type="button">Confirm</button>
      </Dialog>,
    );
    act(() => frames.shift()?.(0));
    const dialog = screen.getByRole('dialog');

    view.rerender(
      <Dialog open={false} onOpenChange={onOpenChange} title="Review changes">
        <button type="button">Confirm</button>
      </Dialog>,
    );
    expect(trigger).not.toHaveFocus();
    view.rerender(
      <Dialog open onOpenChange={onOpenChange} title="Review changes">
        <button type="button">Confirm</button>
      </Dialog>,
    );
    act(() => frames.shift()?.(16));
    fireEvent.transitionEnd(dialog, { propertyName: 'transform' });

    expect(screen.getByRole('dialog')).toHaveAttribute('data-open', 'true');
    expect(trigger).not.toHaveFocus();
    trigger.remove();
  });

  it('does not steal focus from a replacement surface when unmounted', () => {
    const trigger = document.createElement('button');
    trigger.textContent = 'Open';
    const replacement = document.createElement('button');
    replacement.textContent = 'Replacement';
    document.body.append(trigger, replacement);
    trigger.focus();

    const view = render(
      <Dialog open onOpenChange={vi.fn()} title="Review changes">
        <button type="button">Confirm</button>
      </Dialog>,
    );
    act(() => frames.shift()?.(0));
    replacement.focus();
    view.unmount();

    expect(replacement).toHaveFocus();
    trigger.remove();
    replacement.remove();
  });

  it('retains the last open title, body, and footer until exit completes', () => {
    const view = render(
      <Dialog
        open
        onOpenChange={vi.fn()}
        title="Retained title"
        footer={<button type="button">Retained action</button>}
      >
        <p>Retained body</p>
      </Dialog>,
    );
    act(() => frames.shift()?.(0));
    const dialog = screen.getByRole('dialog');

    view.rerender(
      <Dialog open={false} onOpenChange={vi.fn()}>
        {null}
      </Dialog>,
    );

    const exitingDialog = screen.getByRole('dialog', { hidden: true });
    expect(within(exitingDialog).getByText('Retained title')).toBeInTheDocument();
    expect(within(exitingDialog).getByText('Retained body')).toBeInTheDocument();
    expect(
      within(exitingDialog).getByRole('button', { name: 'Retained action', hidden: true }),
    ).toBeInTheDocument();

    fireEvent.transitionEnd(dialog, { propertyName: 'transform' });
    expect(dialog).not.toBeInTheDocument();
  });
});

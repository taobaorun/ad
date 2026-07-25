import { act, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { AgentConversionProgress } from '@/components/AgentConversionProgress';
import i18n from '@/i18n';

describe('AgentConversionProgress', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('updates elapsed time while the preview remains active', async () => {
    await i18n.changeLanguage('en');
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-07-20T12:00:00Z'));
    render(
      <AgentConversionProgress
        progress={{
          phase: 'verifying_inherited_plugins',
          current: 2,
          total: 9,
          item: 'browser@openai-bundled',
        }}
        startedAt={Date.now()}
      />,
    );

    expect(screen.getByText('0s')).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(2000);
    });

    expect(screen.getByText('2s')).toBeInTheDocument();
  });
});

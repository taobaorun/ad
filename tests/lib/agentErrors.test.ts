import { beforeEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import { formatAgentError, formatAgentErrorMessage } from '@/lib/agentErrors';

describe('formatAgentError', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
  });

  it('maps structured Agent errors by code', () => {
    expect(
      formatAgentError({
        code: 'resource_changed',
        message: 'digest mismatch at a private path',
        installationId: 'codex:default',
        retryable: true,
      }),
    ).toBe('The resource changed after preview. Preview the operation again.');
  });

  it('preserves actionable diagnostics from conversion preview failures', () => {
    expect(
      formatAgentError({
        code: 'invalid_plan',
        message: 'Enabled base Plugin browser@openai-bundled marketplace source is unavailable',
        installationId: 'codex:default',
        retryable: false,
        details: { phase: 'conversion_preview' },
      }),
    ).toBe(
      'Conversion preview failed: Enabled base Plugin browser@openai-bundled marketplace source is unavailable',
    );
  });

  it('preserves ordinary Error messages', () => {
    expect(formatAgentError(new Error('Network unavailable'))).toBe('Network unavailable');
  });
});

describe('formatAgentErrorMessage', () => {
  it('preserves actionable messages from structured terminal errors', () => {
    expect(
      formatAgentErrorMessage({
        code: 'io',
        message: 'Access denied. Fix: switch cmux Socket Control to Automation mode.',
        installationId: 'codex:default',
        retryable: false,
      }),
    ).toBe('Access denied. Fix: switch cmux Socket Control to Automation mode.');
  });
});

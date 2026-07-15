import { beforeEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import { formatAgentError } from '@/lib/agentErrors';

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

  it('preserves ordinary Error messages', () => {
    expect(formatAgentError(new Error('Network unavailable'))).toBe('Network unavailable');
  });
});

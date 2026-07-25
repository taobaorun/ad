import i18n from '@/i18n';
import { AgentErrorSchema } from '@/lib/agentTypes';

export function formatAgentError(error: unknown): string {
  const parsed = AgentErrorSchema.safeParse(error);
  if (parsed.success) {
    if (hasConversionPreviewPhase(parsed.data.details)) {
      return i18n.t('agentErrors.conversion_preview', { message: parsed.data.message });
    }
    return i18n.t(`agentErrors.${parsed.data.code}`);
  }
  return formatAgentErrorMessage(error);
}

export function formatAgentErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (hasMessage(error)) return error.message;
  return String(error);
}

function hasConversionPreviewPhase(details: unknown): boolean {
  return (
    typeof details === 'object' &&
    details !== null &&
    'phase' in details &&
    details.phase === 'conversion_preview'
  );
}

function hasMessage(error: unknown): error is { message: string } {
  return (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string'
  );
}

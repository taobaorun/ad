import i18n from '@/i18n';
import { AgentErrorSchema } from '@/lib/agentTypes';

export function formatAgentError(error: unknown): string {
  const parsed = AgentErrorSchema.safeParse(error);
  if (parsed.success) {
    return i18n.t(`agentErrors.${parsed.data.code}`);
  }
  if (error instanceof Error) return error.message;
  if (hasMessage(error)) return error.message;
  return String(error);
}

function hasMessage(error: unknown): error is { message: string } {
  return (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string'
  );
}

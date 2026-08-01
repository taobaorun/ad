import type { ConversionReport } from '@/lib/agentOperationReports';
import type { WorkspaceKey } from '@/lib/agentTypes';
import {
  createDetachedOperationTracker,
  type TrackedDetachedOperation,
} from '@/store/detachedOperations';
import { WORKSPACE_OPERATION_FINISHED_EVENT } from '@/store/workspaceOperations';

const tracker = createDetachedOperationTracker<ConversionReport>({
  mismatchMessage: 'Conversion returned a result for a different workspace',
  onFinished: dispatchFinished,
});

export type TrackedConversionOperation = TrackedDetachedOperation<ConversionReport>;
export const useConversionOperations = tracker.useOperations;

export function runDetachedConversionOperation(
  workspaceKey: WorkspaceKey,
  operationId: string,
  apply: () => Promise<ConversionReport>,
): Promise<ConversionReport> {
  return tracker.run(workspaceKey, operationId, apply);
}

export function clearConversionOperation(workspaceKey: WorkspaceKey): void {
  tracker.clear(workspaceKey);
}

export function resetConversionOperationTracker(): void {
  tracker.reset();
}

function dispatchFinished(workspaceKey: WorkspaceKey): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(
    new CustomEvent(WORKSPACE_OPERATION_FINISHED_EVENT, { detail: { workspaceKey } }),
  );
  window.dispatchEvent(new Event('ad:project-codex-runtime-changed'));
  window.dispatchEvent(new Event('ad:agent-workspace-changed'));
}

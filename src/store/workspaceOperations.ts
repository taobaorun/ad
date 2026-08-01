import type { WorkspaceOperationReport } from '@/lib/agentOperationReports';
import type { WorkspaceKey } from '@/lib/agentTypes';
import {
  createDetachedOperationTracker,
  type TrackedDetachedOperation,
} from '@/store/detachedOperations';

export const WORKSPACE_OPERATION_FINISHED_EVENT = 'ad:agent-operation-finished';

const tracker = createDetachedOperationTracker<WorkspaceOperationReport>({
  mismatchMessage: 'Workspace operation returned a result for a different workspace',
  onFinished: dispatchFinished,
});

export type TrackedWorkspaceOperation = TrackedDetachedOperation<WorkspaceOperationReport>;
export const useWorkspaceOperations = tracker.useOperations;

export function runDetachedWorkspaceOperation(
  workspaceKey: WorkspaceKey,
  operationId: string,
  apply: () => Promise<WorkspaceOperationReport>,
): Promise<WorkspaceOperationReport> {
  return tracker.run(workspaceKey, operationId, apply);
}

export function resetWorkspaceOperationTracker(): void {
  tracker.reset();
}

function dispatchFinished(workspaceKey: WorkspaceKey): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(
    new CustomEvent(WORKSPACE_OPERATION_FINISHED_EVENT, { detail: { workspaceKey } }),
  );
  window.dispatchEvent(new Event('ad:agent-workspace-changed'));
}

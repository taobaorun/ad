import { create } from 'zustand';

import { formatAgentError } from '@/lib/agentErrors';
import type { WorkspaceOperationReport } from '@/lib/agentOperationReports';
import type { WorkspaceKey } from '@/lib/agentTypes';

export const WORKSPACE_OPERATION_FINISHED_EVENT = 'ad:agent-operation-finished';

export type TrackedWorkspaceOperation = {
  operationId: string;
  status: 'applying' | 'complete' | 'partial_failure' | 'failed';
  startedAt: string;
  finishedAt?: string;
  report?: WorkspaceOperationReport;
  error?: string;
};

interface WorkspaceOperationsState {
  operations: Record<string, TrackedWorkspaceOperation>;
}

export const useWorkspaceOperations = create<WorkspaceOperationsState>(() => ({
  operations: {},
}));

const pending = new Map<string, Promise<WorkspaceOperationReport>>();

export function runDetachedWorkspaceOperation(
  workspaceKey: WorkspaceKey,
  operationId: string,
  apply: () => Promise<WorkspaceOperationReport>,
): Promise<WorkspaceOperationReport> {
  const existing = pending.get(workspaceKey);
  if (existing) return existing;

  const startedAt = new Date().toISOString();
  updateOperation(workspaceKey, {
    operationId,
    status: 'applying',
    startedAt,
  });
  const task = apply()
    .then((report) => {
      if (report.workspaceKey !== workspaceKey) {
        throw new Error('Workspace operation returned a result for a different workspace');
      }
      updateOperation(workspaceKey, {
        operationId,
        status: report.outcome === 'partial_failure' ? 'partial_failure' : 'complete',
        startedAt,
        finishedAt: new Date().toISOString(),
        report,
      });
      dispatchFinished(workspaceKey);
      return report;
    })
    .catch((error: unknown) => {
      updateOperation(workspaceKey, {
        operationId,
        status: 'failed',
        startedAt,
        finishedAt: new Date().toISOString(),
        error: formatAgentError(error),
      });
      dispatchFinished(workspaceKey);
      throw error;
    })
    .finally(() => pending.delete(workspaceKey));
  pending.set(workspaceKey, task);
  return task;
}

export function resetWorkspaceOperationTracker(): void {
  pending.clear();
  useWorkspaceOperations.setState({ operations: {} });
}

function updateOperation(workspaceKey: WorkspaceKey, operation: TrackedWorkspaceOperation): void {
  useWorkspaceOperations.setState((state) => ({
    operations: { ...state.operations, [workspaceKey]: operation },
  }));
}

function dispatchFinished(workspaceKey: WorkspaceKey): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(
    new CustomEvent(WORKSPACE_OPERATION_FINISHED_EVENT, { detail: { workspaceKey } }),
  );
  window.dispatchEvent(new Event('ad:agent-workspace-changed'));
}

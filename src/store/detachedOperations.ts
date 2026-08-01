import { create } from 'zustand';

import { formatAgentError } from '@/lib/agentErrors';
import type { WorkspaceKey, WorkspaceOperationOutcome } from '@/lib/agentTypes';

const MAX_COMPLETED_OPERATIONS = 32;

interface WorkspaceScopedReport {
  workspaceKey: WorkspaceKey;
  outcome: WorkspaceOperationOutcome;
}

export type TrackedDetachedOperation<TReport> = {
  operationId: string;
  status: 'applying' | 'complete' | 'partial_failure' | 'failed';
  startedAt: string;
  finishedAt?: string;
  report?: TReport;
  error?: string;
};

interface DetachedOperationsState<TReport> {
  operations: Record<string, TrackedDetachedOperation<TReport>>;
}

interface PendingOperation<TReport> {
  operationId: string;
  promise: Promise<TReport>;
}

interface DetachedOperationTrackerOptions {
  mismatchMessage: string;
  onFinished: (workspaceKey: WorkspaceKey) => void;
}

export class DetachedOperationInProgressError extends Error {
  constructor() {
    super('A different operation is already applying to this workspace');
    this.name = 'DetachedOperationInProgressError';
  }
}

export function createDetachedOperationTracker<TReport extends WorkspaceScopedReport>({
  mismatchMessage,
  onFinished,
}: DetachedOperationTrackerOptions) {
  const useOperations = create<DetachedOperationsState<TReport>>(() => ({ operations: {} }));
  const pending = new Map<string, PendingOperation<TReport>>();

  function run(
    workspaceKey: WorkspaceKey,
    operationId: string,
    apply: () => Promise<TReport>,
  ): Promise<TReport> {
    const existing = pending.get(workspaceKey);
    if (existing) {
      return existing.operationId === operationId
        ? existing.promise
        : Promise.reject(new DetachedOperationInProgressError());
    }

    const startedAt = new Date().toISOString();
    update(workspaceKey, { operationId, status: 'applying', startedAt });
    let task: Promise<TReport>;
    task = apply()
      .then((report) => {
        if (report.workspaceKey !== workspaceKey) throw new Error(mismatchMessage);
        update(workspaceKey, {
          operationId,
          status: report.outcome === 'partial_failure' ? 'partial_failure' : 'complete',
          startedAt,
          finishedAt: new Date().toISOString(),
          report,
        });
        onFinished(workspaceKey);
        return report;
      })
      .catch((error: unknown) => {
        update(workspaceKey, {
          operationId,
          status: 'failed',
          startedAt,
          finishedAt: new Date().toISOString(),
          error: formatAgentError(error),
        });
        onFinished(workspaceKey);
        throw error;
      })
      .finally(() => {
        if (pending.get(workspaceKey)?.promise === task) pending.delete(workspaceKey);
      });
    pending.set(workspaceKey, { operationId, promise: task });
    return task;
  }

  function clear(workspaceKey: WorkspaceKey): void {
    useOperations.setState((state) => {
      const operations = { ...state.operations };
      delete operations[workspaceKey];
      return { operations };
    });
  }

  function reset(): void {
    pending.clear();
    useOperations.setState({ operations: {} });
  }

  function update(workspaceKey: WorkspaceKey, operation: TrackedDetachedOperation<TReport>): void {
    useOperations.setState((state) => ({
      operations: pruneCompletedOperations({
        ...state.operations,
        [workspaceKey]: operation,
      }),
    }));
  }

  return { useOperations, run, clear, reset };
}

function pruneCompletedOperations<TReport>(
  operations: Record<string, TrackedDetachedOperation<TReport>>,
): Record<string, TrackedDetachedOperation<TReport>> {
  const completed = Object.entries(operations)
    .filter(([, operation]) => operation.status !== 'applying')
    .sort(([, left], [, right]) =>
      (left.finishedAt ?? left.startedAt).localeCompare(right.finishedAt ?? right.startedAt),
    );
  const overflow = completed.length - MAX_COMPLETED_OPERATIONS;
  if (overflow <= 0) return operations;
  for (const [workspaceKey] of completed.slice(0, overflow)) delete operations[workspaceKey];
  return operations;
}

import { describe, expect, it, vi } from 'vitest';

import { WorkspaceKeySchema, type WorkspaceOperationOutcome } from '@/lib/agentTypes';
import {
  createDetachedOperationTracker,
  DetachedOperationInProgressError,
} from '@/store/detachedOperations';

interface TestReport {
  workspaceKey: ReturnType<typeof WorkspaceKeySchema.parse>;
  outcome: WorkspaceOperationOutcome;
}

describe('detached operation tracker', () => {
  it('reuses the same operation and rejects a different plan for a busy workspace', async () => {
    const workspaceKey = WorkspaceKeySchema.parse('workspace:test');
    let resolveOperation!: (report: TestReport) => void;
    const operation = new Promise<TestReport>((resolve) => {
      resolveOperation = resolve;
    });
    const apply = vi.fn(() => operation);
    const tracker = createDetachedOperationTracker<TestReport>({
      mismatchMessage: 'wrong workspace',
      onFinished: vi.fn(),
    });

    const first = tracker.run(workspaceKey, 'plan-1', apply);

    expect(tracker.run(workspaceKey, 'plan-1', apply)).toBe(first);
    await expect(tracker.run(workspaceKey, 'plan-2', vi.fn())).rejects.toBeInstanceOf(
      DetachedOperationInProgressError,
    );
    expect(apply).toHaveBeenCalledOnce();

    resolveOperation({ workspaceKey, outcome: 'changed' });
    await expect(first).resolves.toMatchObject({ outcome: 'changed' });
  });

  it('retains only a bounded set of completed workspace results', async () => {
    const tracker = createDetachedOperationTracker<TestReport>({
      mismatchMessage: 'wrong workspace',
      onFinished: vi.fn(),
    });

    for (let index = 0; index < 33; index += 1) {
      const workspaceKey = WorkspaceKeySchema.parse(`workspace:test-${index}`);
      await tracker.run(workspaceKey, `plan-${index}`, async () => ({
        workspaceKey,
        outcome: 'changed',
      }));
    }

    expect(Object.keys(tracker.useOperations.getState().operations)).toHaveLength(32);
    expect(tracker.useOperations.getState().operations['workspace:test-0']).toBeUndefined();
  });
});

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ResourceRemovalPlan, SourceRemovalPlan } from '@/lib/resourceCatalogTypes';
import type { SkillCatalogPlanView } from '@/lib/skillCatalogTypes';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke,
  Channel: class Channel {},
}));

import { tauri } from '@/lib/tauri';

const resourcePlan: ResourceRemovalPlan = {
  planId: 'remove-resource-plan',
  resourceId: 'catalog-resource:sha256:review',
  resourceName: 'Review',
  expectedCatalogRevision: 2,
  affectedProjectCount: 0,
  affectedAgentCount: 0,
  installations: [],
  riskFingerprint: 'risk:remove-resource',
  expiresAt: '2026-08-31T09:00:00Z',
};

const sourcePlan: SourceRemovalPlan = {
  planId: 'remove-source-plan',
  sourceId: 'skill-source:11111111-1111-4111-8111-111111111111',
  sourceName: 'Team tools',
  expectedCatalogRevision: 2,
  affectedProjectCount: 0,
  affectedAgentCount: 0,
  resources: [],
  riskFingerprint: 'risk:remove-source',
  expiresAt: '2026-08-31T09:00:00Z',
};

const catalogPlan: SkillCatalogPlanView = {
  schemaVersion: 1,
  id: 'skill-catalog-plan:add',
  action: 'add',
  expectedCatalogRevision: 'sha256:catalog',
  sourceId: sourcePlan.sourceId,
  displayName: 'Team tools',
  applicability: 'applicable',
  blockingIssues: [],
  affectedResources: [],
  affectedWorkspaces: [],
  confirmationRequired: true,
  riskFingerprint: 'risk:add-source',
  expiresAt: '2026-08-31T09:00:00Z',
};

const updateCatalogPlan: SkillCatalogPlanView = {
  ...catalogPlan,
  id: 'skill-catalog-plan:update',
  action: 'update',
  riskFingerprint: 'risk:update-source',
};

const snapshot = {
  schemaVersion: 1,
  revision: 3,
  sources: {},
  resources: {},
  migratedFromSkillCatalog: false,
};

describe('resource Catalog Tauri refresh notifications', () => {
  beforeEach(() => {
    invoke.mockReset();
    vi.restoreAllMocks();
  });

  it('notifies project inventories after every successful Catalog lifecycle mutation', async () => {
    const dispatch = vi.spyOn(window, 'dispatchEvent');
    invoke
      .mockResolvedValueOnce({ outcome: 'changed', issues: [] })
      .mockResolvedValueOnce({ outcome: 'changed', issues: [] })
      .mockResolvedValueOnce({
        operationId: 'remove-resource-operation',
        resourceId: resourcePlan.resourceId,
        phase: 'complete',
        completed: 0,
        total: 0,
        installations: [],
      })
      .mockResolvedValueOnce({
        operationId: 'retry-resource-operation',
        resourceId: resourcePlan.resourceId,
        phase: 'complete',
        completed: 0,
        total: 0,
        installations: [],
      })
      .mockResolvedValueOnce({
        operationId: 'partial-resource-operation',
        resourceId: resourcePlan.resourceId,
        phase: 'partial_failure',
        completed: 1,
        total: 2,
        installations: [
          {
            installationId: 'installation:succeeded',
            workspaceKey: 'workspace:succeeded',
            agentId: 'codex',
            projectPath: '/work/succeeded',
            state: 'succeeded',
          },
          {
            installationId: 'installation:failed',
            workspaceKey: 'workspace:failed',
            agentId: 'codex',
            projectPath: '/work/failed',
            state: 'failed',
            diagnosticCode: 'uninstall_failed',
          },
        ],
      })
      .mockImplementationOnce(
        async (
          _command: string,
          args: { onProgress: { onmessage?: (message: unknown) => void } },
        ) => {
          args.onProgress.onmessage?.({
            operationId: 'partial-retry-operation',
            sequence: 2,
            phase: 'uninstalling',
            completed: 2,
            total: 3,
            item: {
              installationId: 'installation:new-success',
              workspaceKey: 'workspace:new-success',
              agentId: 'codex',
              projectPath: '/work/new-success',
              state: 'succeeded',
            },
          });
          return {
            operationId: 'partial-retry-operation',
            resourceId: resourcePlan.resourceId,
            phase: 'partial_failure',
            completed: 2,
            total: 3,
            installations: [
              {
                installationId: 'installation:historical-success',
                workspaceKey: 'workspace:historical-success',
                agentId: 'codex',
                projectPath: '/work/historical-success',
                state: 'succeeded',
              },
              {
                installationId: 'installation:new-success',
                workspaceKey: 'workspace:new-success',
                agentId: 'codex',
                projectPath: '/work/new-success',
                state: 'succeeded',
              },
              {
                installationId: 'installation:still-failed',
                workspaceKey: 'workspace:still-failed',
                agentId: 'codex',
                projectPath: '/work/still-failed',
                state: 'failed',
                diagnosticCode: 'uninstall_failed',
              },
            ],
          };
        },
      )
      .mockResolvedValueOnce(snapshot)
      .mockResolvedValueOnce({
        operationId: 'remove-source-operation',
        sourceId: sourcePlan.sourceId,
        phase: 'complete',
        completed: 0,
        total: 0,
        resources: [],
      })
      .mockResolvedValueOnce({
        operationId: 'partial-source-operation',
        sourceId: sourcePlan.sourceId,
        phase: 'partial_failure',
        completed: 1,
        total: 2,
        resources: [
          {
            resourceId: resourcePlan.resourceId,
            resourceName: resourcePlan.resourceName,
            kind: 'skills',
            affectedProjectCount: 1,
            affectedAgentCount: 1,
            state: 'succeeded',
          },
          {
            resourceId: 'catalog-resource:sha256:failed',
            resourceName: 'Failed Plugin',
            kind: 'plugins',
            affectedProjectCount: 1,
            affectedAgentCount: 1,
            state: 'failed',
            diagnosticCode: 'uninstall_failed',
          },
        ],
      });

    await expectOneWorkspaceNotification(dispatch, () =>
      tauri.applySkillCatalogSourcePlan(catalogPlan),
    );
    await expectOneWorkspaceNotification(dispatch, () =>
      tauri.applySkillCatalogSourcePlan(updateCatalogPlan),
    );
    await expectOneWorkspaceNotification(dispatch, () =>
      tauri.applyRemoveCatalogResource(resourcePlan, () => {}),
    );
    await expectOneWorkspaceNotification(dispatch, () =>
      tauri.retryRemoveCatalogResource('retry-resource-operation', () => {}),
    );
    await expectOneWorkspaceNotification(dispatch, () =>
      tauri.applyRemoveCatalogResource(resourcePlan, () => {}),
    );
    await expectOneWorkspaceNotification(dispatch, () =>
      tauri.retryRemoveCatalogResource('partial-retry-operation', () => {}),
    );
    await expectOneWorkspaceNotification(dispatch, () =>
      tauri.readdCatalogResource(resourcePlan.resourceId),
    );
    await expectOneWorkspaceNotification(dispatch, () =>
      tauri.applyRemoveCatalogSource(sourcePlan, () => {}),
    );
    await expectOneWorkspaceNotification(dispatch, () =>
      tauri.applyRemoveCatalogSource(sourcePlan, () => {}),
    );
  });

  it('does not notify for no-change, compensated, or fully failed mutations', async () => {
    const dispatch = vi.spyOn(window, 'dispatchEvent');
    invoke
      .mockResolvedValueOnce({ outcome: 'no_change', issues: [] })
      .mockResolvedValueOnce({ outcome: 'compensated', issues: ['compensated'] })
      .mockResolvedValueOnce({
        operationId: 'failed-resource-operation',
        resourceId: resourcePlan.resourceId,
        phase: 'partial_failure',
        completed: 0,
        total: 1,
        installations: [
          {
            installationId: 'installation:one',
            workspaceKey: 'workspace:one',
            agentId: 'codex',
            projectPath: '/work/one',
            state: 'failed',
            diagnosticCode: 'uninstall_failed',
          },
        ],
      })
      .mockResolvedValueOnce({
        operationId: 'failed-retry-operation',
        resourceId: resourcePlan.resourceId,
        phase: 'partial_failure',
        completed: 1,
        total: 2,
        installations: [
          {
            installationId: 'installation:historical-success',
            workspaceKey: 'workspace:historical-success',
            agentId: 'codex',
            projectPath: '/work/historical-success',
            state: 'succeeded',
          },
          {
            installationId: 'installation:retry',
            workspaceKey: 'workspace:retry',
            agentId: 'codex',
            projectPath: '/work/retry',
            state: 'failed',
            diagnosticCode: 'uninstall_failed',
          },
        ],
      })
      .mockResolvedValueOnce({
        operationId: 'failed-source-operation',
        sourceId: sourcePlan.sourceId,
        phase: 'partial_failure',
        completed: 0,
        total: 1,
        resources: [
          {
            resourceId: resourcePlan.resourceId,
            resourceName: resourcePlan.resourceName,
            kind: 'skills',
            affectedProjectCount: 1,
            affectedAgentCount: 1,
            state: 'failed',
            diagnosticCode: 'uninstall_failed',
          },
        ],
      });

    await expectNoWorkspaceNotification(dispatch, () =>
      tauri.applySkillCatalogSourcePlan(catalogPlan),
    );
    await expectNoWorkspaceNotification(dispatch, () =>
      tauri.applySkillCatalogSourcePlan(updateCatalogPlan),
    );
    await expectNoWorkspaceNotification(dispatch, () =>
      tauri.applyRemoveCatalogResource(resourcePlan, () => {}),
    );
    await expectNoWorkspaceNotification(dispatch, () =>
      tauri.retryRemoveCatalogResource('failed-retry-operation', () => {}),
    );
    await expectNoWorkspaceNotification(dispatch, () =>
      tauri.applyRemoveCatalogSource(sourcePlan, () => {}),
    );
  });

  it('does not notify when any Catalog mutation command is rejected', async () => {
    const dispatch = vi.spyOn(window, 'dispatchEvent');
    const mutations = [
      () => tauri.applySkillCatalogSourcePlan(catalogPlan),
      () => tauri.applyRemoveCatalogResource(resourcePlan, () => {}),
      () => tauri.retryRemoveCatalogResource('rejected-retry-operation', () => {}),
      () => tauri.readdCatalogResource(resourcePlan.resourceId),
      () => tauri.applyRemoveCatalogSource(sourcePlan, () => {}),
    ];

    for (const mutation of mutations) {
      invoke.mockRejectedValueOnce(new Error('command rejected'));
      const before = dispatch.mock.calls.length;
      await expect(mutation()).rejects.toThrow('command rejected');
      expect(dispatch).toHaveBeenCalledTimes(before);
    }
  });
});

async function expectOneWorkspaceNotification(
  dispatch: { mock: { calls: Array<[Event]> } },
  mutation: () => Promise<unknown>,
): Promise<void> {
  const before = dispatch.mock.calls.length;
  await mutation();
  expect(dispatch).toHaveBeenCalledTimes(before + 1);
  expect(dispatch.mock.calls.at(-1)?.[0]).toEqual(
    expect.objectContaining({ type: 'ad:agent-workspace-changed' }),
  );
}

async function expectNoWorkspaceNotification(
  dispatch: { mock: { calls: Array<[Event]> } },
  mutation: () => Promise<unknown>,
): Promise<void> {
  const before = dispatch.mock.calls.length;
  await mutation();
  expect(dispatch).toHaveBeenCalledTimes(before);
}

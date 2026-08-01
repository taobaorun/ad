import { describe, expect, it } from 'vitest';
import {
  AgentContextSchema,
  AgentErrorSchema,
  AgentInstallationSchema,
  AgentProfileRefSchema,
  CapabilityDescriptorSchema,
  CapabilitySchema,
  ConversionProgressEventSchema,
  ConversionRoutePreviewSchema,
  MutationPlanViewSchema,
  OperationHistoryEntrySchema,
  OperationReceiptSchema,
  ProcessObservationSchema,
  ProjectCodexRuntimeStatusSchema,
  ResourceSnapshotSchema,
  SettingsDocumentSchema,
  parseAgentInstallation,
} from '@/lib/agentTypes';
import {
  CollectionResourceInventorySchema,
  CollectionResourceViewSchema,
} from '@/lib/agentResourceInventoryTypes';
import { WorkspaceOperationReportSchema } from '@/lib/agentOperationReports';
import { WorkspaceDescriptorSchema } from '@/lib/agentWorkspaceTypes';

describe('Agent schemas', () => {
  it('accepts a canonical installation at the IPC boundary', () => {
    expect(
      AgentInstallationSchema.parse({
        id: 'codex:default',
        agentId: 'codex',
        rootPath: '/Users/test/.codex',
      }),
    ).toEqual({
      id: 'codex:default',
      agentId: 'codex',
      rootPath: '/Users/test/.codex',
    });
  });

  it('keeps profile identity composite across Agents', () => {
    const claude = AgentProfileRefSchema.parse({ agentId: 'claude-code', profileId: 'default' });
    const codex = AgentProfileRefSchema.parse({ agentId: 'codex', profileId: 'default' });

    expect(claude).not.toEqual(codex);
  });

  it('uses the same capability names as the Rust boundary', () => {
    expect(CapabilitySchema.parse('process_detection')).toBe('process_detection');
  });

  it('returns validation issues for an empty Agent id', () => {
    const result = parseAgentInstallation({
      id: 'invalid',
      agentId: '',
      rootPath: '/tmp/agent',
    });

    expect(result.ok).toBe(false);
  });

  it('validates an installation-scoped project context', () => {
    expect(
      AgentContextSchema.parse({
        installationId: 'codex:default',
        projectPath: '/Users/test/project',
      }),
    ).toEqual({
      installationId: 'codex:default',
      projectPath: '/Users/test/project',
    });
  });

  it('requires a backend-owned workspace identity and rejects unknown fields', () => {
    const workspace = {
      schemaVersion: 1,
      key: 'workspace:sha256:test',
      revision: 'workspace-revision:sha256:test',
      agentId: 'codex',
      canonicalProjectPath: '/Users/test/project',
      baseInstallationId: 'codex:default',
      effectiveInstallationId: 'codex:default',
    };

    expect(WorkspaceDescriptorSchema.parse(workspace).key).toBe('workspace:sha256:test');
    expect(
      WorkspaceDescriptorSchema.safeParse({ ...workspace, physicalPath: '/private/target' })
        .success,
    ).toBe(false);
    const { key: _key, ...withoutKey } = workspace;
    expect(WorkspaceDescriptorSchema.safeParse(withoutKey).success).toBe(false);
  });

  it('keeps effective identity separate from declarations and physical targets', () => {
    const resource = CollectionResourceViewSchema.parse({
      key: 'resource:sha256:team-review',
      kind: 'skills',
      logicalId: 'review',
      displayName: 'Review',
      effectiveState: 'enabled',
      provenance: {
        winner: 'declaration:sha256:project-review',
        declarations: [
          {
            key: 'declaration:sha256:user-review',
            layer: 'user',
            sourceId: 'catalog:personal',
            targetId: 'target:sha256:user-review',
          },
          {
            key: 'declaration:sha256:project-review',
            layer: 'project',
            sourceId: 'catalog:team',
            targetId: 'target:sha256:project-review',
          },
        ],
      },
      ownership: { kind: 'ad_managed', recordId: 'ownership:review' },
      health: { status: 'healthy' },
      management: {
        status: 'managed',
        actions: [
          { action: 'disable', availability: 'available' },
          {
            action: 'remove',
            availability: 'unavailable',
            limitation: { code: 'inherited', messageKey: 'agents.resources.inherited' },
          },
        ],
      },
    });

    expect(resource.provenance.winner).toBe('declaration:sha256:project-review');
    expect(resource.provenance.declarations[0]?.targetId).not.toBe(
      resource.provenance.declarations[1]?.targetId,
    );
    expect(
      CollectionResourceViewSchema.safeParse({
        ...resource,
        physicalPath: '/Users/test/project/.agents/skills/review',
      }).success,
    ).toBe(false);
  });

  it('preserves partial coverage and item diagnostics without hiding valid peers', () => {
    const item = CollectionResourceViewSchema.parse({
      key: 'resource:sha256:healthy',
      kind: 'plugins',
      logicalId: 'healthy',
      displayName: 'Healthy',
      effectiveState: 'enabled',
      provenance: { declarations: [] },
      ownership: { kind: 'external' },
      health: { status: 'healthy' },
      management: { status: 'external', actions: [] },
    });
    const inventory = CollectionResourceInventorySchema.parse({
      workspaceKey: 'workspace:sha256:test',
      agentId: 'codex',
      kind: 'plugins',
      coverage: {
        status: 'partial',
        observed: 2,
        visible: 1,
        diagnostics: [
          {
            code: 'malformed_manifest',
            messageKey: 'agents.resources.malformedManifest',
            retryable: false,
          },
        ],
      },
      resources: [item],
    });

    expect(inventory.coverage.status).toBe('partial');
    expect(inventory.resources).toHaveLength(1);
    expect(inventory.coverage.diagnostics).toHaveLength(1);
  });

  it('separates domain outcomes from the optional execution receipt', () => {
    const report = WorkspaceOperationReportSchema.parse({
      workspaceKey: 'workspace:sha256:test',
      outcome: 'unsupported',
      issues: [
        {
          code: 'external_plugin',
          messageKey: 'agents.resources.externalPlugin',
        },
      ],
    });

    expect(report.receipt).toBeUndefined();
    expect(report.outcome).toBe('unsupported');
  });

  it('rejects Agent-specific fields from resource snapshots', () => {
    const snapshot = {
      resource: {
        installationId: 'codex:default',
        projectPath: '/Users/test/project',
        kind: 'settings',
        scope: 'project',
        logicalId: 'project-config',
      },
      location: {
        path: '/Users/test/project/.codex/config.toml',
        origin: 'project',
      },
      mediaType: 'application/toml',
      content: 'model = "gpt-5.4"\n',
      digest: 'sha256:abc',
      observedAt: '2026-07-15T01:00:00Z',
      codexConfig: { model: 'gpt-5.4' },
    };

    expect(ResourceSnapshotSchema.safeParse(snapshot).success).toBe(false);
    delete (snapshot as Partial<typeof snapshot>).codexConfig;
    expect(ResourceSnapshotSchema.parse(snapshot).resource.kind).toBe('settings');
    const { content: _content, ...withoutContent } = snapshot;
    expect(ResourceSnapshotSchema.safeParse(withoutContent).success).toBe(false);
  });

  it('represents an editable settings target that does not exist yet', () => {
    const document = SettingsDocumentSchema.parse({
      resource: {
        installationId: 'codex:default',
        projectPath: '/Users/test/project',
        kind: 'settings',
        scope: 'project',
        logicalId: 'project-config',
      },
      location: {
        path: '/Users/test/project/.codex/config.toml',
        origin: 'project',
      },
      mediaType: 'application/toml',
      content: '',
      exists: false,
    });

    expect(document.exists).toBe(false);
    expect(document.digest).toBeUndefined();
  });

  it('validates capability descriptors derived from ports', () => {
    const descriptor = CapabilityDescriptorSchema.parse({
      kind: 'skills',
      scopes: ['user', 'project'],
      operations: ['list', 'install', 'enable', 'disable'],
      availability: 'degraded',
      limitations: [
        {
          code: 'project_install_pending',
          messageKey: 'agents.capabilities.projectInstallPending',
        },
      ],
    });

    expect(descriptor.operations).toContain('install');
  });

  it('validates installation-scoped process observations', () => {
    expect(
      ProcessObservationSchema.parse({
        pid: 42,
        installationId: 'codex:default',
        executable: 'codex',
        cwd: '/Users/test/project',
      }),
    ).toEqual({
      pid: 42,
      installationId: 'codex:default',
      executable: 'codex',
      cwd: '/Users/test/project',
    });
  });

  it('requires desired and applied Project Codex inheritance state', () => {
    const status = ProjectCodexRuntimeStatusSchema.parse({
      baseInstallationId: 'codex:default',
      runtimeInstallationId: 'codex:/Users/test/.ad/codex-homes/project',
      runtimeHome: '/Users/test/.ad/codex-homes/project',
      prepared: true,
      fresh: false,
      desiredInheritBaseConfig: false,
      appliedInheritBaseConfig: true,
      needsRefresh: true,
      pluginCount: 1,
      authMode: 'shared_file',
    });

    expect(status.needsRefresh).toBe(true);
    expect(
      ProjectCodexRuntimeStatusSchema.safeParse({
        ...status,
        desiredInheritBaseConfig: undefined,
      }).success,
    ).toBe(false);
  });

  it('keeps mutation content out of the frontend plan view', () => {
    const plan = {
      id: 'plan-1',
      agentId: 'codex',
      context: { installationId: 'codex:default' },
      changes: [
        {
          resource: {
            installationId: 'codex:default',
            kind: 'settings',
            scope: 'user',
            logicalId: 'user-config',
          },
          kind: 'replace',
          target: {
            id: 'target:sha256:user-config',
            kind: 'agent_resource',
            display: 'settings/user-config',
          },
          scope: 'user',
          dependencies: [],
          activationImpact: [],
        },
      ],
      riskFingerprint: 'risk:sha256:test',
      expiresAt: '2026-07-15T01:05:00Z',
    };

    expect(MutationPlanViewSchema.parse(plan).id).toBe('plan-1');
    expect(MutationPlanViewSchema.safeParse({ ...plan, targetContent: 'secret' }).success).toBe(
      false,
    );
    expect(
      MutationPlanViewSchema.safeParse({
        ...plan,
        changes: [
          {
            ...plan.changes[0],
            target: { kind: 'physical_path', path: '/private/secret' },
          },
        ],
      }).success,
    ).toBe(false);
  });

  it('validates partial receipts and structured errors', () => {
    expect(
      OperationReceiptSchema.parse({
        schemaVersion: 2,
        id: 'receipt-1',
        planId: 'plan-1',
        operationKind: 'apply',
        context: { installationId: 'codex:default' },
        status: 'partial_failure',
        appliedResources: [],
        backupPaths: ['/Users/test/.ad/backups/config.toml'],
        postApplyStates: [
          {
            resource: {
              installationId: 'codex:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-config',
            },
            kind: 'file',
            digest: 'sha256:after',
          },
        ],
        manifestDigest: 'sha256:manifest',
        rollback: { available: true },
        createdAt: '2026-07-15T01:00:00Z',
        message: 'A compensation write failed',
      }).status,
    ).toBe('partial_failure');

    expect(
      AgentErrorSchema.parse({
        code: 'resource_changed',
        message: 'The resource changed after preview',
        installationId: 'codex:default',
        retryable: true,
      }).retryable,
    ).toBe(true);
  });

  it('accepts directory states in operation receipts', () => {
    const receipt = OperationReceiptSchema.parse({
      schemaVersion: 2,
      id: 'receipt-directory',
      planId: 'plan-directory',
      operationKind: 'apply',
      context: {
        installationId: 'codex:project',
        projectPath: '/Users/test/project',
      },
      status: 'complete',
      appliedResources: [],
      backupPaths: [],
      postApplyStates: [
        {
          resource: {
            installationId: 'codex:project',
            projectPath: '/Users/test/project',
            kind: 'plugins',
            scope: 'project',
            logicalId: 'marketplace:team',
          },
          kind: 'directory',
          digest: 'sha256:directory',
        },
      ],
      rollback: { available: true },
      createdAt: '2026-07-15T01:00:00Z',
    });

    expect(receipt.postApplyStates[0]?.kind).toBe('directory');
  });

  it('validates operation history entries at the IPC boundary', () => {
    const entry = OperationHistoryEntrySchema.parse({
      createdAt: '2026-07-15T01:00:00Z',
      receipt: {
        schemaVersion: 2,
        id: 'receipt-1',
        planId: 'plan-1',
        operationKind: 'apply',
        context: {
          installationId: 'codex:default',
        },
        status: 'complete',
        appliedResources: [],
        backupPaths: [],
        postApplyStates: [],
        rollback: { available: false, reason: 'missing_evidence' },
        createdAt: '2026-07-15T01:00:00Z',
      },
    });

    expect(entry.receipt?.id).toBe('receipt-1');
  });

  it('validates artifact conversion previews without mutation content', () => {
    const preview = ConversionRoutePreviewSchema.parse({
      sourceAgentId: 'claude-code',
      targetAgentId: 'codex',
      artifacts: [
        {
          id: 'user-settings:model',
          kind: 'settings',
          source: {
            resource: {
              installationId: 'claude-code:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-settings',
            },
            location: { path: '/Users/test/.claude/settings.json', origin: 'user' },
          },
          target: {
            resource: {
              installationId: 'codex:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-config',
            },
            location: { path: '/Users/test/.codex/config.toml', origin: 'user' },
          },
          disposition: 'mapped',
          risk: 'confirmation',
          message: 'Model key maps to Codex',
        },
      ],
      summary: {
        total: 1,
        automatic: 1,
        requiresInput: 0,
        unsupported: 0,
        conflicts: 0,
        unchanged: 0,
        dangerous: 0,
      },
    });

    expect(preview.artifacts.at(0)?.disposition).toBe('mapped');
    expect(preview.plan).toBeUndefined();
  });

  it('validates conversion progress without accepting backend-only details', () => {
    const progress = ConversionProgressEventSchema.parse({
      phase: 'verifying_inherited_plugins',
      current: 2,
      total: 9,
      item: 'browser@openai-bundled',
    });

    expect(progress.item).toBe('browser@openai-bundled');
    expect(
      ConversionProgressEventSchema.safeParse({
        ...progress,
        mutationContent: 'private',
      }).success,
    ).toBe(false);
  });
});

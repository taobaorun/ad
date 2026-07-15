import { describe, expect, it } from 'vitest';
import {
  AgentContextSchema,
  AgentErrorSchema,
  AgentInstallationSchema,
  AgentProfileRefSchema,
  CapabilityDescriptorSchema,
  CapabilitySchema,
  ConversionRoutePreviewSchema,
  MutationPlanViewSchema,
  OperationReceiptSchema,
  ResourceSnapshotSchema,
  parseAgentInstallation,
} from '@/lib/agentTypes';

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
        },
      ],
      expiresAt: '2026-07-15T01:05:00Z',
    };

    expect(MutationPlanViewSchema.parse(plan).id).toBe('plan-1');
    expect(MutationPlanViewSchema.safeParse({ ...plan, targetContent: 'secret' }).success).toBe(
      false,
    );
  });

  it('validates partial receipts and structured errors', () => {
    expect(
      OperationReceiptSchema.parse({
        id: 'receipt-1',
        planId: 'plan-1',
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

  it('validates artifact conversion previews without mutation content', () => {
    const preview = ConversionRoutePreviewSchema.parse({
      sourceAgentId: 'claude-code',
      targetAgentId: 'codex',
      artifacts: [
        {
          id: 'user-settings:model',
          kind: 'settings',
          source: {
            installationId: 'claude-code:default',
            kind: 'settings',
            scope: 'user',
            logicalId: 'user-settings',
          },
          target: {
            installationId: 'codex:default',
            kind: 'settings',
            scope: 'user',
            logicalId: 'user-config',
          },
          disposition: 'mapped',
          message: 'Model key maps to Codex',
        },
      ],
    });

    expect(preview.artifacts.at(0)?.disposition).toBe('mapped');
    expect(preview.plan).toBeUndefined();
  });
});

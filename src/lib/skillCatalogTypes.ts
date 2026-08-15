import { z } from 'zod';
import { ResourceRefSchema, WorkspaceKeySchema } from '@/lib/agentTypes';

const contentDigestSchema = z.string().min(1);

export const SkillActivationImpactSchema = z
  .object({
    instructions: z.array(z.string()),
    hooks: z.array(z.string()),
    mcp: z.array(z.string()),
    commands: z.array(z.string()),
    scripts: z.array(z.string()),
    binaries: z.array(z.string()),
    executablePaths: z.array(z.string()),
    digest: contentDigestSchema,
  })
  .strict();

export const SkillArtifactRefSchema = z
  .object({
    schemaVersion: z.literal(1),
    artifactId: z.string().min(1),
    sourceId: z.string().min(1),
    sourceRevision: z.string().min(1),
    treeDigest: contentDigestSchema,
    manifestDigest: contentDigestSchema,
    skills: z.array(
      z
        .object({
          logicalId: z.string().min(1),
          subpath: z.string(),
          instructionDigest: contentDigestSchema,
        })
        .strict(),
    ),
    activationImpact: SkillActivationImpactSchema,
  })
  .strict();

const SourceResourceItemSchema = z
  .object({
    kind: z.enum(['skills', 'plugins']),
    installId: z.string().min(1),
    displayName: z.string().min(1),
    description: z.string().optional(),
    subpath: z.string(),
    descriptorDigest: contentDigestSchema,
    supportedAgents: z.array(z.enum(['claude-code', 'codex'])).default([]),
  })
  .strict();

export const SkillSourceBindingSchema = z
  .object({
    schemaVersion: z.union([z.literal(2), z.literal(3)]),
    bindingId: z.string().startsWith('skill-source-binding:'),
    sourceId: z.string().startsWith('skill-source:'),
    sourceType: z.enum(['git', 'local']),
    sourceRevision: z.string().min(1),
    stableRoot: z.string().min(1),
    physicalRoot: z.string().min(1),
    treeDigest: contentDigestSchema,
    manifestDigest: contentDigestSchema,
    skills: SkillArtifactRefSchema.shape.skills,
    resources: z.array(SourceResourceItemSchema).default([]),
    activationImpact: SkillActivationImpactSchema,
  })
  .strict();

export const SkillCatalogEntrySchema = z
  .object({
    sourceId: z.string().startsWith('skill-source:'),
    displayName: z.string().min(1),
    sourceType: z.enum(['git', 'local']),
    location: z.string().min(1),
    branch: z.string().optional(),
    subdirectory: z.string().optional(),
    autoUpdate: z.boolean(),
    currentArtifact: SkillArtifactRefSchema.optional(),
    currentBinding: SkillSourceBindingSchema.optional(),
    addedAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
  })
  .strict()
  .refine(
    (entry) =>
      Number(entry.currentArtifact !== undefined) + Number(entry.currentBinding !== undefined) ===
      1,
    'catalog entry must contain exactly one source payload',
  );

export const SkillCatalogSnapshotSchema = z
  .object({
    schemaVersion: z.union([z.literal(1), z.literal(2)]),
    revision: contentDigestSchema,
    entries: z.array(SkillCatalogEntrySchema),
  })
  .strict();

export const SkillCatalogPlanViewSchema = z
  .object({
    schemaVersion: z.literal(1),
    id: z.string().min(1),
    action: z.enum(['add', 'update', 'remove']),
    expectedCatalogRevision: contentDigestSchema,
    sourceId: z.string().startsWith('skill-source:'),
    displayName: z.string().min(1),
    artifact: SkillArtifactRefSchema.optional(),
    binding: SkillSourceBindingSchema.optional(),
    currentArtifact: SkillArtifactRefSchema.optional(),
    currentBinding: SkillSourceBindingSchema.optional(),
    rollbackOf: z.string().optional(),
    applicability: z.enum(['applicable', 'blocked']),
    blockingIssues: z
      .array(
        z
          .object({
            code: z.string().min(1),
            message: z.string().min(1),
            resource: ResourceRefSchema.optional(),
          })
          .strict(),
      )
      .default([]),
    affectedResources: z.array(ResourceRefSchema).default([]),
    affectedWorkspaces: z.array(WorkspaceKeySchema).default([]),
    confirmationRequired: z.boolean(),
    riskFingerprint: z.string().min(1),
    expiresAt: z.string().datetime(),
  })
  .strict();

export const SkillCatalogReceiptSchema = z
  .object({
    schemaVersion: z.union([z.literal(1), z.literal(2)]),
    id: z.string().min(1),
    planId: z.string().min(1),
    action: z.enum(['add', 'update', 'remove']),
    sourceId: z.string().startsWith('skill-source:'),
    beforeCatalogRevision: contentDigestSchema,
    afterCatalogRevision: contentDigestSchema,
    artifact: SkillArtifactRefSchema.optional(),
    binding: SkillSourceBindingSchema.optional(),
    previousBinding: SkillSourceBindingSchema.optional(),
    rollbackOf: z.string().optional(),
    affectedResources: z.array(ResourceRefSchema).default([]),
    affectedWorkspaces: z.array(WorkspaceKeySchema).default([]),
    backupId: z.string().optional(),
    status: z.enum(['complete', 'compensated', 'recovered']),
    createdAt: z.string().datetime(),
  })
  .strict();

export const SkillCatalogOperationReportSchema = z
  .object({
    outcome: z.enum(['changed', 'no_change', 'compensated', 'partial_failure']),
    source: SkillCatalogEntrySchema.optional(),
    receipt: SkillCatalogReceiptSchema.optional(),
    issues: z.array(z.string()),
  })
  .strict();

export const SkillSourcePreviewProgressSchema = z
  .object({
    sequence: z.number().int().positive(),
    phase: z.enum(['preparing', 'cloning', 'inspecting', 'planning']),
  })
  .strict();

export interface SkillSourceRequest {
  displayName: string;
  sourceType: 'git' | 'local';
  location: string;
  branch?: string;
  subdirectory?: string;
  autoUpdate: boolean;
}

export type SkillCatalogEntry = z.infer<typeof SkillCatalogEntrySchema>;
export type SkillCatalogSnapshot = z.infer<typeof SkillCatalogSnapshotSchema>;
export type SkillCatalogPlanView = z.infer<typeof SkillCatalogPlanViewSchema>;
export type SkillCatalogOperationReport = z.infer<typeof SkillCatalogOperationReportSchema>;
export type SkillSourcePreviewProgress = z.infer<typeof SkillSourcePreviewProgressSchema>;

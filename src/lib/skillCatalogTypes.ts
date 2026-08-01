import { z } from 'zod';

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

export const SkillCatalogEntrySchema = z
  .object({
    sourceId: z.string().startsWith('skill-source:'),
    displayName: z.string().min(1),
    sourceType: z.enum(['git', 'local']),
    location: z.string().min(1),
    branch: z.string().optional(),
    subdirectory: z.string().optional(),
    autoUpdate: z.boolean(),
    currentArtifact: SkillArtifactRefSchema,
    addedAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
  })
  .strict();

export const SkillCatalogSnapshotSchema = z
  .object({
    schemaVersion: z.literal(1),
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
    currentArtifact: SkillArtifactRefSchema.optional(),
    confirmationRequired: z.boolean(),
    riskFingerprint: z.string().min(1),
    expiresAt: z.string().datetime(),
  })
  .strict();

export const SkillCatalogReceiptSchema = z
  .object({
    schemaVersion: z.literal(1),
    id: z.string().min(1),
    planId: z.string().min(1),
    action: z.enum(['add', 'update', 'remove']),
    sourceId: z.string().startsWith('skill-source:'),
    beforeCatalogRevision: contentDigestSchema,
    afterCatalogRevision: contentDigestSchema,
    artifact: SkillArtifactRefSchema.optional(),
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

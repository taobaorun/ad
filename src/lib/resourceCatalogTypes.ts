import { z } from 'zod';

const contentDigestSchema = z.string().startsWith('sha256:');

export const CatalogSourceBindingSchema = z
  .object({
    bindingId: z.string().min(1),
    sourceRevision: z.string().min(1),
    stableRoot: z.string().min(1),
    physicalRoot: z.string().min(1),
    treeDigest: contentDigestSchema,
    manifestDigest: contentDigestSchema,
  })
  .strict();

export const CatalogSourceSchema = z
  .object({
    id: z.string().startsWith('skill-source:'),
    displayName: z.string().min(1),
    sourceType: z.enum(['git', 'local']),
    location: z.string().min(1),
    branch: z.string().optional(),
    subdirectory: z.string().optional(),
    binding: CatalogSourceBindingSchema.optional(),
    health: z.enum(['healthy', 'degraded']),
  })
  .strict();

export const CatalogResourceSchema = z
  .object({
    id: z.string().startsWith('catalog-resource:'),
    sourceId: z.string().startsWith('skill-source:'),
    kind: z.enum(['skills', 'plugins']),
    installId: z.string().min(1),
    displayName: z.string().min(1),
    description: z.string().optional(),
    subpath: z.string(),
    descriptorDigest: contentDigestSchema,
    compatibleAgents: z.array(z.enum(['claude-code', 'codex'])).default([]),
    present: z.boolean(),
    lifecycle: z.enum(['managed', 'suppressed']),
    suppressedAtRevision: z.number().int().nonnegative().optional(),
    lastDiagnosticCode: z.string().optional(),
  })
  .strict();

export const ResourceCatalogSnapshotSchema = z
  .object({
    schemaVersion: z.literal(1),
    revision: z.number().int().nonnegative(),
    sources: z.record(z.string(), CatalogSourceSchema),
    resources: z.record(z.string(), CatalogResourceSchema),
    migratedFromSkillCatalog: z.boolean(),
  })
  .strict();

export type CatalogSource = z.infer<typeof CatalogSourceSchema>;
export type CatalogResource = z.infer<typeof CatalogResourceSchema>;
export type ResourceCatalogSnapshot = z.infer<typeof ResourceCatalogSnapshotSchema>;

export const ResourceRemovalInstallationSchema = z
  .object({
    installationId: z.string().min(1),
    workspaceKey: z.string().min(1),
    agentId: z.string().min(1),
    projectPath: z.string().min(1).optional(),
    state: z.enum(['pending', 'running', 'succeeded', 'failed']),
    diagnosticCode: z.string().optional(),
  })
  .strict();

export const ResourceRemovalPlanSchema = z
  .object({
    planId: z.string().min(1),
    resourceId: z.string().min(1),
    resourceName: z.string().min(1),
    expectedCatalogRevision: z.number().int().nonnegative(),
    affectedProjectCount: z.number().int().nonnegative(),
    affectedAgentCount: z.number().int().nonnegative(),
    installations: z.array(ResourceRemovalInstallationSchema),
    riskFingerprint: z.string().min(1),
    expiresAt: z.string().datetime(),
  })
  .strict();

export const ResourceRemovalProgressSchema = z
  .object({
    operationId: z.string().min(1),
    sequence: z.number().int().positive(),
    phase: z.enum(['uninstalling', 'suppressing', 'complete', 'partial_failure']),
    completed: z.number().int().nonnegative(),
    total: z.number().int().nonnegative(),
    item: ResourceRemovalInstallationSchema.optional(),
  })
  .strict();

export const ResourceRemovalReportSchema = z
  .object({
    operationId: z.string().min(1),
    resourceId: z.string().min(1),
    phase: z.enum(['uninstalling', 'suppressing', 'complete', 'partial_failure']),
    completed: z.number().int().nonnegative(),
    total: z.number().int().nonnegative(),
    installations: z.array(ResourceRemovalInstallationSchema),
  })
  .strict();

export type ResourceRemovalPlan = z.infer<typeof ResourceRemovalPlanSchema>;
export type ResourceRemovalProgress = z.infer<typeof ResourceRemovalProgressSchema>;
export type ResourceRemovalReport = z.infer<typeof ResourceRemovalReportSchema>;

export const ResourceRemovalOperationSchema = z
  .object({
    schemaVersion: z.literal(1),
    operationId: z.string().min(1),
    resourceId: z.string().min(1),
    startedAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
    phase: z.enum(['uninstalling', 'suppressing', 'complete', 'partial_failure']),
    completed: z.number().int().nonnegative(),
    total: z.number().int().nonnegative(),
    installations: z.array(ResourceRemovalInstallationSchema),
  })
  .strict();

export type ResourceRemovalOperation = z.infer<typeof ResourceRemovalOperationSchema>;

export const SourceRemovalResourceSchema = z
  .object({
    resourceId: z.string().min(1),
    resourceName: z.string().min(1),
    kind: z.enum(['skills', 'plugins']),
    affectedProjectCount: z.number().int().nonnegative(),
    affectedAgentCount: z.number().int().nonnegative(),
    state: z.enum(['pending', 'running', 'succeeded', 'failed']),
    diagnosticCode: z.string().optional(),
  })
  .strict();

export const SourceRemovalPlanSchema = z
  .object({
    planId: z.string().min(1),
    sourceId: z.string().min(1),
    sourceName: z.string().min(1),
    expectedCatalogRevision: z.number().int().nonnegative(),
    affectedProjectCount: z.number().int().nonnegative(),
    affectedAgentCount: z.number().int().nonnegative(),
    resources: z.array(SourceRemovalResourceSchema),
    riskFingerprint: z.string().min(1),
    expiresAt: z.string().datetime(),
  })
  .strict();

export const SourceRemovalProgressSchema = z
  .object({
    operationId: z.string().min(1),
    sequence: z.number().int().positive(),
    phase: z.enum(['uninstalling', 'removing_source', 'complete', 'partial_failure']),
    completed: z.number().int().nonnegative(),
    total: z.number().int().nonnegative(),
    item: SourceRemovalResourceSchema.optional(),
  })
  .strict();

export const SourceRemovalReportSchema = z
  .object({
    operationId: z.string().min(1),
    sourceId: z.string().min(1),
    phase: z.enum(['uninstalling', 'removing_source', 'complete', 'partial_failure']),
    completed: z.number().int().nonnegative(),
    total: z.number().int().nonnegative(),
    resources: z.array(SourceRemovalResourceSchema),
  })
  .strict();

export type SourceRemovalPlan = z.infer<typeof SourceRemovalPlanSchema>;
export type SourceRemovalProgress = z.infer<typeof SourceRemovalProgressSchema>;
export type SourceRemovalReport = z.infer<typeof SourceRemovalReportSchema>;

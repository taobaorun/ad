import { z } from 'zod';

const ContentDigestSchema = z.string().min(1);
const ReceiptIdSchema = z.string().min(1);

export const LegacySkillMigrationStatusSchema = z.enum([
  'ready_to_archive',
  'needs_reconciliation',
  'blocked',
  'offline',
]);

export const LegacySkillLinkSchema = z.object({
  logicalId: z.string(),
  targetKind: z.enum([
    'managed_artifact',
    'legacy_checkout',
    'local_source',
    'external',
    'missing',
  ]),
  sourceId: z.string().optional(),
  migrationReceiptId: ReceiptIdSchema.optional(),
  health: z.enum(['ready', 'degraded', 'blocked']),
});

export const LegacyProjectSkillSchema = z.object({
  stateId: z.string(),
  stateDigest: ContentDigestSchema,
  projectPath: z.string(),
  canonicalProjectPath: z.string().optional(),
  online: z.boolean(),
  mode: z.enum(['allowlist', 'blocklist']),
  listedSkills: z.array(z.string()),
  missingSourceIds: z.array(z.string()),
  links: z.array(LegacySkillLinkSchema),
  health: z.enum(['ready', 'degraded', 'blocked']),
  migrationStatus: LegacySkillMigrationStatusSchema,
});

export const LegacyProjectSkillArchiveSchema = z.object({
  schemaVersion: z.literal(1),
  archiveId: z.string(),
  originalStateId: z.string(),
  projectPath: z.string(),
  canonicalProjectPath: z.string(),
  stateDigest: ContentDigestSchema,
  receiptId: ReceiptIdSchema,
  archivedAt: z.string(),
  status: z.enum(['archived', 'restored']),
});

export const LegacySkillInventorySchema = z.object({
  schemaVersion: z.literal(1),
  sources: z.array(
    z.object({
      legacyId: z.string(),
      sourceType: z.enum(['git', 'local']),
      displayLocation: z.string(),
      safeIdentity: z.boolean(),
      available: z.boolean(),
      health: z.enum(['ready', 'degraded', 'blocked']),
    }),
  ),
  projects: z.array(LegacyProjectSkillSchema),
  archives: z.array(LegacyProjectSkillArchiveSchema),
  diagnostics: z.array(
    z.object({
      code: z.string(),
      messageKey: z.string(),
      subjectId: z.string().optional(),
    }),
  ),
});

export const LegacySkillMigrationPlanViewSchema = z.object({
  schemaVersion: z.literal(1),
  id: z.string(),
  projectPath: z.string(),
  canonicalProjectPath: z.string(),
  stateId: z.string(),
  stateDigest: ContentDigestSchema,
  migrationReceiptIds: z.array(ReceiptIdSchema),
  confirmationRequired: z.boolean(),
  riskFingerprint: z.string(),
  expiresAt: z.string(),
});

export const LegacySkillMigrationReceiptSchema = z.object({
  schemaVersion: z.literal(1),
  id: ReceiptIdSchema,
  planId: z.string(),
  parentReceiptId: ReceiptIdSchema.optional(),
  archiveId: z.string(),
  originalStateId: z.string(),
  projectPath: z.string(),
  canonicalProjectPath: z.string(),
  stateDigest: ContentDigestSchema,
  migrationReceiptIds: z.array(ReceiptIdSchema),
  status: z.enum(['complete', 'recovered', 'compensated', 'restored']),
  createdAt: z.string(),
});

export const LegacySkillMigrationReportSchema = z.object({
  outcome: z.enum(['archived', 'restored', 'compensated']),
  receipt: LegacySkillMigrationReceiptSchema.optional(),
  issues: z.array(z.string()),
});

export type LegacySkillInventory = z.infer<typeof LegacySkillInventorySchema>;
export type LegacyProjectSkill = z.infer<typeof LegacyProjectSkillSchema>;
export type LegacyProjectSkillArchive = z.infer<typeof LegacyProjectSkillArchiveSchema>;
export type LegacySkillMigrationPlanView = z.infer<typeof LegacySkillMigrationPlanViewSchema>;
export type LegacySkillMigrationReport = z.infer<typeof LegacySkillMigrationReportSchema>;

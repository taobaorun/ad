import { z } from 'zod';

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export const JsonValueSchema: z.ZodType<JsonValue> = z.lazy(() =>
  z.union([
    z.null(),
    z.boolean(),
    z.number(),
    z.string(),
    z.array(JsonValueSchema),
    z.record(JsonValueSchema),
  ]),
);

const RequiredIdSchema = z.string().min(1, 'identifier is required');

export const AgentIdSchema = RequiredIdSchema.brand<'AgentId'>();
export const InstallationIdSchema = RequiredIdSchema.brand<'InstallationId'>();
export const ProfileIdSchema = RequiredIdSchema.brand<'ProfileId'>();
export const PlanIdSchema = RequiredIdSchema.brand<'PlanId'>();
export const ReceiptIdSchema = RequiredIdSchema.brand<'ReceiptId'>();
export const ContentDigestSchema = RequiredIdSchema.brand<'ContentDigest'>();

export const CapabilitySchema = z.enum([
  'settings',
  'skills',
  'plugins',
  'process_detection',
  'terminal_launch',
  'conversion',
]);

export const AgentMetadataSchema = z
  .object({
    id: AgentIdSchema,
    displayName: z.string().min(1, 'displayName is required'),
    capabilities: z.array(CapabilitySchema).default([]),
  })
  .strict();

export const AgentInstallationSchema = z
  .object({
    id: InstallationIdSchema,
    agentId: AgentIdSchema,
    rootPath: z.string().min(1, 'rootPath is required'),
  })
  .strict();

export const AgentContextSchema = z
  .object({
    installationId: InstallationIdSchema,
    projectPath: z.string().min(1).optional(),
  })
  .strict();

export const AgentProfileRefSchema = z
  .object({
    agentId: AgentIdSchema,
    profileId: ProfileIdSchema,
  })
  .strict();

export const ResourceKindSchema = z.enum([
  'settings',
  'instructions',
  'skills',
  'plugins',
  'hooks',
  'mcp',
  'agents',
  'rules',
]);

export const ResourceScopeSchema = z.enum(['user', 'project']);
export const ResourceOriginSchema = z.enum(['user', 'project', 'system']);

export const ResourceRefSchema = z
  .object({
    installationId: InstallationIdSchema,
    projectPath: z.string().min(1).optional(),
    kind: ResourceKindSchema,
    scope: ResourceScopeSchema,
    logicalId: z.string().min(1),
  })
  .strict();

export const ResourceLocationSchema = z
  .object({
    path: z.string().min(1),
    origin: ResourceOriginSchema,
  })
  .strict();

export const ResourceSnapshotSchema = z
  .object({
    resource: ResourceRefSchema,
    location: ResourceLocationSchema,
    mediaType: z.string().min(1),
    content: JsonValueSchema,
    digest: ContentDigestSchema,
    observedAt: z.string().datetime({ offset: true }),
  })
  .strict();

export const CapabilityKindSchema = z.enum([
  'settings',
  'skills',
  'plugins',
  'process_detection',
  'terminal_launch',
]);

export const CapabilityOperationSchema = z.enum([
  'inspect',
  'edit',
  'preview',
  'apply',
  'rollback',
  'list',
  'install',
  'enable',
  'disable',
  'detect',
  'launch',
]);

export const CapabilityAvailabilitySchema = z.enum(['available', 'degraded', 'unavailable']);

export const CapabilityLimitationSchema = z
  .object({
    code: z.string().min(1),
    messageKey: z.string().min(1),
  })
  .strict();

export const CapabilityDescriptorSchema = z
  .object({
    kind: CapabilityKindSchema,
    scopes: z.array(ResourceScopeSchema).default([]),
    operations: z.array(CapabilityOperationSchema).default([]),
    availability: CapabilityAvailabilitySchema,
    limitations: z.array(CapabilityLimitationSchema).default([]),
  })
  .strict();

export const MutationKindSchema = z.enum(['create', 'replace', 'delete']);

export const MutationPlanChangeViewSchema = z
  .object({
    resource: ResourceRefSchema,
    kind: MutationKindSchema,
  })
  .strict();

export const MutationPlanViewSchema = z
  .object({
    id: PlanIdSchema,
    agentId: AgentIdSchema,
    context: AgentContextSchema,
    changes: z.array(MutationPlanChangeViewSchema).default([]),
    expiresAt: z.string().datetime({ offset: true }),
  })
  .strict();

export const OperationStatusSchema = z.enum(['complete', 'compensated', 'partial_failure']);

export const OperationReceiptSchema = z
  .object({
    id: ReceiptIdSchema,
    planId: PlanIdSchema,
    status: OperationStatusSchema,
    appliedResources: z.array(ResourceRefSchema).default([]),
    backupPaths: z.array(z.string().min(1)).default([]),
    message: z.string().optional(),
  })
  .strict();

export const AgentErrorCodeSchema = z.enum([
  'invalid_plan',
  'resource_changed',
  'permission_denied',
  'unsupported',
  'plan_expired',
  'partial_failure',
  'io',
]);

export const AgentErrorSchema = z
  .object({
    code: AgentErrorCodeSchema,
    message: z.string().min(1),
    agentId: AgentIdSchema.optional(),
    installationId: InstallationIdSchema.optional(),
    resource: ResourceRefSchema.optional(),
    retryable: z.boolean().default(false),
    details: JsonValueSchema.optional(),
  })
  .strict();

export const ConversionIssueSchema = z
  .object({
    path: z.string(),
    kind: z.enum(['unsupported', 'requires_confirmation']),
    message: z.string(),
  })
  .strict();

export const ConversionPreviewSchema = z
  .object({
    sourceAgentId: AgentIdSchema,
    targetAgentId: AgentIdSchema,
    targetFormat: z.string(),
    targetContent: z.string(),
    issues: z.array(ConversionIssueSchema).default([]),
  })
  .strict();

export type AgentId = z.infer<typeof AgentIdSchema>;
export type InstallationId = z.infer<typeof InstallationIdSchema>;
export type ProfileId = z.infer<typeof ProfileIdSchema>;
export type PlanId = z.infer<typeof PlanIdSchema>;
export type ReceiptId = z.infer<typeof ReceiptIdSchema>;
export type ContentDigest = z.infer<typeof ContentDigestSchema>;
export type Capability = z.infer<typeof CapabilitySchema>;
export type AgentMetadata = z.infer<typeof AgentMetadataSchema>;
export type AgentInstallation = z.infer<typeof AgentInstallationSchema>;
export type AgentContext = z.infer<typeof AgentContextSchema>;
export type AgentProfileRef = z.infer<typeof AgentProfileRefSchema>;
export type ResourceKind = z.infer<typeof ResourceKindSchema>;
export type ResourceScope = z.infer<typeof ResourceScopeSchema>;
export type ResourceOrigin = z.infer<typeof ResourceOriginSchema>;
export type ResourceRef = z.infer<typeof ResourceRefSchema>;
export type ResourceLocation = z.infer<typeof ResourceLocationSchema>;
export type ResourceSnapshot = z.infer<typeof ResourceSnapshotSchema>;
export type CapabilityKind = z.infer<typeof CapabilityKindSchema>;
export type CapabilityOperation = z.infer<typeof CapabilityOperationSchema>;
export type CapabilityAvailability = z.infer<typeof CapabilityAvailabilitySchema>;
export type CapabilityLimitation = z.infer<typeof CapabilityLimitationSchema>;
export type CapabilityDescriptor = z.infer<typeof CapabilityDescriptorSchema>;
export type MutationKind = z.infer<typeof MutationKindSchema>;
export type MutationPlanChangeView = z.infer<typeof MutationPlanChangeViewSchema>;
export type MutationPlanView = z.infer<typeof MutationPlanViewSchema>;
export type OperationStatus = z.infer<typeof OperationStatusSchema>;
export type OperationReceipt = z.infer<typeof OperationReceiptSchema>;
export type AgentErrorCode = z.infer<typeof AgentErrorCodeSchema>;
export type AgentError = z.infer<typeof AgentErrorSchema>;
export type ConversionIssue = z.infer<typeof ConversionIssueSchema>;
export type ConversionPreview = z.infer<typeof ConversionPreviewSchema>;

export function parseAgentInstallation(
  input: unknown,
): { ok: true; value: AgentInstallation } | { ok: false; issues: string[] } {
  const result = AgentInstallationSchema.safeParse(input);
  if (result.success) return { ok: true, value: result.data };
  return {
    ok: false,
    issues: result.error.issues.map(
      (issue) => `${issue.path.join('.') || '<root>'}: ${issue.message}`,
    ),
  };
}

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
    projectPath: z.string().min(1).optional(),
    baseInstallationId: InstallationIdSchema.optional(),
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

export const SettingsDocumentSchema = z
  .object({
    resource: ResourceRefSchema,
    location: ResourceLocationSchema,
    mediaType: z.string().min(1),
    content: JsonValueSchema,
    exists: z.boolean(),
    digest: ContentDigestSchema.optional(),
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

export const ProcessObservationSchema = z
  .object({
    pid: z.number().int().nonnegative(),
    installationId: InstallationIdSchema,
    executable: z.string().min(1),
    cwd: z.string().min(1).optional(),
  })
  .strict();

export const ProjectCodexRuntimeStatusSchema = z
  .object({
    baseInstallationId: InstallationIdSchema,
    runtimeInstallationId: InstallationIdSchema,
    runtimeHome: z.string().min(1),
    prepared: z.boolean(),
    fresh: z.boolean(),
    desiredInheritBaseConfig: z.boolean(),
    appliedInheritBaseConfig: z.boolean(),
    needsRefresh: z.boolean(),
    pluginCount: z.number().int().nonnegative(),
    authMode: z.enum(['shared_file', 'keychain_blocked', 'missing']),
    profileId: z.string().min(1).optional(),
  })
  .strict();

export const MutationKindSchema = z.enum(['create', 'replace', 'delete']);

export const MutationPlanChangeViewSchema = z
  .object({
    resource: ResourceRefSchema,
    kind: MutationKindSchema,
  })
  .strict();

export const PlanAcknowledgementCodeSchema = z.enum([
  'conversion_apply',
  'dangerous_permission_expansion',
]);
export const PlanRiskLevelSchema = z.enum(['confirmation', 'dangerous']);
export const AcknowledgementRequirementSchema = z
  .object({
    code: PlanAcknowledgementCodeSchema,
    risk: PlanRiskLevelSchema,
  })
  .strict();
export const PlanAcknowledgementSchema = z
  .object({
    code: PlanAcknowledgementCodeSchema,
    accepted: z.boolean(),
  })
  .strict();

export const MutationPlanViewSchema = z
  .object({
    id: PlanIdSchema,
    agentId: AgentIdSchema,
    context: AgentContextSchema,
    changes: z.array(MutationPlanChangeViewSchema).default([]),
    requiredAcknowledgements: z.array(AcknowledgementRequirementSchema).default([]),
    expiresAt: z.string().datetime({ offset: true }),
  })
  .strict();

export const OperationStatusSchema = z.enum(['complete', 'compensated', 'partial_failure']);

export const ResourceStateKindSchema = z.enum(['missing', 'file', 'symlink', 'directory']);

export const AppliedResourceStateSchema = z
  .object({
    resource: ResourceRefSchema,
    kind: ResourceStateKindSchema,
    digest: ContentDigestSchema.optional(),
  })
  .strict();

export const OperationReceiptSchema = z
  .object({
    id: ReceiptIdSchema,
    planId: PlanIdSchema,
    status: OperationStatusSchema,
    appliedResources: z.array(ResourceRefSchema).default([]),
    backupPaths: z.array(z.string().min(1)).default([]),
    postApplyStates: z.array(AppliedResourceStateSchema).default([]),
    manifestDigest: ContentDigestSchema.optional(),
    message: z.string().optional(),
  })
  .strict();

export const OperationHistoryEntrySchema = z
  .object({
    receipt: OperationReceiptSchema,
    createdAt: z.string().datetime({ offset: true }),
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
  'confirmation_required',
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

export const ArtifactDispositionSchema = z.enum([
  'exact',
  'mapped',
  'partial',
  'requires_input',
  'unsupported',
  'conflict',
  'unchanged',
]);

export const ConversionRiskLevelSchema = z.enum(['safe', 'confirmation', 'dangerous']);
export const ConversionResolutionKindSchema = z.enum([
  'select_target_model',
  'select_permission_preset',
  'confirm_local_skill_source',
  'complete_plugin_setup',
  'resolve_conflict',
]);
export const ResolutionRequirementSchema = z
  .object({ kind: ConversionResolutionKindSchema })
  .strict();
export const ConversionEndpointSchema = z
  .object({
    resource: ResourceRefSchema,
    location: ResourceLocationSchema,
  })
  .strict();

export const PluginConversionDetailCodeSchema = z.enum([
  'source_disabled',
  'package_copy',
  'package_transform',
  'partial',
  'lsp_unsupported',
  'unresolved',
]);

export const ConversionArtifactSchema = z
  .object({
    id: z.string().min(1),
    kind: ResourceKindSchema,
    source: ConversionEndpointSchema,
    target: ConversionEndpointSchema.optional(),
    disposition: ArtifactDispositionSchema,
    resolution: ResolutionRequirementSchema.optional(),
    risk: ConversionRiskLevelSchema,
    itemCount: z.number().int().positive().optional(),
    detailCode: PluginConversionDetailCodeSchema.optional(),
    message: z.string().min(1),
  })
  .strict();

export const ConversionSummarySchema = z
  .object({
    total: z.number().int().nonnegative(),
    automatic: z.number().int().nonnegative(),
    requiresInput: z.number().int().nonnegative(),
    unsupported: z.number().int().nonnegative(),
    conflicts: z.number().int().nonnegative(),
    unchanged: z.number().int().nonnegative(),
    dangerous: z.number().int().nonnegative(),
  })
  .strict();

export const ConversionProgressPhaseSchema = z.enum([
  'reading_configuration',
  'inspecting_skills',
  'inspecting_plugins',
  'preparing_project_plugin',
  'verifying_inherited_plugins',
  'finalizing_plan',
]);

export const ConversionProgressEventSchema = z
  .object({
    phase: ConversionProgressPhaseSchema,
    current: z.number().int().nonnegative(),
    total: z.number().int().nonnegative().optional(),
    item: z.string().min(1).optional(),
  })
  .strict();

export const ConversionRoutePreviewSchema = z
  .object({
    sourceAgentId: AgentIdSchema,
    targetAgentId: AgentIdSchema,
    artifacts: z.array(ConversionArtifactSchema),
    summary: ConversionSummarySchema,
    plan: MutationPlanViewSchema.optional(),
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
export type SettingsDocument = z.infer<typeof SettingsDocumentSchema>;
export type CapabilityKind = z.infer<typeof CapabilityKindSchema>;
export type CapabilityOperation = z.infer<typeof CapabilityOperationSchema>;
export type CapabilityAvailability = z.infer<typeof CapabilityAvailabilitySchema>;
export type CapabilityLimitation = z.infer<typeof CapabilityLimitationSchema>;
export type CapabilityDescriptor = z.infer<typeof CapabilityDescriptorSchema>;
export type ProcessObservation = z.infer<typeof ProcessObservationSchema>;
export type ProjectCodexRuntimeStatus = z.infer<typeof ProjectCodexRuntimeStatusSchema>;
export type MutationKind = z.infer<typeof MutationKindSchema>;
export type MutationPlanChangeView = z.infer<typeof MutationPlanChangeViewSchema>;
export type MutationPlanView = z.infer<typeof MutationPlanViewSchema>;
export type PlanAcknowledgementCode = z.infer<typeof PlanAcknowledgementCodeSchema>;
export type PlanRiskLevel = z.infer<typeof PlanRiskLevelSchema>;
export type AcknowledgementRequirement = z.infer<typeof AcknowledgementRequirementSchema>;
export type PlanAcknowledgement = z.infer<typeof PlanAcknowledgementSchema>;
export type OperationStatus = z.infer<typeof OperationStatusSchema>;
export type OperationReceipt = z.infer<typeof OperationReceiptSchema>;
export type OperationHistoryEntry = z.infer<typeof OperationHistoryEntrySchema>;
export type AppliedResourceState = z.infer<typeof AppliedResourceStateSchema>;
export type AgentErrorCode = z.infer<typeof AgentErrorCodeSchema>;
export type AgentError = z.infer<typeof AgentErrorSchema>;
export type ConversionIssue = z.infer<typeof ConversionIssueSchema>;
export type ConversionPreview = z.infer<typeof ConversionPreviewSchema>;
export type ArtifactDisposition = z.infer<typeof ArtifactDispositionSchema>;
export type ConversionRiskLevel = z.infer<typeof ConversionRiskLevelSchema>;
export type ConversionResolutionKind = z.infer<typeof ConversionResolutionKindSchema>;
export type ResolutionRequirement = z.infer<typeof ResolutionRequirementSchema>;
export type ConversionEndpoint = z.infer<typeof ConversionEndpointSchema>;
export type ConversionArtifact = z.infer<typeof ConversionArtifactSchema>;
export type ConversionSummary = z.infer<typeof ConversionSummarySchema>;
export type ConversionProgressPhase = z.infer<typeof ConversionProgressPhaseSchema>;
export type ConversionProgressEvent = z.infer<typeof ConversionProgressEventSchema>;
export type ConversionRoutePreview = z.infer<typeof ConversionRoutePreviewSchema>;

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

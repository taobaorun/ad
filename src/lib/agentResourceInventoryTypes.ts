import { z } from 'zod';

import {
  AgentIdSchema,
  CapabilityLimitationSchema,
  DeclarationKeySchema,
  OwnershipRecordIdSchema,
  PhysicalTargetIdSchema,
  ResourceKeySchema,
  ResourceKindSchema,
  ResourceScopeSchema,
  WorkspaceKeySchema,
} from '@/lib/agentTypes';

export const CoverageStatusSchema = z.enum(['complete', 'partial', 'failed']);
export const EffectiveResourceStateSchema = z.enum([
  'enabled',
  'disabled',
  'unconfigured',
  'conflict',
  'external',
  'unsupported',
  'unknown',
]);
export const ResourceLayerSchema = z.enum(['system', 'user', 'project', 'runtime']);
export const ItemDiagnosticSchema = z
  .object({
    code: z.string().min(1),
    messageKey: z.string().min(1),
    retryable: z.boolean().default(false),
    resourceKey: ResourceKeySchema.optional(),
  })
  .strict();
export const CategoryCoverageSchema = z
  .object({
    status: CoverageStatusSchema,
    observed: z.number().int().nonnegative(),
    visible: z.number().int().nonnegative(),
    diagnostics: z.array(ItemDiagnosticSchema).default([]),
  })
  .strict();
export const ResourceDeclarationViewSchema = z
  .object({
    key: DeclarationKeySchema,
    layer: ResourceLayerSchema,
    sourceId: z.string().min(1),
    targetId: PhysicalTargetIdSchema,
    scope: ResourceScopeSchema.optional(),
  })
  .strict();
export const ResourceProvenanceViewSchema = z
  .object({
    declarations: z.array(ResourceDeclarationViewSchema).default([]),
    winner: DeclarationKeySchema.optional(),
  })
  .strict();
export const ResourceOwnershipKindSchema = z.enum([
  'ad_managed',
  'agent_managed',
  'external',
  'unknown',
]);
export const ResourceOwnershipViewSchema = z
  .object({
    kind: ResourceOwnershipKindSchema,
    recordId: OwnershipRecordIdSchema.optional(),
  })
  .strict();
export const ResourceHealthStatusSchema = z.enum(['healthy', 'degraded', 'error']);
export const ResourceHealthViewSchema = z
  .object({
    status: ResourceHealthStatusSchema,
    diagnostic: ItemDiagnosticSchema.optional(),
  })
  .strict();
export const ResourceActionSchema = z.enum([
  'inspect',
  'edit',
  'install',
  'update',
  'remove',
  'enable',
  'disable',
  'convert',
  'open_external',
]);
export const ResourceActionAvailabilitySchema = z.enum([
  'available',
  'confirmation_required',
  'unavailable',
  'external',
]);
export const ResourceActionViewSchema = z
  .object({
    action: ResourceActionSchema,
    availability: ResourceActionAvailabilitySchema,
    limitation: CapabilityLimitationSchema.optional(),
  })
  .strict();
export const ResourceManagementStatusSchema = z.enum([
  'managed',
  'read_only',
  'external',
  'unsupported',
]);
export const ResourceManagementViewSchema = z
  .object({
    status: ResourceManagementStatusSchema,
    actions: z.array(ResourceActionViewSchema).default([]),
  })
  .strict();
export const CollectionResourceViewSchema = z
  .object({
    key: ResourceKeySchema,
    kind: ResourceKindSchema,
    logicalId: z.string().min(1),
    displayName: z.string().min(1),
    description: z.string().min(1).optional(),
    effectiveState: EffectiveResourceStateSchema,
    provenance: ResourceProvenanceViewSchema,
    ownership: ResourceOwnershipViewSchema,
    health: ResourceHealthViewSchema,
    management: ResourceManagementViewSchema,
  })
  .strict();
export const CollectionResourceInventorySchema = z
  .object({
    workspaceKey: WorkspaceKeySchema,
    agentId: AgentIdSchema,
    kind: ResourceKindSchema,
    coverage: CategoryCoverageSchema,
    resources: z.array(CollectionResourceViewSchema).default([]),
  })
  .strict();

export type CoverageStatus = z.infer<typeof CoverageStatusSchema>;
export type ItemDiagnostic = z.infer<typeof ItemDiagnosticSchema>;
export type CategoryCoverage = z.infer<typeof CategoryCoverageSchema>;
export type EffectiveResourceState = z.infer<typeof EffectiveResourceStateSchema>;
export type ResourceLayer = z.infer<typeof ResourceLayerSchema>;
export type ResourceDeclarationView = z.infer<typeof ResourceDeclarationViewSchema>;
export type ResourceProvenanceView = z.infer<typeof ResourceProvenanceViewSchema>;
export type ResourceOwnershipKind = z.infer<typeof ResourceOwnershipKindSchema>;
export type ResourceOwnershipView = z.infer<typeof ResourceOwnershipViewSchema>;
export type ResourceHealthStatus = z.infer<typeof ResourceHealthStatusSchema>;
export type ResourceHealthView = z.infer<typeof ResourceHealthViewSchema>;
export type ResourceAction = z.infer<typeof ResourceActionSchema>;
export type ResourceActionAvailability = z.infer<typeof ResourceActionAvailabilitySchema>;
export type ResourceActionView = z.infer<typeof ResourceActionViewSchema>;
export type ResourceManagementStatus = z.infer<typeof ResourceManagementStatusSchema>;
export type ResourceManagementView = z.infer<typeof ResourceManagementViewSchema>;
export type CollectionResourceView = z.infer<typeof CollectionResourceViewSchema>;
export type CollectionResourceInventory = z.infer<typeof CollectionResourceInventorySchema>;

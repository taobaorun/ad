import { z } from 'zod';

import { OperationReceiptSchema, ResourceKeySchema, WorkspaceKeySchema } from '@/lib/agentTypes';

export const WorkspaceOperationOutcomeSchema = z.enum([
  'changed',
  'no_change',
  'external',
  'unsupported',
  'conflict',
  'partial_failure',
]);
export const WorkspaceOperationIssueSchema = z
  .object({
    code: z.string().min(1),
    messageKey: z.string().min(1),
    resourceKey: ResourceKeySchema.optional(),
  })
  .strict();
export const WorkspaceOperationReportSchema = z
  .object({
    workspaceKey: WorkspaceKeySchema,
    outcome: WorkspaceOperationOutcomeSchema,
    issues: z.array(WorkspaceOperationIssueSchema).default([]),
    receipt: OperationReceiptSchema.optional(),
  })
  .strict();
export const ConversionItemFinalStateSchema = z.enum([
  'exact',
  'mapped',
  'unchanged',
  'requires_input',
  'unsupported',
  'conflict',
  'failed',
]);
export const ConversionItemReportSchema = z
  .object({
    itemId: z.string().min(1),
    state: ConversionItemFinalStateSchema,
    residuals: z.array(WorkspaceOperationIssueSchema).default([]),
  })
  .strict();
export const ConversionReportSchema = z
  .object({
    workspaceKey: WorkspaceKeySchema,
    outcome: WorkspaceOperationOutcomeSchema,
    items: z.array(ConversionItemReportSchema).default([]),
    residuals: z.array(WorkspaceOperationIssueSchema).default([]),
    receipt: OperationReceiptSchema.optional(),
  })
  .strict();

export type WorkspaceOperationOutcome = z.infer<typeof WorkspaceOperationOutcomeSchema>;
export type WorkspaceOperationIssue = z.infer<typeof WorkspaceOperationIssueSchema>;
export type WorkspaceOperationReport = z.infer<typeof WorkspaceOperationReportSchema>;
export type ConversionItemFinalState = z.infer<typeof ConversionItemFinalStateSchema>;
export type ConversionItemReport = z.infer<typeof ConversionItemReportSchema>;
export type ConversionReport = z.infer<typeof ConversionReportSchema>;

import { z } from 'zod';

import {
  AgentIdSchema,
  InstallationIdSchema,
  WorkspaceKeySchema,
  WorkspaceRevisionSchema,
} from '@/lib/agentTypes';

export const ProjectRuntimeIdentitySchema = z
  .object({
    installationId: InstallationIdSchema,
    baseInstallationId: InstallationIdSchema,
    revision: WorkspaceRevisionSchema,
  })
  .strict();

export const WorkspaceDescriptorSchema = z
  .object({
    schemaVersion: z.literal(1),
    key: WorkspaceKeySchema,
    revision: WorkspaceRevisionSchema,
    agentId: AgentIdSchema,
    canonicalProjectPath: z.string().min(1),
    baseInstallationId: InstallationIdSchema,
    effectiveInstallationId: InstallationIdSchema,
    projectRuntime: ProjectRuntimeIdentitySchema.optional(),
  })
  .strict();

export type ProjectRuntimeIdentity = z.infer<typeof ProjectRuntimeIdentitySchema>;
export type WorkspaceDescriptor = z.infer<typeof WorkspaceDescriptorSchema>;

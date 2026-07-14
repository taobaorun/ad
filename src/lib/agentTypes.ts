import { z } from 'zod';

export const AgentIdSchema = z.string().min(1, 'agentId is required');

export const CapabilitySchema = z.enum([
  'settings',
  'skills',
  'plugins',
  'process_detection',
  'terminal_launch',
  'conversion',
]);

export const AgentMetadataSchema = z.object({
  id: AgentIdSchema,
  displayName: z.string().min(1, 'displayName is required'),
  capabilities: z.array(CapabilitySchema).default([]),
});

export const AgentInstallationSchema = z.object({
  agentId: AgentIdSchema,
  rootPath: z.string().min(1, 'rootPath is required'),
});

export const AgentProfileRefSchema = z.object({
  agentId: AgentIdSchema,
  profileId: z.string().min(1, 'profileId is required'),
});

export type AgentId = z.infer<typeof AgentIdSchema>;
export type Capability = z.infer<typeof CapabilitySchema>;
export type AgentMetadata = z.infer<typeof AgentMetadataSchema>;
export type AgentInstallation = z.infer<typeof AgentInstallationSchema>;
export type AgentProfileRef = z.infer<typeof AgentProfileRefSchema>;

export function parseAgentInstallation(
  input: unknown,
): { ok: true; value: AgentInstallation } | { ok: false; issues: string[] } {
  const result = AgentInstallationSchema.safeParse(input);
  if (result.success) return { ok: true, value: result.data };
  return {
    ok: false,
    issues: result.error.issues.map((issue) => `${issue.path.join('.') || '<root>'}: ${issue.message}`),
  };
}

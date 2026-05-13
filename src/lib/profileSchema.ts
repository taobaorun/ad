import { z } from 'zod';

export const ClaudeSettingsSchema = z
  .object({
    env: z.record(z.string(), z.string()).default({}),
    permissions: z.unknown().optional(),
    hooks: z.unknown().optional(),
    model: z.string().optional(),
    theme: z.string().optional(),
  })
  .catchall(z.unknown());

export const ProfileFileSchema = z.object({
  id: z
    .string()
    .min(1, 'id is required')
    .regex(/^[A-Za-z0-9._-]+$/, 'id must match [A-Za-z0-9._-]'),
  displayName: z.string().min(1, 'displayName is required'),
  description: z.string().nullish(),
  color: z
    .string()
    .regex(/^#[0-9A-Fa-f]{6}$/, 'color must be #RRGGBB')
    .default('#7C3AED'),
  createdAt: z.string().datetime({ offset: true }),
  updatedAt: z.string().datetime({ offset: true }),
  settings: ClaudeSettingsSchema,
});

export type ProfileFile = z.infer<typeof ProfileFileSchema>;
export type ClaudeSettings = z.infer<typeof ClaudeSettingsSchema>;

/**
 * Validates and parses a `ProfileFile` from arbitrary JSON. Returns either the
 * parsed value or a flat list of human-readable issues for surfacing in the UI.
 */
export function parseProfileFile(
  input: unknown,
): { ok: true; value: ProfileFile } | { ok: false; issues: string[] } {
  const result = ProfileFileSchema.safeParse(input);
  if (result.success) return { ok: true, value: result.data };
  return {
    ok: false,
    issues: result.error.issues.map((i) => `${i.path.join('.') || '<root>'}: ${i.message}`),
  };
}

export function blankProfile(id: string): ProfileFile {
  const now = new Date().toISOString();
  return {
    id,
    displayName: id,
    description: null,
    color: '#7C3AED',
    createdAt: now,
    updatedAt: now,
    settings: { env: {} },
  };
}

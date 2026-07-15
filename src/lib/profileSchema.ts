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

/**
 * v0.2 layered profile recipe. `shared` and `local` are JSON blocks that get
 * merged into the project's `.claude/settings.json` and `settings.local.json`.
 * `env` is presented as `export` snippets — never written to a file.
 */
export const ProfileLayersSchema = z.object({
  shared: z.unknown().optional(),
  local: z.unknown().optional(),
  env: z.record(z.string(), z.string()).default({}),
});

export const ProfileFileSchema = z.object({
  agentId: z.string().min(1, 'agentId is required').default('claude-code'),
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
  // v0.2 — defaulted so v0.1 profiles parsed pre-migration still validate.
  layers: ProfileLayersSchema.default({ env: {} }),
  // v0.1 legacy. Kept readable + writable for backward-compat with the legacy
  // global-overwrite activation path until M5 sunsets it. New flow uses `layers`.
  settings: ClaudeSettingsSchema,
});

export const CLAUDE_PROFILE_PAYLOAD_SCHEMA = 'ad.profile/claude-code.v2' as const;
export const CODEX_PROFILE_PAYLOAD_SCHEMA = 'ad.profile/codex.v1' as const;

const ProfileIdSchema = z
  .string()
  .min(1, 'profileId is required')
  .max(64, 'profileId must be at most 64 characters')
  .regex(
    /^[A-Za-z0-9][A-Za-z0-9._-]*$/,
    'profileId must start with alphanumeric and match [A-Za-z0-9._-]',
  );

export const ProfileMetadataSchema = z
  .object({
    displayName: z.string().min(1, 'displayName is required'),
    description: z.string().nullish(),
    color: z.string().regex(/^#[0-9A-Fa-f]{6}$/, 'color must be #RRGGBB'),
    createdAt: z.string().datetime({ offset: true }),
    updatedAt: z.string().datetime({ offset: true }),
  })
  .strict();

export const ClaudeProfilePayloadSchema = z
  .object({
    layers: ProfileLayersSchema,
    settings: ClaudeSettingsSchema,
  })
  .strict();

export const CodexProfilePayloadSchema = z
  .object({
    configToml: z.string(),
  })
  .strict();

const AgentProfileBaseSchema = z.object({
  schemaVersion: z.literal(1),
  metadata: ProfileMetadataSchema,
});

export const AgentProfileSchema = z.discriminatedUnion('payloadSchema', [
  AgentProfileBaseSchema.extend({
    key: z
      .object({
        agentId: z.literal('claude-code'),
        profileId: ProfileIdSchema,
      })
      .strict(),
    payloadSchema: z.literal(CLAUDE_PROFILE_PAYLOAD_SCHEMA),
    payload: ClaudeProfilePayloadSchema,
  }).strict(),
  AgentProfileBaseSchema.extend({
    key: z
      .object({
        agentId: z.literal('codex'),
        profileId: ProfileIdSchema,
      })
      .strict(),
    payloadSchema: z.literal(CODEX_PROFILE_PAYLOAD_SCHEMA),
    payload: CodexProfilePayloadSchema,
  }).strict(),
]);

export type ProfileFile = z.infer<typeof ProfileFileSchema>;
export type ClaudeSettings = z.infer<typeof ClaudeSettingsSchema>;
export type ProfileLayers = z.infer<typeof ProfileLayersSchema>;
export type AgentProfile = z.infer<typeof AgentProfileSchema>;
export type ProfileMetadata = z.infer<typeof ProfileMetadataSchema>;

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
    agentId: 'claude-code',
    id,
    displayName: id,
    description: null,
    color: '#7C3AED',
    createdAt: now,
    updatedAt: now,
    layers: { env: {} },
    settings: { env: {} },
  };
}

export function profileFileToAgentProfile(profile: ProfileFile): AgentProfile {
  if (profile.agentId !== 'claude-code') {
    throw new Error('Legacy ProfileFile conversion only supports claude-code');
  }
  return AgentProfileSchema.parse({
    schemaVersion: 1,
    key: {
      agentId: 'claude-code',
      profileId: profile.id,
    },
    metadata: {
      displayName: profile.displayName,
      description: profile.description,
      color: profile.color,
      createdAt: profile.createdAt,
      updatedAt: profile.updatedAt,
    },
    payloadSchema: CLAUDE_PROFILE_PAYLOAD_SCHEMA,
    payload: {
      layers: profile.layers,
      settings: profile.settings,
    },
  });
}

/**
 * Derives a flat `ClaudeSettings` block from a layered profile. Used to keep
 * the legacy `settings` field in sync with `layers` when saving — the
 * legacy global-overwrite activation path still reads `settings`.
 *
 * Order: `shared` is the base, `local` is shallow-merged on top, then `env`
 * is folded into the `.env` field (existing entries from `local.env` are
 * preserved; layer-`env` entries override).
 */
export function settingsFromLayers(layers: ProfileLayers): ClaudeSettings {
  const merged: Record<string, unknown> = {};
  if (isObject(layers.shared)) Object.assign(merged, layers.shared);
  if (isObject(layers.local)) Object.assign(merged, layers.local);

  if (Object.keys(layers.env).length > 0) {
    const existing = isObject(merged.env) ? (merged.env as Record<string, string>) : {};
    merged.env = { ...existing, ...layers.env };
  }
  if (!merged.env) merged.env = {};

  // Best-effort coerce. If the merged shape has unknown nested types that
  // ClaudeSettingsSchema rejects, propagate the raw object — backend
  // ClaudeSettings.extra (flatten) catches all unknown keys.
  return merged as ClaudeSettings;
}

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

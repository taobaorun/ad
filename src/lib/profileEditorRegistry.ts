import {
  AgentProfileSchema,
  CLAUDE_PROFILE_PAYLOAD_SCHEMA,
  ClaudeProfilePayloadSchema,
  CODEX_PROFILE_PAYLOAD_SCHEMA,
  CodexProfilePayloadSchema,
  type AgentProfile,
} from '@/lib/profileSchema';

export interface ProfilePayloadEditor {
  language: 'json' | 'text';
  serialize: (profile: AgentProfile) => string;
  update: (profile: AgentProfile, text: string) => AgentProfile;
}

export interface AgentProfileFeatures {
  legacyProjectTemplates: boolean;
  legacyImport: boolean;
}

const features: Record<string, AgentProfileFeatures> = {
  'claude-code': { legacyProjectTemplates: true, legacyImport: true },
  codex: { legacyProjectTemplates: false, legacyImport: false },
};

const editors: Record<string, ProfilePayloadEditor> = {
  [CLAUDE_PROFILE_PAYLOAD_SCHEMA]: {
    language: 'json',
    serialize: (profile) => JSON.stringify(profile.payload, null, 2),
    update: (profile, text) =>
      AgentProfileSchema.parse({
        ...profile,
        payload: ClaudeProfilePayloadSchema.parse(JSON.parse(text)),
      }),
  },
  [CODEX_PROFILE_PAYLOAD_SCHEMA]: {
    language: 'text',
    serialize: (profile) => CodexProfilePayloadSchema.parse(profile.payload).configToml,
    update: (profile, text) =>
      AgentProfileSchema.parse({
        ...profile,
        payload: { configToml: text },
      }),
  },
};

export function profileEditorFor(payloadSchema: string): ProfilePayloadEditor {
  const editor = editors[payloadSchema];
  if (!editor) throw new Error(`Unsupported profile payload schema: ${payloadSchema}`);
  return editor;
}

export function profileFeaturesFor(agentId: string): AgentProfileFeatures {
  const result = features[agentId];
  if (!result) throw new Error(`Unsupported built-in Agent profile: ${agentId}`);
  return result;
}

export function createBlankAgentProfile(agentId: string, profileId: string): AgentProfile {
  const now = new Date().toISOString();
  const common = {
    schemaVersion: 1,
    metadata: {
      displayName: profileId,
      description: null,
      color: '#7C3AED',
      createdAt: now,
      updatedAt: now,
    },
  };
  if (agentId === 'claude-code') {
    return AgentProfileSchema.parse({
      ...common,
      key: { agentId, profileId },
      payloadSchema: CLAUDE_PROFILE_PAYLOAD_SCHEMA,
      payload: {
        layers: { env: {} },
        settings: { env: {} },
      },
    });
  }
  if (agentId === 'codex') {
    return AgentProfileSchema.parse({
      ...common,
      key: { agentId, profileId },
      payloadSchema: CODEX_PROFILE_PAYLOAD_SCHEMA,
      payload: { configToml: '' },
    });
  }
  throw new Error(`Unsupported built-in Agent profile: ${agentId}`);
}

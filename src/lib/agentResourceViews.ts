import type { JsonValue, ResourceSnapshot, SettingsDocument } from '@/lib/agentTypes';
import type { CollectionResourceView } from '@/lib/agentResourceInventoryTypes';

export interface CollectionItemView {
  name: string;
  description?: string;
  enabled: boolean;
}

export function editableResourceText(snapshot: SettingsDocument): string {
  if (snapshot.mediaType === 'application/toml' && typeof snapshot.content === 'string') {
    return snapshot.content;
  }
  if (snapshot.mediaType === 'application/json') {
    return JSON.stringify(snapshot.content, null, 2);
  }
  return typeof snapshot.content === 'string'
    ? snapshot.content
    : JSON.stringify(snapshot.content, null, 2);
}

export function editedResourceContent(snapshot: SettingsDocument, text: string): JsonValue {
  if (snapshot.mediaType === 'application/json') {
    return JSON.parse(text) as JsonValue;
  }
  return text;
}

export function collectionItemView(
  snapshot: ResourceSnapshot | CollectionResourceView,
): CollectionItemView {
  if ('effectiveState' in snapshot) {
    return {
      name: snapshot.displayName,
      description: snapshot.description,
      enabled: snapshot.effectiveState === 'enabled',
    };
  }
  const content = isRecord(snapshot.content) ? snapshot.content : {};
  const name = typeof content.name === 'string' ? content.name : snapshot.resource.logicalId;
  const description = typeof content.description === 'string' ? content.description : undefined;
  const enabled =
    typeof content.enabled === 'boolean'
      ? content.enabled
      : content.scope === 'global' || content.scope === 'project';
  return { name, description, enabled };
}

function isRecord(value: JsonValue): value is Record<string, JsonValue> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

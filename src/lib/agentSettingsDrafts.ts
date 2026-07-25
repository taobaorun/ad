import { editableResourceText } from '@/lib/agentResourceViews';
import type { SettingsDocument } from '@/lib/agentTypes';

function resourceKey(snapshot: SettingsDocument): string {
  return JSON.stringify([
    snapshot.resource.installationId,
    snapshot.resource.projectPath ?? null,
    snapshot.resource.scope,
    snapshot.resource.logicalId,
  ]);
}

export function settingsDocumentKey(snapshot: SettingsDocument): string {
  return resourceKey(snapshot);
}

export function mergeSettingsDrafts(
  previousSnapshots: SettingsDocument[],
  currentDrafts: Record<string, string>,
  nextSnapshots: SettingsDocument[],
): Record<string, string> {
  const previousByKey = new Map(
    previousSnapshots.map((snapshot) => [resourceKey(snapshot), snapshot]),
  );
  return Object.fromEntries(
    nextSnapshots.map((snapshot) => {
      const key = resourceKey(snapshot);
      const previous = previousByKey.get(key);
      const draft = currentDrafts[key];
      const wasDirty =
        previous !== undefined && draft !== undefined && draft !== editableResourceText(previous);
      return [key, wasDirty ? draft : editableResourceText(snapshot)];
    }),
  );
}

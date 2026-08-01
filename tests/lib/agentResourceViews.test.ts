import { describe, expect, it } from 'vitest';

import { collectionItemView, editableResourceText } from '@/lib/agentResourceViews';
import { ResourceSnapshotSchema, SettingsDocumentSchema } from '@/lib/agentTypes';
import { CollectionResourceViewSchema } from '@/lib/agentResourceInventoryTypes';

function snapshot(content: unknown, mediaType: string, logicalId = 'review') {
  return SettingsDocumentSchema.parse({
    resource: {
      installationId: 'codex:default',
      kind: mediaType.includes('skill') ? 'skills' : 'settings',
      scope: 'user',
      logicalId,
    },
    location: { path: `/tmp/${logicalId}`, origin: 'user' },
    mediaType,
    content,
    exists: true,
    digest: 'sha256:test',
  });
}

function collectionSnapshot(content: unknown, logicalId = 'review') {
  return ResourceSnapshotSchema.parse({
    resource: {
      installationId: 'codex:default',
      kind: 'skills',
      scope: 'user',
      logicalId,
    },
    location: { path: `/tmp/${logicalId}`, origin: 'user' },
    mediaType: 'application/vnd.ad.skill+json',
    content,
    digest: 'sha256:test',
    observedAt: '2026-07-15T01:00:00Z',
  });
}

describe('Agent resource views', () => {
  it('renders JSON and TOML settings without checking the Agent id', () => {
    expect(editableResourceText(snapshot({ model: 'claude' }, 'application/json'))).toBe(
      '{\n  "model": "claude"\n}',
    );
    expect(editableResourceText(snapshot('model = "gpt-5.4"\n', 'application/toml'))).toBe(
      'model = "gpt-5.4"\n',
    );
  });

  it('normalizes collection items from the shared media contract', () => {
    expect(
      collectionItemView(
        collectionSnapshot({ name: 'Review', description: 'Reviews code', enabled: false }),
      ),
    ).toEqual({ name: 'Review', description: 'Reviews code', enabled: false });
  });

  it('uses backend-owned effective state for authoritative collection views', () => {
    const resource = CollectionResourceViewSchema.parse({
      key: 'resource:sha256:review',
      kind: 'skills',
      logicalId: 'review',
      displayName: 'Review',
      description: 'Reviews code',
      effectiveState: 'disabled',
      provenance: { declarations: [] },
      ownership: { kind: 'ad_managed' },
      health: { status: 'healthy' },
      management: { status: 'managed', actions: [] },
    });

    expect(collectionItemView(resource)).toEqual({
      name: 'Review',
      description: 'Reviews code',
      enabled: false,
    });
  });
});

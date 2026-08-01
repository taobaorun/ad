import { describe, expect, it } from 'vitest';

import { capabilityAllows, resourceActionAllows } from '@/lib/agentCapabilities';
import { CapabilityDescriptorSchema } from '@/lib/agentTypes';
import { CollectionResourceViewSchema } from '@/lib/agentResourceInventoryTypes';

const descriptors = CapabilityDescriptorSchema.array().parse([
  {
    kind: 'terminal_launch',
    scopes: ['project'],
    operations: ['launch'],
    availability: 'available',
    limitations: [],
  },
]);

describe('capabilityAllows', () => {
  it('requires a real operation and requested scope', () => {
    expect(capabilityAllows(descriptors, 'terminal_launch', 'launch', 'project')).toBe(true);
    expect(capabilityAllows(descriptors, 'terminal_launch', 'launch', 'user')).toBe(false);
    expect(capabilityAllows(descriptors, 'settings', 'inspect', 'project')).toBe(false);
  });

  it('lets a per-item limitation override a broad capability', () => {
    const resource = CollectionResourceViewSchema.parse({
      key: 'resource:sha256:review',
      kind: 'skills',
      logicalId: 'review',
      displayName: 'Review',
      effectiveState: 'enabled',
      provenance: { declarations: [] },
      ownership: { kind: 'external' },
      health: { status: 'healthy' },
      management: {
        status: 'external',
        actions: [
          {
            action: 'disable',
            availability: 'unavailable',
            limitation: { code: 'external', messageKey: 'agents.resources.external' },
          },
        ],
      },
    });

    expect(resourceActionAllows(resource, 'disable')).toBe(false);
  });
});

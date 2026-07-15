import { describe, expect, it } from 'vitest';

import { capabilityAllows } from '@/lib/agentCapabilities';
import { CapabilityDescriptorSchema } from '@/lib/agentTypes';

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
});

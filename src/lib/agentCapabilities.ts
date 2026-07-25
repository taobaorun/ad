import type {
  CapabilityDescriptor,
  CapabilityKind,
  CapabilityOperation,
  ResourceScope,
} from '@/lib/agentTypes';

export function capabilityAllows(
  capabilities: CapabilityDescriptor[],
  kind: CapabilityKind,
  operation: CapabilityOperation,
  scope?: ResourceScope,
): boolean {
  const descriptor = capabilities.find((capability) => capability.kind === kind);
  return (
    descriptor !== undefined &&
    descriptor.availability !== 'unavailable' &&
    descriptor.operations.includes(operation) &&
    (scope === undefined || descriptor.scopes.includes(scope))
  );
}

export function capabilityFor(
  capabilities: CapabilityDescriptor[],
  kind: CapabilityKind,
): CapabilityDescriptor | undefined {
  return capabilities.find((capability) => capability.kind === kind);
}

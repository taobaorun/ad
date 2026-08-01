import type {
  CapabilityDescriptor,
  CapabilityKind,
  CapabilityOperation,
  ResourceScope,
} from '@/lib/agentTypes';
import type {
  CollectionResourceView,
  ResourceAction,
  ResourceActionView,
} from '@/lib/agentResourceInventoryTypes';

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

export function actionForResource(
  resource: CollectionResourceView,
  action: ResourceAction,
): ResourceActionView | undefined {
  return resource.management.actions.find((candidate) => candidate.action === action);
}

export function resourceActionAllows(
  resource: CollectionResourceView,
  action: ResourceAction,
): boolean {
  const itemAction = actionForResource(resource, action);
  return (
    itemAction?.availability === 'available' || itemAction?.availability === 'confirmation_required'
  );
}

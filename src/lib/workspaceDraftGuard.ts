export const WORKSPACE_DRAFT_GUARD_EVENT = 'ad:before-agent-workspace-change';

export function requestAgentWorkspaceChange(): boolean {
  if (typeof window === 'undefined') return true;
  return window.dispatchEvent(new Event(WORKSPACE_DRAFT_GUARD_EVENT, { cancelable: true }));
}

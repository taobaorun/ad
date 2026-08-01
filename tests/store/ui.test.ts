import { beforeEach, describe, expect, it } from 'vitest';

import { WORKSPACE_DRAFT_GUARD_EVENT } from '@/lib/workspaceDraftGuard';
import { useUiState } from '@/store/ui';

describe('useUiState workspace draft guard', () => {
  beforeEach(() => {
    window.localStorage.clear();
    useUiState.setState({ activeProjectPath: '/Users/test/project-a' });
  });

  it('keeps the active project when a dirty Settings draft blocks the switch', () => {
    const block = (event: Event) => event.preventDefault();
    window.addEventListener(WORKSPACE_DRAFT_GUARD_EVENT, block);

    useUiState.getState().setActiveProject('/Users/test/project-b');

    expect(useUiState.getState().activeProjectPath).toBe('/Users/test/project-a');
    expect(window.localStorage.getItem('ad.ui-state.v1')).toBeNull();
    window.removeEventListener(WORKSPACE_DRAFT_GUARD_EVENT, block);
  });
});

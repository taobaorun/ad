import { act, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { ProjectCodexRuntimeCard } from '@/components/ProjectDetail';
import { useProjectCodexRuntimeInspection } from '@/hooks/useProjectCodexRuntimeInspection';
import i18n from '@/i18n';
import {
  AgentContextSchema,
  AgentInstallationSchema,
  ProjectCodexRuntimeStatusSchema,
} from '@/lib/agentTypes';
import {
  isProjectRuntimeLaunchUnavailable,
  resolveBaseProjectContext,
  resolveProjectAgentContext,
} from '@/lib/projectCodexRuntime';

const { inspectProjectCodexRuntime } = vi.hoisted(() => ({
  inspectProjectCodexRuntime: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    inspectProjectCodexRuntime,
  },
}));

const inheritedRuntime = ProjectCodexRuntimeStatusSchema.parse({
  baseInstallationId: 'codex:default',
  runtimeInstallationId: 'codex:/Users/test/.ad/codex-homes/project',
  runtimeHome: '/Users/test/.ad/codex-homes/project',
  prepared: true,
  fresh: false,
  desiredInheritBaseConfig: false,
  appliedInheritBaseConfig: true,
  needsRefresh: true,
  pluginCount: 2,
  authMode: 'shared_file',
});

describe('ProjectCodexRuntimeCard', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    inspectProjectCodexRuntime.mockReset();
  });

  it('distinguishes desired and applied policy and offers Preview', () => {
    const onInheritanceChange = vi.fn();
    const onPreview = vi.fn();
    render(
      <ProjectCodexRuntimeCard
        inheritBaseConfig={false}
        runtimeStatus={{ state: 'loaded', runtime: inheritedRuntime, error: null }}
        saving={false}
        onInheritanceChange={onInheritanceChange}
        onPreview={onPreview}
        onRetry={vi.fn()}
      />,
    );

    const toggle = screen.getByRole('switch', { name: 'Inherit Base config' });
    expect(toggle).not.toBeChecked();
    expect(screen.getByText('Desired: Isolated · Applied: Inherited')).toBeInTheDocument();
    expect(screen.getByText('The selected policy needs Preview → Apply')).toBeInTheDocument();
    fireEvent.click(toggle);
    expect(onInheritanceChange).toHaveBeenCalledWith(true);
    fireEvent.click(screen.getByRole('button', { name: 'Preview changes' }));
    expect(onPreview).toHaveBeenCalledOnce();
  });

  it('exposes checking and failure recovery without misreporting readiness', () => {
    const { rerender } = render(
      <ProjectCodexRuntimeCard
        inheritBaseConfig
        runtimeStatus={{ state: 'checking', runtime: null, error: null }}
        saving={false}
        onInheritanceChange={vi.fn()}
        onPreview={vi.fn()}
        onRetry={vi.fn()}
      />,
    );
    expect(screen.getByRole('switch')).toBeDisabled();
    expect(screen.getByRole('status')).toHaveTextContent('Checking Project Codex runtime…');

    const onRetry = vi.fn();
    rerender(
      <ProjectCodexRuntimeCard
        inheritBaseConfig
        runtimeStatus={{ state: 'error', runtime: null, error: 'manifest is invalid' }}
        saving={false}
        onInheritanceChange={vi.fn()}
        onPreview={vi.fn()}
        onRetry={onRetry}
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('manifest is invalid');
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it('labels a derived descriptor without generated config as not prepared', () => {
    render(
      <ProjectCodexRuntimeCard
        inheritBaseConfig
        runtimeStatus={{
          state: 'loaded',
          runtime: {
            ...inheritedRuntime,
            prepared: false,
            fresh: false,
            desiredInheritBaseConfig: true,
            appliedInheritBaseConfig: true,
            needsRefresh: true,
            authMode: 'missing',
          },
          error: null,
        }}
        saving={false}
        onInheritanceChange={vi.fn()}
        onPreview={vi.fn()}
        onRetry={vi.fn()}
      />,
    );

    expect(screen.getByRole('status')).toHaveTextContent('Project Codex has not been prepared');
    expect(screen.getByRole('button', { name: 'Preview setup' })).toBeEnabled();
  });
});

describe('Project Codex launch context', () => {
  it('uses the exact base installation when a selected runtime belongs to another project', () => {
    const installations = AgentInstallationSchema.array().parse([
      {
        id: 'codex:base',
        agentId: 'codex',
        rootPath: '/Users/test/.codex',
      },
      {
        id: 'codex:runtime-a',
        agentId: 'codex',
        rootPath: '/Users/test/.ad/codex-homes/a',
        projectPath: '/Users/test/project-a',
        baseInstallationId: 'codex:base',
      },
    ]);
    const activeContext = AgentContextSchema.parse({
      installationId: 'codex:runtime-a',
      projectPath: '/Users/test/project-a',
    });

    expect(
      resolveBaseProjectContext(activeContext, installations, '/Users/test/project-b'),
    ).toEqual({
      installationId: 'codex:base',
      projectPath: '/Users/test/project-b',
    });
  });

  it('falls back to the base installation when inherited config needs no generated runtime', () => {
    const baseContext = AgentContextSchema.parse({
      installationId: 'codex:/Users/test/.codex',
      projectPath: '/Users/test/project',
    });
    const runtimeInspection = { state: 'loaded', runtime: null, error: null } as const;

    expect(resolveProjectAgentContext(baseContext, 'codex', null)).toEqual(baseContext);
    expect(isProjectRuntimeLaunchUnavailable('codex', true, runtimeInspection)).toBe(false);
    expect(isProjectRuntimeLaunchUnavailable('codex', false, runtimeInspection)).toBe(true);
  });

  it('uses a prepared runtime installation for project operations', () => {
    const baseContext = AgentContextSchema.parse({
      installationId: 'codex:/Users/test/.codex',
      projectPath: '/Users/test/project',
    });
    const readyRuntime = {
      ...inheritedRuntime,
      fresh: true,
      desiredInheritBaseConfig: true,
      appliedInheritBaseConfig: true,
      needsRefresh: false,
    };

    expect(resolveProjectAgentContext(baseContext, 'codex', readyRuntime)).toEqual({
      installationId: readyRuntime.runtimeInstallationId,
      projectPath: '/Users/test/project',
    });
    expect(
      isProjectRuntimeLaunchUnavailable('codex', true, {
        state: 'loaded',
        runtime: readyRuntime,
        error: null,
      }),
    ).toBe(false);
  });
});

describe('useProjectCodexRuntimeInspection', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    inspectProjectCodexRuntime.mockReset();
  });

  it('keeps the newest runtime inspection result', async () => {
    let resolveInitial!: (runtime: typeof inheritedRuntime) => void;
    let resolveRefresh!: (runtime: typeof inheritedRuntime) => void;
    inspectProjectCodexRuntime
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveInitial = resolve;
        }),
      )
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveRefresh = resolve;
        }),
      );
    const context = AgentContextSchema.parse({
      installationId: 'codex:default',
      projectPath: '/Users/test/project',
    });
    const { result } = renderHook(() => useProjectCodexRuntimeInspection(context, 'codex'));

    act(() => window.dispatchEvent(new Event('ad:project-codex-runtime-changed')));
    const refreshed = { ...inheritedRuntime, appliedInheritBaseConfig: false };
    await act(async () => resolveRefresh(refreshed));
    await waitFor(() => expect(result.current[0].runtime).toEqual(refreshed));

    await act(async () => resolveInitial(inheritedRuntime));
    expect(result.current[0].runtime).toEqual(refreshed);
  });

  it('localizes typed runtime inspection errors', async () => {
    inspectProjectCodexRuntime.mockRejectedValueOnce({
      code: 'resource_changed',
      message: 'manifest digest mismatch',
      installationId: 'codex:default',
      retryable: false,
    });
    const context = AgentContextSchema.parse({
      installationId: 'codex:default',
      projectPath: '/Users/test/project',
    });
    const { result } = renderHook(() => useProjectCodexRuntimeInspection(context, 'codex'));

    await waitFor(() => expect(result.current[0].state).toBe('error'));
    expect(result.current[0].error).toBe(
      'The resource changed after preview. Preview the operation again.',
    );
  });
});

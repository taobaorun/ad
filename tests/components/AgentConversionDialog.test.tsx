import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentConversionButton } from '@/components/AgentConversionDialog';
import i18n from '@/i18n';
import { AgentInstallationSchema } from '@/lib/agentTypes';
import { useAgents } from '@/store/agents';
import { useProjects } from '@/store/projects';
import { useUiState } from '@/store/ui';

const {
  resolveAgentContext,
  previewClaudeToCodexRoute,
  applyConversionPlan,
  previewAgentRollback,
  applyAgentRollbackPlan,
} = vi.hoisted(() => ({
  resolveAgentContext: vi.fn(),
  previewClaudeToCodexRoute: vi.fn(),
  applyConversionPlan: vi.fn(),
  previewAgentRollback: vi.fn(),
  applyAgentRollbackPlan: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    resolveAgentContext,
    previewClaudeToCodexRoute,
    applyConversionPlan,
    previewAgentRollback,
    applyAgentRollbackPlan,
  },
}));

const installations = AgentInstallationSchema.array().parse([
  {
    id: 'claude-code:default',
    agentId: 'claude-code',
    rootPath: '/Users/test/.claude',
  },
  {
    id: 'codex:default',
    agentId: 'codex',
    rootPath: '/Users/test/.codex',
  },
]);

describe('AgentConversionButton', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    useAgents.setState({ installations });
    useUiState.setState({ activeProjectPath: '/Users/test/project' });
    useProjects.setState({
      projects: [
        {
          path: '/Users/test/project',
          displayName: 'project',
          addedAt: '2026-07-25T00:00:00Z',
          currentProfileId: null,
          lastApplied: null,
          pinned: false,
          inheritBaseConfig: true,
        },
      ],
    });
    resolveAgentContext.mockReset().mockImplementation((installationId, projectPath) =>
      Promise.resolve({
        installationId,
        ...(projectPath ? { projectPath } : {}),
      }),
    );
    previewClaudeToCodexRoute.mockReset().mockResolvedValue({
      sourceAgentId: 'claude-code',
      targetAgentId: 'codex',
      artifacts: [
        {
          id: 'settings:model',
          kind: 'settings',
          source: {
            resource: {
              installationId: 'claude-code:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-settings',
            },
            location: { path: '/Users/test/.claude/settings.json', origin: 'user' },
          },
          target: {
            resource: {
              installationId: 'codex:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-config',
            },
            location: { path: '/Users/test/.codex/config.toml', origin: 'user' },
          },
          disposition: 'mapped',
          risk: 'confirmation',
          message: 'Model maps to Codex',
        },
      ],
      summary: {
        total: 1,
        automatic: 1,
        requiresInput: 0,
        unsupported: 0,
        conflicts: 0,
        unchanged: 0,
        dangerous: 0,
      },
      plan: {
        id: 'conversion-plan',
        agentId: 'codex',
        context: { installationId: 'codex:default' },
        changes: [
          {
            resource: {
              installationId: 'codex:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-config',
            },
            kind: 'replace',
          },
        ],
        requiredAcknowledgements: [{ code: 'conversion_apply', risk: 'confirmation' }],
        riskFingerprint: 'risk:conversion-plan',
        expiresAt: '2026-07-15T01:05:00Z',
      },
    });
    applyConversionPlan.mockReset().mockResolvedValue({
      id: 'conversion-receipt',
      planId: 'conversion-plan',
      status: 'complete',
      appliedResources: [],
      backupPaths: ['/Users/test/.ad/backups/config.toml'],
      postApplyStates: [],
    });
    previewAgentRollback.mockReset().mockResolvedValue({
      id: 'conversion-rollback-plan',
      riskFingerprint: 'risk:conversion-rollback',
    });
    applyAgentRollbackPlan.mockReset().mockResolvedValue({
      id: 'rollback-receipt',
      planId: 'rollback-plan',
      status: 'complete',
      appliedResources: [],
      backupPaths: [],
      postApplyStates: [],
    });
    vi.spyOn(window, 'confirm').mockReturnValue(true);
  });

  it('previews artifacts before explicitly applying a conversion', async () => {
    const runtimeChanged = vi.fn();
    const workspaceChanged = vi.fn();
    window.addEventListener('ad:project-codex-runtime-changed', runtimeChanged);
    window.addEventListener('ad:agent-workspace-changed', workspaceChanged);
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    expect(screen.getByText(/The Claude Code source is read-only/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await screen.findByText('Mapped');
    expect(previewClaudeToCodexRoute).toHaveBeenCalledWith(
      { installationId: 'claude-code:default' },
      { installationId: 'codex:default' },
      {},
      expect.any(Function),
    );
    expect(
      screen.getByText('This item will be converted to the listed Codex target.'),
    ).toBeInTheDocument();
    expect(screen.getByText('/Users/test/.claude/settings.json')).toBeInTheDocument();
    expect(screen.getByText('/Users/test/.codex/config.toml')).toBeInTheDocument();
    expect(applyConversionPlan).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Apply conversion' }));
    await waitFor(() =>
      expect(applyConversionPlan).toHaveBeenCalledWith(
        'conversion-plan',
        { installationId: 'codex:default' },
        'risk:conversion-plan',
        [{ code: 'conversion_apply', accepted: true }],
      ),
    );
    expect(await screen.findByText('1 backup created')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Rollback target' }));
    await waitFor(() =>
      expect(previewAgentRollback).toHaveBeenCalledWith('conversion-receipt', {
        installationId: 'codex:default',
      }),
    );
    expect(applyAgentRollbackPlan).toHaveBeenCalledWith(
      'conversion-rollback-plan',
      { installationId: 'codex:default' },
      'risk:conversion-rollback',
      true,
    );
    expect(runtimeChanged).toHaveBeenCalledTimes(2);
    expect(workspaceChanged).toHaveBeenCalledTimes(2);
    window.removeEventListener('ad:project-codex-runtime-changed', runtimeChanged);
    window.removeEventListener('ad:agent-workspace-changed', workspaceChanged);
  });

  it('reports a compensated receipt as a failure without offering rollback', async () => {
    applyConversionPlan.mockResolvedValueOnce({
      id: 'compensated-receipt',
      planId: 'conversion-plan',
      status: 'compensated',
      appliedResources: [],
      backupPaths: ['/Users/test/.ad/backups/config.toml'],
      postApplyStates: [],
      message: 'The target write failed and changes were restored.',
    });
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await screen.findByText('Mapped');
    fireEvent.click(screen.getByRole('button', { name: 'Apply conversion' }));

    expect(
      await screen.findByText('Conversion failed; all applied changes were restored'),
    ).toBeInTheDocument();
    expect(screen.queryByText('Conversion applied')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Rollback target' })).not.toBeInTheDocument();
  });

  it('reports a partial receipt as recoverable failure', async () => {
    applyConversionPlan.mockResolvedValueOnce({
      id: 'partial-receipt',
      planId: 'conversion-plan',
      status: 'partial_failure',
      appliedResources: [],
      backupPaths: ['/Users/test/.ad/backups/config.toml'],
      postApplyStates: [],
      message: 'One target could not be restored.',
    });
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await screen.findByText('Mapped');
    fireEvent.click(screen.getByRole('button', { name: 'Apply conversion' }));

    expect(
      await screen.findByText('Conversion partially applied; rollback is available'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Rollback target' })).toBeEnabled();
  });

  it('previews only the current project scope with canonical Agent contexts', async () => {
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.change(screen.getByRole('combobox', { name: 'Conversion scope' }), {
      target: { value: 'project' },
    });
    expect(screen.getByText('/Users/test/project')).toBeInTheDocument();
    expect(screen.getByText(/isolated CODEX_HOME/)).toBeInTheDocument();
    fireEvent.change(screen.getByRole('textbox', { name: 'Codex Profile (optional)' }), {
      target: { value: 'project-api' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));

    await waitFor(() => {
      expect(resolveAgentContext).toHaveBeenCalledWith(
        'claude-code:default',
        '/Users/test/project',
      );
      expect(resolveAgentContext).toHaveBeenCalledWith('codex:default', '/Users/test/project');
      expect(previewClaudeToCodexRoute).toHaveBeenCalledWith(
        {
          installationId: 'claude-code:default',
          projectPath: '/Users/test/project',
        },
        {
          installationId: 'codex:default',
          projectPath: '/Users/test/project',
        },
        { profileId: 'project-api', inheritBaseConfig: true },
        expect.any(Function),
      );
    });
  });

  it('hides configuration instance controls when each Agent has one installation', () => {
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));

    expect(
      screen.queryByRole('combobox', { name: 'Claude Code configuration instance' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('combobox', { name: 'Codex configuration instance' }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText('/Users/test/.claude')).not.toBeInTheDocument();
    expect(screen.queryByText('/Users/test/.codex')).not.toBeInTheDocument();
  });

  it('excludes a derived runtime from User and unrelated Project targets', async () => {
    useAgents.setState({
      installations: AgentInstallationSchema.array().parse([
        installations[0],
        {
          id: 'codex:other-runtime',
          agentId: 'codex',
          rootPath: '/Users/test/.ad/codex-homes/other',
          projectPath: '/Users/test/other',
          baseInstallationId: 'codex:default',
        },
        installations[1],
      ]),
    });
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await waitFor(() =>
      expect(previewClaudeToCodexRoute).toHaveBeenCalledWith(
        { installationId: 'claude-code:default' },
        { installationId: 'codex:default' },
        {},
        expect.any(Function),
      ),
    );
    expect(resolveAgentContext).not.toHaveBeenCalledWith('codex:other-runtime', expect.anything());

    previewClaudeToCodexRoute.mockClear();
    resolveAgentContext.mockClear();
    fireEvent.change(screen.getByRole('combobox', { name: 'Conversion scope' }), {
      target: { value: 'project' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await waitFor(() =>
      expect(resolveAgentContext).toHaveBeenCalledWith('codex:default', '/Users/test/project'),
    );
    expect(resolveAgentContext).not.toHaveBeenCalledWith(
      'codex:other-runtime',
      '/Users/test/project',
    );
  });

  it('passes explicit Codex model and permission decisions into preview', async () => {
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Codex model' }), {
      target: { value: 'gpt-5.6-sol' },
    });
    fireEvent.change(screen.getByRole('combobox', { name: 'Codex permissions' }), {
      target: { value: 'never_danger_full_access' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));

    await waitFor(() =>
      expect(previewClaudeToCodexRoute).toHaveBeenCalledWith(
        { installationId: 'claude-code:default' },
        { installationId: 'codex:default' },
        {
          targetModel: 'gpt-5.6-sol',
          permissionPreset: 'never_danger_full_access',
        },
        expect.any(Function),
      ),
    );
  });

  it('explains when the source was read but no safe changes are available', async () => {
    previewClaudeToCodexRoute.mockResolvedValueOnce({
      sourceAgentId: 'claude-code',
      targetAgentId: 'codex',
      artifacts: [
        {
          id: 'user-settings:model',
          kind: 'settings',
          source: {
            resource: {
              installationId: 'claude-code:default',
              kind: 'settings',
              scope: 'user',
              logicalId: 'user-settings',
            },
            location: { path: '/Users/test/.claude/settings.json', origin: 'user' },
          },
          disposition: 'requires_input',
          resolution: { kind: 'select_target_model' },
          risk: 'confirmation',
          message: 'Select a Codex model',
        },
      ],
      summary: {
        total: 1,
        automatic: 0,
        requiresInput: 1,
        unsupported: 0,
        conflicts: 0,
        unchanged: 0,
        dangerous: 0,
      },
    });
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));

    expect(await screen.findByText(/The source was read/)).toHaveTextContent(
      '1 items require input',
    );
    expect(screen.queryByRole('button', { name: 'Apply conversion' })).not.toBeInTheDocument();
  });

  it('turns a confirmed local Skill into a new backend preview', async () => {
    const safe = await previewClaudeToCodexRoute();
    previewClaudeToCodexRoute.mockReset().mockResolvedValue({
      ...safe,
      artifacts: [
        {
          id: 'skill:review',
          kind: 'skills',
          source: {
            resource: {
              installationId: 'claude-code:default',
              kind: 'skills',
              scope: 'user',
              logicalId: 'local/review',
            },
            location: { path: '/Users/test/.claude/skills/review', origin: 'user' },
          },
          target: {
            resource: {
              installationId: 'codex:default',
              kind: 'skills',
              scope: 'user',
              logicalId: 'review',
            },
            location: { path: '/Users/test/.agents/skills/review', origin: 'user' },
          },
          disposition: 'requires_input',
          resolution: { kind: 'confirm_local_skill_source' },
          risk: 'confirmation',
          message: 'Confirm the resolved local Skill source',
        },
      ],
      summary: {
        total: 1,
        automatic: 0,
        requiresInput: 1,
        unsupported: 0,
        conflicts: 0,
        unchanged: 0,
        dangerous: 0,
      },
    });
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Include this Skill' }));

    await waitFor(() =>
      expect(previewClaudeToCodexRoute).toHaveBeenLastCalledWith(
        { installationId: 'claude-code:default' },
        { installationId: 'codex:default' },
        { confirmedSkillIds: ['review'] },
        expect.any(Function),
      ),
    );
  });

  it('locks scope and decisions while a preview is in flight', () => {
    previewClaudeToCodexRoute.mockReturnValue(new Promise(() => {}));
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));

    expect(screen.getByRole('combobox', { name: 'Conversion scope' })).toBeDisabled();
    expect(screen.getByRole('textbox', { name: 'Codex model' })).toBeDisabled();
    expect(screen.getByRole('combobox', { name: 'Codex permissions' })).toBeDisabled();
  });

  it('discards a pending Project preview after the active Project changes', async () => {
    const stalePreview = await previewClaudeToCodexRoute();
    let resolvePreview!: (preview: typeof stalePreview) => void;
    previewClaudeToCodexRoute.mockReset().mockReturnValue(
      new Promise((resolve) => {
        resolvePreview = resolve;
      }),
    );
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.change(screen.getByRole('combobox', { name: 'Conversion scope' }), {
      target: { value: 'project' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await waitFor(() => expect(previewClaudeToCodexRoute).toHaveBeenCalledOnce());

    act(() => useUiState.setState({ activeProjectPath: '/Users/test/other-project' }));
    await act(async () => resolvePreview(stalePreview));

    expect(screen.queryByRole('button', { name: 'Apply conversion' })).not.toBeInTheDocument();
    expect(screen.queryByText('Mapped')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Preview conversion' })).toBeEnabled();
  });

  it('shows real backend progress details while a preview is in flight', async () => {
    previewClaudeToCodexRoute.mockImplementation(
      (_source, _target, _options, onProgress: (event: unknown) => void) => {
        onProgress({
          phase: 'verifying_inherited_plugins',
          current: 2,
          total: 9,
          item: 'browser@openai-bundled',
        });
        return new Promise(() => {});
      },
    );
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));

    expect(await screen.findByText('Verifying inherited Codex Plugins')).toBeInTheDocument();
    expect(screen.getByText('browser@openai-bundled · 2 / 9')).toBeInTheDocument();
    const progressStatus = screen.getByRole('status');
    expect(progressStatus).toHaveAttribute('aria-busy', 'true');
    expect(screen.getByTestId('conversion-sticky-footer')).toContainElement(progressStatus);
  });

  it('keeps the conversion progress indicator visible while apply is in flight', async () => {
    applyConversionPlan.mockReturnValue(new Promise(() => {}));
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await screen.findByText('Mapped');
    fireEvent.click(screen.getByRole('button', { name: 'Apply conversion' }));

    const progressStatus = await screen.findByRole('status');
    expect(progressStatus).toHaveAttribute('aria-busy', 'true');
    expect(progressStatus).toHaveTextContent('Applying…');
    expect(screen.getByTestId('conversion-sticky-footer')).toContainElement(progressStatus);
    for (const closeButton of screen.getAllByRole('button', { name: 'Close' })) {
      expect(closeButton).toBeDisabled();
    }
    expect(screen.getByRole('button', { name: 'Preview conversion' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Applying…' })).toBeDisabled();
  });

  it('discards a completed Project apply result after the active Project changes', async () => {
    const receipt = await applyConversionPlan();
    let resolveApply!: (value: typeof receipt) => void;
    applyConversionPlan.mockReset().mockReturnValue(
      new Promise((resolve) => {
        resolveApply = resolve;
      }),
    );
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.change(screen.getByRole('combobox', { name: 'Conversion scope' }), {
      target: { value: 'project' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await screen.findByText('Mapped');
    fireEvent.click(screen.getByRole('button', { name: 'Apply conversion' }));
    await waitFor(() => expect(applyConversionPlan).toHaveBeenCalledOnce());

    act(() => useUiState.setState({ activeProjectPath: '/Users/test/other-project' }));
    await act(async () => resolveApply(receipt));

    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Preview conversion' })).toBeEnabled(),
    );
    expect(screen.queryByText('1 backup created')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Rollback target' })).not.toBeInTheDocument();
  });

  it('opens pending Project setup with isolated policy and disables Base profiles', async () => {
    useProjects.setState((state) => ({
      projects: state.projects.map((project) => ({
        ...project,
        inheritBaseConfig: false,
      })),
    }));
    render(<AgentConversionButton />);

    act(() => window.dispatchEvent(new Event('ad:open-project-conversion')));
    expect(await screen.findByRole('combobox', { name: 'Conversion scope' })).toHaveValue(
      'project',
    );
    expect(screen.getByRole('textbox', { name: 'Codex Profile (optional)' })).toBeDisabled();
    expect(
      screen.getByText(
        'Profiles from the Base config are unavailable while Base config inheritance is off.',
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await waitFor(() =>
      expect(previewClaudeToCodexRoute).toHaveBeenCalledWith(
        {
          installationId: 'claude-code:default',
          projectPath: '/Users/test/project',
        },
        {
          installationId: 'codex:default',
          projectPath: '/Users/test/project',
        },
        { inheritBaseConfig: false },
        expect.any(Function),
      ),
    );
  });

  it('requires a separate confirmation for unrestricted permission plans', async () => {
    const safe = await previewClaudeToCodexRoute();
    previewClaudeToCodexRoute.mockReset().mockResolvedValue({
      ...safe,
      artifacts: safe.artifacts.map((artifact: { risk: string }) => ({
        ...artifact,
        risk: 'dangerous',
      })),
      summary: { ...safe.summary, dangerous: 1 },
      plan: {
        ...safe.plan,
        requiredAcknowledgements: [
          { code: 'conversion_apply', risk: 'confirmation' },
          { code: 'dangerous_permission_expansion', risk: 'dangerous' },
        ],
      },
    });
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await screen.findByText('Mapped');
    fireEvent.click(screen.getByRole('button', { name: 'Apply conversion' }));

    expect(
      screen.getByRole('alertdialog', { name: 'Confirm unrestricted Codex permissions' }),
    ).toBeInTheDocument();
    expect(applyConversionPlan).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Apply unrestricted permissions' }));
    await waitFor(() =>
      expect(applyConversionPlan).toHaveBeenCalledWith(
        'conversion-plan',
        { installationId: 'codex:default' },
        'risk:conversion-plan',
        [
          { code: 'conversion_apply', accepted: true },
          { code: 'dangerous_permission_expansion', accepted: true },
        ],
      ),
    );
  });
});

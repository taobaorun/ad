import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentConversionButton } from '@/components/AgentConversionDialog';
import i18n from '@/i18n';
import { AgentInstallationSchema } from '@/lib/agentTypes';
import { useAgents } from '@/store/agents';
import { useUiState } from '@/store/ui';

const {
  resolveAgentContext,
  previewClaudeToCodexRoute,
  applyConversionPlan,
  rollbackAgentReceipt,
} = vi.hoisted(() => ({
  resolveAgentContext: vi.fn(),
  previewClaudeToCodexRoute: vi.fn(),
  applyConversionPlan: vi.fn(),
  rollbackAgentReceipt: vi.fn(),
}));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    resolveAgentContext,
    previewClaudeToCodexRoute,
    applyConversionPlan,
    rollbackAgentReceipt,
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
    rollbackAgentReceipt.mockReset().mockResolvedValue({
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
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    expect(screen.getByText(/The Claude Code source is read-only/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Preview conversion' }));
    await screen.findByText('Mapped');
    expect(previewClaudeToCodexRoute).toHaveBeenCalledWith(
      { installationId: 'claude-code:default' },
      { installationId: 'codex:default' },
      {},
    );
    expect(
      screen.getByText('This item will be converted to the listed Codex target.'),
    ).toBeInTheDocument();
    expect(screen.getByText('/Users/test/.claude/settings.json')).toBeInTheDocument();
    expect(screen.getByText('/Users/test/.codex/config.toml')).toBeInTheDocument();
    expect(applyConversionPlan).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Apply conversion' }));
    await waitFor(() =>
      expect(applyConversionPlan).toHaveBeenCalledWith('conversion-plan', [
        { code: 'conversion_apply', accepted: true },
      ]),
    );
    expect(await screen.findByText('1 backup created')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Rollback target' }));
    await waitFor(() =>
      expect(rollbackAgentReceipt).toHaveBeenCalledWith('conversion-receipt', true),
    );
  });

  it('previews only the current project scope with canonical Agent contexts', async () => {
    render(<AgentConversionButton />);

    fireEvent.click(screen.getByRole('button', { name: 'Convert configuration' }));
    fireEvent.change(screen.getByRole('combobox', { name: 'Conversion scope' }), {
      target: { value: 'project' },
    });
    expect(screen.getByText('/Users/test/project')).toBeInTheDocument();
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
        {},
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
      expect(applyConversionPlan).toHaveBeenCalledWith('conversion-plan', [
        { code: 'conversion_apply', accepted: true },
        { code: 'dangerous_permission_expansion', accepted: true },
      ]),
    );
  });
});

import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import { AgentSelector } from '@/components/AgentSelector';
import i18n from '@/i18n';
import { AgentContextSchema, AgentInstallationSchema, AgentMetadataSchema } from '@/lib/agentTypes';
import { useAgents } from '@/store/agents';

describe('AgentSelector', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    useAgents.setState({
      agents: [
        AgentMetadataSchema.parse({
          id: 'claude-code',
          displayName: 'Claude Code',
          capabilities: [],
        }),
        AgentMetadataSchema.parse({ id: 'codex', displayName: 'Codex', capabilities: [] }),
      ],
      installations: [
        AgentInstallationSchema.parse({
          id: 'claude-code:/Users/test/.claude',
          agentId: 'claude-code',
          rootPath: '/Users/test/.claude',
        }),
        AgentInstallationSchema.parse({
          id: 'codex:/Users/test/.codex',
          agentId: 'codex',
          rootPath: '/Users/test/.codex',
        }),
        AgentInstallationSchema.parse({
          id: 'codex:/Users/test/environment-codex',
          agentId: 'codex',
          rootPath: '/Users/test/environment-codex',
        }),
        AgentInstallationSchema.parse({
          id: 'codex:/Users/test/project-home',
          agentId: 'codex',
          rootPath: '/Users/test/project-home',
          projectPath: '/Users/test/project',
          baseInstallationId: 'codex:/Users/test/.codex',
        }),
      ],
      activeAgentId: 'codex',
      activeContext: AgentContextSchema.parse({
        installationId: 'codex:/Users/test/.codex',
      }),
      capabilitiesByAgent: {},
      activeCapabilities: [],
      loading: false,
    });
  });

  it('shows distinct Base installations without exposing Project runtimes', () => {
    render(<AgentSelector />);

    expect(screen.getAllByRole('option').map((option) => option.textContent)).toEqual([
      'Claude Code',
      'Codex — /Users/test/.codex',
      'Codex — /Users/test/environment-codex',
    ]);
    expect(
      screen.queryByRole('option', { name: /project-home/ }),
    ).not.toBeInTheDocument();
  });

  it('selects the exact Base installation after switching Agents', () => {
    render(<AgentSelector />);

    const selector = screen.getByRole('combobox', {
      name: 'Select Agent or Base configuration instance',
    });
    fireEvent.change(selector, {
      target: { value: 'codex:/Users/test/environment-codex' },
    });
    expect(useAgents.getState().activeContext).toEqual({
      installationId: 'codex:/Users/test/environment-codex',
    });

    fireEvent.change(selector, {
      target: { value: 'claude-code:/Users/test/.claude' },
    });
    expect(useAgents.getState().activeContext).toEqual({
      installationId: 'claude-code:/Users/test/.claude',
    });

    fireEvent.change(selector, {
      target: { value: 'codex:/Users/test/environment-codex' },
    });
    expect(useAgents.getState().activeContext).toEqual({
      installationId: 'codex:/Users/test/environment-codex',
    });
  });

  it('represents a hidden Project runtime by its Base installation', () => {
    useAgents.setState({
      activeContext: AgentContextSchema.parse({
        installationId: 'codex:/Users/test/project-home',
        projectPath: '/Users/test/project',
      }),
    });

    render(<AgentSelector />);

    expect(
      screen.getByRole('combobox', {
        name: 'Select Agent or Base configuration instance',
      }),
    ).toHaveValue('codex:/Users/test/.codex');
  });
});

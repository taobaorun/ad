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
          id: 'codex:/Users/test/project-home',
          agentId: 'codex',
          rootPath: '/Users/test/project-home',
          projectPath: '/Users/test/project',
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

  it('shows and selects distinct installations for the same Agent', () => {
    render(<AgentSelector />);

    expect(screen.getByRole('option', { name: 'Codex — /Users/test/.codex' })).toBeInTheDocument();
    expect(
      screen.getByRole('option', { name: 'Codex — /Users/test/project-home' }),
    ).toBeInTheDocument();

    fireEvent.change(screen.getByRole('combobox', { name: 'Select Agent' }), {
      target: { value: 'codex:/Users/test/project-home' },
    });

    expect(useAgents.getState().activeContext).toEqual({
      installationId: 'codex:/Users/test/project-home',
      projectPath: '/Users/test/project',
    });
  });
});

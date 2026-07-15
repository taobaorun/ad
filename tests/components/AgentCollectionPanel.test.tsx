import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentCollectionPanel } from '@/components/AgentCollectionPanel';
import i18n from '@/i18n';
import {
  AgentContextSchema,
  CapabilityDescriptorSchema,
  ResourceSnapshotSchema,
} from '@/lib/agentTypes';

const { listAgentSkills, listAgentPlugins, previewAgentCollectionToggle, applyAgentPlan } =
  vi.hoisted(() => ({
    listAgentSkills: vi.fn(),
    listAgentPlugins: vi.fn(),
    previewAgentCollectionToggle: vi.fn(),
    applyAgentPlan: vi.fn(),
  }));

vi.mock('@/lib/tauri', () => ({
  tauri: {
    listAgentSkills,
    listAgentPlugins,
    previewAgentCollectionToggle,
    applyAgentPlan,
  },
}));

const context = AgentContextSchema.parse({
  installationId: 'codex:default',
  projectPath: '/Users/test/project',
});
const skill = ResourceSnapshotSchema.parse({
  resource: {
    installationId: 'codex:default',
    projectPath: '/Users/test/project',
    kind: 'skills',
    scope: 'project',
    logicalId: 'review',
  },
  location: { path: '/Users/test/project/.agents/skills/review', origin: 'project' },
  mediaType: 'application/vnd.ad.skill+json',
  content: { name: 'Review', enabled: true },
  digest: 'sha256:skill',
  observedAt: '2026-07-15T01:00:00Z',
});
const capabilities = CapabilityDescriptorSchema.array().parse([
  {
    kind: 'skills',
    scopes: ['user', 'project'],
    operations: ['list', 'enable', 'disable', 'preview', 'apply'],
    availability: 'available',
    limitations: [],
  },
  {
    kind: 'plugins',
    scopes: ['user'],
    operations: ['list', 'enable', 'disable', 'preview', 'apply'],
    availability: 'degraded',
    limitations: [
      {
        code: 'marketplace',
        messageKey: 'agents.capabilities.codexPluginInstallRequiresMarketplace',
      },
    ],
  },
]);

describe('AgentCollectionPanel', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
    listAgentSkills.mockReset().mockResolvedValue([skill]);
    listAgentPlugins.mockReset().mockResolvedValue([]);
    previewAgentCollectionToggle.mockReset().mockResolvedValue({
      id: 'plan-2',
      agentId: 'codex',
      context,
      changes: [{ resource: skill.resource, kind: 'replace' }],
      expiresAt: '2026-07-15T01:05:00Z',
    });
    applyAgentPlan.mockReset().mockResolvedValue({
      id: 'receipt-2',
      planId: 'plan-2',
      status: 'complete',
      appliedResources: [skill.resource],
      backupPaths: [],
      postApplyStates: [],
    });
  });

  it('previews a collection toggle before applying it', async () => {
    render(<AgentCollectionPanel context={context} capabilities={capabilities} />);

    fireEvent.click(await screen.findByRole('switch', { name: 'Disable Review' }));
    await waitFor(() => expect(previewAgentCollectionToggle).toHaveBeenCalledWith(context, skill.resource, false));
    expect(applyAgentPlan).not.toHaveBeenCalled();

    fireEvent.click(await screen.findByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(applyAgentPlan).toHaveBeenCalledWith('plan-2'));
  });
});

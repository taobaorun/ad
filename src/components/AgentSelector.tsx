import { useTranslation } from 'react-i18next';
import { useAgents } from '@/store/agents';

export function AgentSelector() {
  const { t } = useTranslation();
  const agents = useAgents((state) => state.agents);
  const installations = useAgents((state) => state.installations);
  const activeContext = useAgents((state) => state.activeContext);
  const activeAgentId = useAgents((state) => state.activeAgentId);
  const selectContext = useAgents((state) => state.selectContext);
  const displayNames = new Map(agents.map((agent) => [agent.id, agent.displayName]));
  const baseInstallations = installations.filter(
    (installation) => !installation.projectPath && !installation.baseInstallationId,
  );
  const installationCounts = baseInstallations.reduce<Record<string, number>>(
    (counts, installation) => {
      counts[installation.agentId] = (counts[installation.agentId] ?? 0) + 1;
      return counts;
    },
    {},
  );
  const activeInstallation = installations.find(
    (installation) => installation.id === activeContext?.installationId,
  );
  const activeBaseId = activeInstallation?.baseInstallationId ?? activeContext?.installationId;
  const selectedBaseId =
    baseInstallations.find((installation) => installation.id === activeBaseId)?.id ??
    baseInstallations.find((installation) => installation.agentId === activeAgentId)?.id ??
    '';

  return (
    <label
      className="inline-flex items-center gap-1.5 text-xs"
      title={t('app.titlebar.agentSelectorTitle')}
    >
      <span className="sr-only">{t('app.titlebar.agentSelectorLabel')}</span>
      <select
        value={selectedBaseId}
        onChange={(event) => {
          const installation = baseInstallations.find(
            (candidate) => candidate.id === event.target.value,
          );
          if (installation) {
            selectContext({ installationId: installation.id });
          }
        }}
        aria-label={t('app.titlebar.agentSelectorLabel')}
        className="rounded-lg px-2 py-1 text-xs"
        style={{
          border: '0.5px solid var(--ds-line-strong)',
          background: 'var(--ds-bg-card)',
          color: 'var(--ds-fg-2)',
          maxWidth: 130,
        }}
      >
        {baseInstallations.map((installation) => (
          <option key={installation.id} value={installation.id}>
            {displayNames.get(installation.agentId) ?? installation.agentId}
            {(installationCounts[installation.agentId] ?? 0) > 1
              ? ` — ${installation.rootPath}`
              : ''}
          </option>
        ))}
      </select>
    </label>
  );
}

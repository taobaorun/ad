import { useTranslation } from 'react-i18next';
import { useAgents } from '@/store/agents';

export function AgentSelector() {
  const { t } = useTranslation();
  const agents = useAgents((state) => state.agents);
  const installations = useAgents((state) => state.installations);
  const activeContext = useAgents((state) => state.activeContext);
  const selectContext = useAgents((state) => state.selectContext);
  const displayNames = new Map(agents.map((agent) => [agent.id, agent.displayName]));
  const installationCounts = installations.reduce<Record<string, number>>(
    (counts, installation) => {
      counts[installation.agentId] = (counts[installation.agentId] ?? 0) + 1;
      return counts;
    },
    {},
  );

  return (
    <label
      className="inline-flex items-center gap-1.5 text-xs"
      title={t('app.titlebar.agentSelectorTitle')}
    >
      <span className="sr-only">{t('app.titlebar.agentSelectorLabel')}</span>
      <select
        value={activeContext?.installationId ?? ''}
        onChange={(event) => {
          const installation = installations.find((item) => item.id === event.target.value);
          if (installation) {
            selectContext({
              installationId: installation.id,
              projectPath: installation.projectPath,
            });
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
        {installations.map((installation) => (
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

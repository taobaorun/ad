import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useAgents } from '@/store/agents';

export function AgentSelector() {
  const { t } = useTranslation();
  const agents = useAgents((state) => state.agents);
  const activeAgentId = useAgents((state) => state.activeAgentId);
  const select = useAgents((state) => state.select);

  return (
    <label className="inline-flex items-center gap-1.5 text-xs" title={t('app.titlebar.agentSelectorTitle')}>
      <span className="sr-only">{t('app.titlebar.agentSelectorLabel')}</span>
      <select
        value={activeAgentId}
        onChange={(event) => select(event.target.value)}
        aria-label={t('app.titlebar.agentSelectorLabel')}
        className="rounded-lg px-2 py-1 text-xs"
        style={{
          border: '0.5px solid var(--ds-line-strong)',
          background: 'var(--ds-bg-card)',
          color: 'var(--ds-fg-2)',
          maxWidth: 130,
        }}
      >
        {agents.map((agent) => (
          <option key={agent.id} value={agent.id}>
            {agent.displayName}
          </option>
        ))}
      </select>
    </label>
  );
}

export function useLoadAgents(): void {
  const loadAll = useAgents((state) => state.loadAll);
  useEffect(() => {
    void loadAll();
  }, [loadAll]);
}

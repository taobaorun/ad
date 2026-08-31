import { lazy, Suspense } from 'react';
import { useTranslation } from 'react-i18next';

import type { AgentContext, CapabilityDescriptor } from '@/lib/agentTypes';

import { AgentCollectionPanel } from './AgentCollectionPanel';
import { HistoryPanel } from './HistoryPanel';

const AgentSettingsEditor = lazy(() =>
  import('./AgentSettingsEditor').then((module) => ({ default: module.AgentSettingsEditor })),
);

export type ProjectWorkspaceTab = 'settings' | 'resources' | 'history';

export function ProjectWorkspacePanels({
  activeTab,
  reloadKey,
  context,
  settingsAvailable,
  capabilities,
  onOpenHistory,
}: {
  activeTab: ProjectWorkspaceTab;
  reloadKey: number;
  context: AgentContext | null;
  settingsAvailable: boolean;
  capabilities: CapabilityDescriptor[];
  onOpenHistory: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div
      className="min-h-0 flex-1"
      style={{ width: '100%', maxWidth: 1400, margin: '0 auto', padding: '0 40px 40px' }}
    >
      <section
        id="project-workspace-panel-settings"
        role="tabpanel"
        aria-labelledby="project-workspace-tab-settings"
        hidden={activeTab !== 'settings'}
        className="h-full pt-5"
      >
        {context && settingsAvailable ? (
          <Suspense fallback={<EditorSkeleton />}>
            <AgentSettingsEditor key={reloadKey} context={context} />
          </Suspense>
        ) : (
          <Unavailable>{t('agentWorkspace.settingsUnavailable')}</Unavailable>
        )}
      </section>
      <section
        id="project-workspace-panel-resources"
        role="tabpanel"
        aria-labelledby="project-workspace-tab-resources"
        hidden={activeTab !== 'resources'}
        className="h-full pt-2"
      >
        <div className="h-full rounded-b-lg border border-t-0 border-border">
          {context ? (
            <AgentCollectionPanel
              key={reloadKey}
              context={context}
              scope="project"
              capabilities={capabilities}
              onOpenHistory={onOpenHistory}
            />
          ) : (
            <Unavailable>{t('agentWorkspace.resourcesUnavailable')}</Unavailable>
          )}
        </div>
      </section>
      <section
        id="project-workspace-panel-history"
        role="tabpanel"
        aria-labelledby="project-workspace-tab-history"
        hidden={activeTab !== 'history'}
        className="h-full pt-2"
      >
        <div className="h-full overflow-hidden rounded-b-lg border border-t-0 border-border">
          <HistoryPanel />
        </div>
      </section>
    </div>
  );
}

function EditorSkeleton() {
  return (
    <div
      className="h-full w-full rounded-lg"
      style={{ background: 'var(--ds-bg-inset)', border: '0.5px solid var(--ds-line)' }}
    />
  );
}

function Unavailable({ children }: { children: string }) {
  return (
    <div
      role="status"
      className="flex h-full items-center justify-center text-sm"
      style={{ color: 'var(--ds-fg-4)' }}
    >
      {children}
    </div>
  );
}

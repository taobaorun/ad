import { useEffect, useState } from 'react';
import { ArrowRightLeft } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { useAgents } from '@/store/agents';
import { useProjects } from '@/store/projects';
import { useUiState } from '@/store/ui';

import { AgentConversionDialog } from './AgentConversionDialog';

export function AgentConversionButton() {
  const { t } = useTranslation();
  const installations = useAgents((state) => state.installations);
  const activeProjectPath = useUiState((state) => state.activeProjectPath);
  const projects = useProjects((state) => state.projects);
  const [open, setOpen] = useState(false);
  const [preferProjectScope, setPreferProjectScope] = useState(false);
  const activeProject = projects.find((project) => project.path === activeProjectPath);
  const sourceInstallations = installations.filter(
    (installation) => installation.agentId === 'claude-code',
  );
  const targetInstallations = installations.filter(
    (installation) => installation.agentId === 'codex',
  );

  useEffect(() => {
    const openForProject = () => {
      setPreferProjectScope(true);
      setOpen(true);
    };
    window.addEventListener('ad:open-project-conversion', openForProject);
    return () => window.removeEventListener('ad:open-project-conversion', openForProject);
  }, []);

  if (sourceInstallations.length === 0 || targetInstallations.length === 0) return null;

  return (
    <>
      <button
        type="button"
        onClick={() => {
          setPreferProjectScope(false);
          setOpen(true);
        }}
        className="inline-flex h-7 items-center gap-1.5 rounded-md border border-border px-2 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
        aria-label={t('agentConversion.open')}
        title={t('agentConversion.open')}
      >
        <ArrowRightLeft className="h-3.5 w-3.5" aria-hidden="true" />
        {t('agentConversion.shortLabel')}
      </button>
      <AgentConversionDialog
        open={open}
        onOpenChange={setOpen}
        sourceInstallations={sourceInstallations}
        targetInstallations={targetInstallations}
        activeProjectPath={activeProjectPath}
        inheritBaseConfig={activeProject?.inheritBaseConfig ?? true}
        preferProjectScope={preferProjectScope}
      />
    </>
  );
}

import {
  AgentContextSchema,
  type AgentContext,
  type AgentInstallation,
  type ProjectCodexRuntimeStatus,
} from '@/lib/agentTypes';

interface RuntimeInspectionLike {
  state: 'idle' | 'checking' | 'loaded' | 'error';
  runtime: ProjectCodexRuntimeStatus | null;
  error?: string | null;
}

export function resolveBaseProjectContext(
  activeContext: AgentContext | null,
  installations: AgentInstallation[],
  projectPath: string,
): AgentContext | null {
  if (!activeContext) return null;
  const activeInstallation = installations.find(
    (installation) => installation.id === activeContext.installationId,
  );
  if (activeInstallation?.projectPath && activeInstallation.projectPath !== projectPath) {
    const baseInstallation = installations.find(
      (installation) => installation.id === activeInstallation.baseInstallationId,
    );
    if (!baseInstallation) return null;
    return AgentContextSchema.parse({
      installationId: baseInstallation.id,
      projectPath,
    });
  }
  return AgentContextSchema.parse({
    installationId: activeContext.installationId,
    projectPath: activeInstallation?.projectPath ?? projectPath,
  });
}

export function resolveProjectAgentContext(
  baseContext: AgentContext | null,
  activeAgentId: string | null,
  runtime: ProjectCodexRuntimeStatus | null,
): AgentContext | null {
  if (!baseContext || activeAgentId !== 'codex' || !runtime?.prepared) {
    return baseContext;
  }
  return AgentContextSchema.parse({
    ...baseContext,
    installationId: runtime.runtimeInstallationId,
  });
}

export function isProjectRuntimeLaunchUnavailable(
  activeAgentId: string | null,
  inheritBaseConfig: boolean,
  runtimeInspection: RuntimeInspectionLike,
): boolean {
  if (activeAgentId !== 'codex') {
    return false;
  }
  if (runtimeInspection.state !== 'loaded') {
    return true;
  }
  const runtime = runtimeInspection.runtime;
  if (runtime === null) {
    return !inheritBaseConfig;
  }
  if (!runtime.prepared) {
    return !runtime.desiredInheritBaseConfig;
  }
  return runtime.needsRefresh || runtime.authMode !== 'shared_file';
}

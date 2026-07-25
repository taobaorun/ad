import { useEffect, useRef, useState } from 'react';

import { formatAgentError } from '@/lib/agentErrors';
import type { AgentContextSchema, ProjectCodexRuntimeStatus } from '@/lib/agentTypes';
import { tauri } from '@/lib/tauri';

export type RuntimeInspection =
  | { state: 'idle' | 'checking'; runtime: null; error: null }
  | { state: 'loaded'; runtime: ProjectCodexRuntimeStatus | null; error: null }
  | { state: 'error'; runtime: ProjectCodexRuntimeStatus | null; error: string };

export function useProjectCodexRuntimeInspection(
  projectContext: ReturnType<typeof AgentContextSchema.parse> | null,
  activeAgentId: string | null,
) {
  const [runtimeInspection, setRuntimeInspection] = useState<RuntimeInspection>({
    state: 'idle',
    runtime: null,
    error: null,
  });
  const generationRef = useRef(0);

  useEffect(() => {
    if (!projectContext || activeAgentId !== 'codex') {
      generationRef.current += 1;
      setRuntimeInspection({ state: 'idle', runtime: null, error: null });
      return;
    }
    const loadRuntime = () => {
      const generation = ++generationRef.current;
      setRuntimeInspection({ state: 'checking', runtime: null, error: null });
      void tauri
        .inspectProjectCodexRuntime(projectContext)
        .then((runtime) => {
          if (generation !== generationRef.current) return;
          setRuntimeInspection({ state: 'loaded', runtime, error: null });
        })
        .catch((error) => {
          if (generation !== generationRef.current) return;
          setRuntimeInspection({
            state: 'error',
            runtime: null,
            error: formatAgentError(error),
          });
        });
    };
    loadRuntime();
    window.addEventListener('ad:project-codex-runtime-changed', loadRuntime);
    return () => {
      generationRef.current += 1;
      window.removeEventListener('ad:project-codex-runtime-changed', loadRuntime);
    };
  }, [activeAgentId, projectContext]);

  return [runtimeInspection, setRuntimeInspection] as const;
}

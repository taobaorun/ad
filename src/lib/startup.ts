import { useAgents } from '@/store/agents';
import { useProfiles } from '@/store/profiles';
import { useProjects } from '@/store/projects';

export const STARTUP_DEADLINE_MS = 12_000;

export type StartupTask = 'agents' | 'profiles' | 'projects';

export interface StartupLoaders {
  loadAgents: () => Promise<void>;
  loadProfiles: () => Promise<void>;
  loadProjects: () => Promise<void>;
}

export interface StartupFailure {
  task: StartupTask;
  kind: 'rejection' | 'timeout';
  message: string;
}

export interface StartupResult {
  timedOut: boolean;
  failures: StartupFailure[];
}

interface StartupOptions {
  loaders?: StartupLoaders;
  deadlineMs?: number;
}

interface TaskState {
  settled: boolean;
  failure?: StartupFailure;
}

const TASK_ORDER: StartupTask[] = ['agents', 'profiles', 'projects'];

const DEFAULT_LOADERS: StartupLoaders = {
  loadAgents: () => useAgents.getState().loadAll(),
  loadProfiles: () => useProfiles.getState().loadAll(),
  loadProjects: () => useProjects.getState().loadAll(),
};

function failureMessage(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  if (typeof reason === 'string') return reason;
  try {
    const serialized = JSON.stringify(reason);
    if (serialized !== undefined) return serialized;
  } catch {
    // Fall through when the rejection value cannot be serialized.
  }
  return String(reason);
}

export function coordinateStartup(options: StartupOptions = {}): Promise<StartupResult> {
  const loaders = options.loaders ?? DEFAULT_LOADERS;
  const deadlineMs = options.deadlineMs ?? STARTUP_DEADLINE_MS;
  const states: Record<StartupTask, TaskState> = {
    agents: { settled: false },
    profiles: { settled: false },
    projects: { settled: false },
  };

  const attempt = (task: StartupTask, loader: () => Promise<void>): Promise<void> => {
    let load: Promise<void>;
    try {
      load = loader();
    } catch (reason) {
      load = Promise.reject(reason);
    }

    return load.then(
      () => {
        states[task].settled = true;
      },
      (reason: unknown) => {
        states[task] = {
          settled: true,
          failure: { task, kind: 'rejection', message: failureMessage(reason) },
        };
      },
    );
  };

  const agentsAttempt = attempt('agents', loaders.loadAgents);
  const projectsAttempt = attempt('projects', loaders.loadProjects);
  const profilesAttempt = agentsAttempt.then(() => attempt('profiles', loaders.loadProfiles));

  let deadlineTimer: ReturnType<typeof setTimeout>;
  const settledResult = Promise.all([agentsAttempt, profilesAttempt, projectsAttempt]).then(() => {
    clearTimeout(deadlineTimer);
    return {
      timedOut: false,
      failures: TASK_ORDER.flatMap((task) => states[task].failure ?? []),
    } satisfies StartupResult;
  });
  const timeoutResult = new Promise<StartupResult>((resolve) => {
    deadlineTimer = setTimeout(() => {
      const failures = TASK_ORDER.map((task) =>
        states[task].settled
          ? states[task].failure
          : {
              task,
              kind: 'timeout' as const,
              message: `Startup ${task} load timed out after ${deadlineMs}ms`,
            },
      ).filter((failure): failure is StartupFailure => failure !== undefined);
      resolve({ timedOut: true, failures });
    }, deadlineMs);
  });

  return Promise.race([settledResult, timeoutResult]);
}

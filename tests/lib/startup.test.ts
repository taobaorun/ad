import { afterEach, describe, expect, it, vi } from 'vitest';
import { coordinateStartup, STARTUP_DEADLINE_MS, type StartupLoaders } from '@/lib/startup';
import { injectStartupCopy, revealStartup, startStartupSpotlight } from '@/lib/startupSurface';

function deferred(): {
  promise: Promise<void>;
  resolve: () => void;
  reject: (reason: unknown) => void;
} {
  let resolve!: () => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<void>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function flushMicrotasks(): Promise<void> {
  for (let index = 0; index < 5; index += 1) await Promise.resolve();
}

describe('coordinateStartup', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('uses a 12-second production deadline', () => {
    expect(STARTUP_DEADLINE_MS).toBe(12_000);
  });

  it('starts Projects with Agents and waits to start Profiles until the Agent attempt settles', async () => {
    const agents = deferred();
    const projects = deferred();
    const profiles = deferred();
    const calls: string[] = [];
    const loaders: StartupLoaders = {
      loadAgents: vi.fn(() => {
        calls.push('agents');
        return agents.promise;
      }),
      loadProfiles: vi.fn(() => {
        calls.push('profiles');
        return profiles.promise;
      }),
      loadProjects: vi.fn(() => {
        calls.push('projects');
        return projects.promise;
      }),
    };

    const startup = coordinateStartup({ loaders, deadlineMs: 1_000 });

    expect(calls).toEqual(['agents', 'projects']);
    projects.resolve();
    await Promise.resolve();
    expect(loaders.loadProfiles).not.toHaveBeenCalled();

    agents.resolve();
    await flushMicrotasks();
    expect(calls).toEqual(['agents', 'projects', 'profiles']);
    profiles.resolve();

    await expect(startup).resolves.toEqual({ timedOut: false, failures: [] });
    expect(loaders.loadAgents).toHaveBeenCalledTimes(1);
    expect(loaders.loadProfiles).toHaveBeenCalledTimes(1);
    expect(loaders.loadProjects).toHaveBeenCalledTimes(1);
  });

  it.each([
    ['agents', new Error('Agent discovery failed')],
    ['profiles', 'Profile read failed'],
    ['projects', { reason: 'Project read failed' }],
  ] as const)('returns rejection metadata when %s fails', async (failedTask, reason) => {
    const loaders: StartupLoaders = {
      loadAgents: vi.fn(() =>
        failedTask === 'agents' ? Promise.reject(reason) : Promise.resolve(),
      ),
      loadProfiles: vi.fn(() =>
        failedTask === 'profiles' ? Promise.reject(reason) : Promise.resolve(),
      ),
      loadProjects: vi.fn(() =>
        failedTask === 'projects' ? Promise.reject(reason) : Promise.resolve(),
      ),
    };

    const result = await coordinateStartup({ loaders, deadlineMs: 1_000 });

    expect(result.timedOut).toBe(false);
    expect(result.failures).toEqual([
      {
        task: failedTask,
        kind: 'rejection',
        message:
          reason instanceof Error
            ? reason.message
            : typeof reason === 'string'
              ? reason
              : JSON.stringify(reason),
      },
    ]);
    expect(loaders.loadProfiles).toHaveBeenCalledTimes(1);
  });

  it('collects multiple rejection failures without rejecting the coordinator', async () => {
    const loaders: StartupLoaders = {
      loadAgents: vi.fn().mockRejectedValue(new Error('Agent discovery failed')),
      loadProfiles: vi.fn().mockRejectedValue(new Error('Profile read failed')),
      loadProjects: vi.fn().mockRejectedValue(new Error('Project read failed')),
    };

    await expect(coordinateStartup({ loaders, deadlineMs: 1_000 })).resolves.toEqual({
      timedOut: false,
      failures: [
        { task: 'agents', kind: 'rejection', message: 'Agent discovery failed' },
        { task: 'profiles', kind: 'rejection', message: 'Profile read failed' },
        { task: 'projects', kind: 'rejection', message: 'Project read failed' },
      ],
    });
  });

  it('returns timeout metadata at the deadline without cancelling late idempotent loads', async () => {
    vi.useFakeTimers();
    const agents = deferred();
    const projects = deferred();
    const profiles = deferred();
    let lateStoreUpdates = 0;
    const loaders: StartupLoaders = {
      loadAgents: vi.fn(async () => {
        await agents.promise;
        lateStoreUpdates += 1;
      }),
      loadProfiles: vi.fn(async () => {
        await profiles.promise;
        lateStoreUpdates += 1;
      }),
      loadProjects: vi.fn(async () => {
        await projects.promise;
        lateStoreUpdates += 1;
      }),
    };

    const startup = coordinateStartup({ loaders, deadlineMs: 25 });
    await vi.advanceTimersByTimeAsync(25);

    await expect(startup).resolves.toEqual({
      timedOut: true,
      failures: [
        { task: 'agents', kind: 'timeout', message: 'Startup agents load timed out after 25ms' },
        {
          task: 'profiles',
          kind: 'timeout',
          message: 'Startup profiles load timed out after 25ms',
        },
        {
          task: 'projects',
          kind: 'timeout',
          message: 'Startup projects load timed out after 25ms',
        },
      ],
    });
    expect(loaders.loadProfiles).not.toHaveBeenCalled();

    agents.resolve();
    projects.resolve();
    await flushMicrotasks();
    expect(loaders.loadProfiles).toHaveBeenCalledTimes(1);
    profiles.resolve();
    await flushMicrotasks();

    expect(lateStoreUpdates).toBe(3);
    expect(loaders.loadAgents).toHaveBeenCalledTimes(1);
    expect(loaders.loadProfiles).toHaveBeenCalledTimes(1);
    expect(loaders.loadProjects).toHaveBeenCalledTimes(1);
  });
});

describe('startup surface helpers', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = '';
  });

  it('injects localized quote and status copy into the static splash', () => {
    document.body.innerHTML = `
      <p id="ad-splash-quote">fallback</p>
      <div id="ad-splash-status">fallback status</div>
    `;

    injectStartupCopy(document, {
      quote: 'Be Water, My Friend',
      status: '正在加载 AD',
    });

    expect(document.getElementById('ad-splash-quote')?.textContent).toBe('Be Water, My Friend');
    expect(document.getElementById('ad-splash-status')?.textContent).toBe('正在加载 AD');
  });

  it('drives the quote spotlight without relying on WKWebView media queries', () => {
    document.body.innerHTML = '<p id="ad-splash-quote"></p>';
    const frames: Array<(timestamp: number) => void> = [];
    const requestFrame = vi.fn((callback: (timestamp: number) => void) => {
      frames.push(callback);
      return frames.length;
    });

    startStartupSpotlight(document, { requestFrame });
    const quote = document.getElementById('ad-splash-quote')!;

    expect(quote.style.animation).toBe('none');
    expect(quote.style.backgroundPosition).toBe('10% 0px');
    frames.shift()!(0);
    const firstPosition = quote.style.backgroundPosition;
    frames.shift()!(225);
    expect(quote.style.backgroundPosition).not.toBe(firstPosition);
    expect(requestFrame).toHaveBeenCalledTimes(3);
  });

  it('keeps the quote static when reduced motion is requested', () => {
    document.body.innerHTML = '<p id="ad-splash-quote"></p>';
    const requestFrame = vi.fn();

    startStartupSpotlight(document, { requestFrame, reduceMotion: true });

    const quote = document.getElementById('ad-splash-quote')!;
    expect(quote.style.animation).toBe('none');
    expect(quote.style.backgroundPosition).toBe('10% 0px');
    expect(requestFrame).not.toHaveBeenCalled();
  });

  it('uses the production media query to detect reduced motion', () => {
    document.body.innerHTML = '<p id="ad-splash-quote"></p>';
    const requestFrame = vi.fn();
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: true })),
    );

    startStartupSpotlight(document, { requestFrame });

    expect(matchMedia).toHaveBeenCalledWith('(prefers-reduced-motion: reduce)');
    expect(requestFrame).not.toHaveBeenCalled();
  });

  it('reveals the React root after two frames and removes the splash after its exit', async () => {
    vi.useFakeTimers();
    document.body.innerHTML = `
      <div id="ad-splash"><div id="ad-splash-child"></div></div>
      <div id="root" inert aria-hidden="true"></div>
    `;
    const frames: Array<(timestamp: number) => void> = [];
    const requestFrame = vi.fn((callback: (timestamp: number) => void) => {
      frames.push(callback);
      return frames.length;
    });

    const revealing = revealStartup(document, { requestFrame, exitDurationMs: 180 });
    const root = document.getElementById('root')!;
    const splash = document.getElementById('ad-splash')!;

    expect(requestFrame).toHaveBeenCalledTimes(1);
    expect(root.hasAttribute('inert')).toBe(true);
    frames.shift()!(0);
    expect(requestFrame).toHaveBeenCalledTimes(2);
    expect(root.hasAttribute('inert')).toBe(true);
    frames.shift()!(16);
    await Promise.resolve();

    expect(root.hasAttribute('inert')).toBe(true);
    expect(root.getAttribute('aria-hidden')).toBe('true');
    expect(root.classList.contains('ad-app-enter')).toBe(true);
    expect(splash.classList.contains('ad-splash-exit')).toBe(true);
    expect(document.getElementById('ad-splash')).toBe(splash);

    document
      .getElementById('ad-splash-child')!
      .dispatchEvent(new Event('transitionend', { bubbles: true }));
    await Promise.resolve();
    expect(document.getElementById('ad-splash')).toBe(splash);

    splash.dispatchEvent(new Event('transitionend'));
    await revealing;
    expect(document.getElementById('ad-splash')).toBeNull();
    expect(root.hasAttribute('inert')).toBe(false);
    expect(root.hasAttribute('aria-hidden')).toBe(false);
    expect(root.classList.contains('ad-app-enter')).toBe(true);
  });

  it('removes the exiting splash after the transition timeout fallback', async () => {
    vi.useFakeTimers();
    document.body.innerHTML = `
      <div id="ad-splash"></div>
      <div id="root" inert aria-hidden="true"></div>
    `;
    const requestFrame = (callback: (timestamp: number) => void) => {
      callback(0);
      return 1;
    };

    const revealing = revealStartup(document, { requestFrame, exitDurationMs: 180 });
    await vi.advanceTimersByTimeAsync(180);
    await revealing;

    expect(document.getElementById('ad-splash')).toBeNull();
    expect(document.getElementById('root')?.hasAttribute('inert')).toBe(false);
    expect(document.getElementById('root')?.hasAttribute('aria-hidden')).toBe(false);
  });

  it('restores root accessibility when the splash is already absent', async () => {
    document.body.innerHTML = '<div id="root" inert aria-hidden="true"></div>';
    const requestFrame = (callback: (timestamp: number) => void) => {
      callback(0);
      return 1;
    };

    await revealStartup(document, { requestFrame });

    const root = document.getElementById('root')!;
    expect(root.classList.contains('ad-app-enter')).toBe(true);
    expect(root.hasAttribute('inert')).toBe(false);
    expect(root.hasAttribute('aria-hidden')).toBe(false);
  });
});

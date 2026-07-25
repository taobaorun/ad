interface StartupCopy {
  quote: string;
  status: string;
}

interface StartupFailureCopy {
  title: string;
  message: string;
  retry: string;
}

interface StartupFailureOptions {
  reload?: () => void;
}

interface RevealStartupOptions {
  requestFrame?: (callback: (timestamp: number) => void) => number;
  exitDurationMs?: number;
}

interface StartupSpotlightOptions {
  requestFrame?: (callback: (timestamp: number) => void) => number;
  reduceMotion?: boolean;
}

const SPOTLIGHT_STEP_MS = 10;
const SPOTLIGHT_STEP_COUNT = 30;
const SPOTLIGHT_CYCLE_MS = SPOTLIGHT_STEP_MS * SPOTLIGHT_STEP_COUNT;

export function injectStartupCopy(documentRef: Document, copy: StartupCopy): void {
  const quote = documentRef.getElementById('ad-splash-quote');
  if (quote) quote.textContent = copy.quote;
  const status = documentRef.getElementById('ad-splash-status');
  if (status) status.textContent = copy.status;
}

export function showStartupFailure(
  documentRef: Document,
  copy: StartupFailureCopy,
  options: StartupFailureOptions = {},
): void {
  documentRef.getElementById('ad-splash')?.remove();
  const root = documentRef.getElementById('root');
  if (!root) return;

  const panel = documentRef.createElement('section');
  panel.setAttribute('role', 'alert');
  panel.className =
    'flex min-h-screen flex-col items-center justify-center gap-3 bg-background px-6 text-center text-foreground';
  const title = documentRef.createElement('h1');
  title.className = 'text-xl font-semibold';
  title.textContent = copy.title;
  const message = documentRef.createElement('p');
  message.className = 'max-w-md text-sm text-muted-foreground';
  message.textContent = copy.message;
  const retry = documentRef.createElement('button');
  retry.type = 'button';
  retry.className =
    'rounded-md border border-border bg-card px-4 py-2 text-sm font-medium hover:bg-muted';
  retry.textContent = copy.retry;
  retry.addEventListener('click', () => {
    if (options.reload) {
      options.reload();
    } else {
      documentRef.defaultView?.location.reload();
    }
  });
  panel.append(title, message, retry);
  root.replaceChildren(panel);
  root.classList.add('ad-app-enter');
  root.removeAttribute('inert');
  root.removeAttribute('aria-hidden');
}

export function startStartupSpotlight(
  documentRef: Document,
  options: StartupSpotlightOptions = {},
): void {
  const quote = documentRef.getElementById('ad-splash-quote');
  if (!quote) return;

  const requestFrame = options.requestFrame ?? requestAnimationFrame;
  let startedAt: number | undefined;
  quote.style.animation = 'none';
  quote.style.backgroundPosition = '10% 0';
  const reduceMotion =
    options.reduceMotion ?? globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
  if (reduceMotion) return;

  const tick = (timestamp: number) => {
    if (!quote.isConnected) return;
    startedAt ??= timestamp - SPOTLIGHT_CYCLE_MS / 2;
    const elapsedMs = (timestamp - startedAt) % SPOTLIGHT_CYCLE_MS;
    const spotlightStep = Math.floor(elapsedMs / SPOTLIGHT_STEP_MS);
    const quoteProgress = spotlightStep / SPOTLIGHT_STEP_COUNT;
    quote.style.backgroundPosition = `${140 - quoteProgress * 260}% 0`;
    requestFrame(tick);
  };

  requestFrame(tick);
}

export async function revealStartup(
  documentRef: Document,
  options: RevealStartupOptions = {},
): Promise<void> {
  const requestFrame = options.requestFrame ?? requestAnimationFrame;
  const exitDurationMs = options.exitDurationMs ?? 260;

  await new Promise<void>((resolve) => {
    requestFrame(() => requestFrame(() => resolve()));
  });

  const splash = documentRef.getElementById('ad-splash');
  const root = documentRef.getElementById('root');
  root?.classList.add('ad-app-enter');
  if (!splash) {
    root?.removeAttribute('inert');
    root?.removeAttribute('aria-hidden');
    return;
  }
  splash.classList.add('ad-splash-exit');

  await new Promise<void>((resolve) => {
    const finish = (event?: Event) => {
      if (event && event.target !== splash) return;
      clearTimeout(timeout);
      splash.removeEventListener('transitionend', finish);
      resolve();
    };
    const timeout = setTimeout(finish, exitDurationMs);
    splash.addEventListener('transitionend', finish);
  });
  splash.remove();
  root?.removeAttribute('inert');
  root?.removeAttribute('aria-hidden');
}

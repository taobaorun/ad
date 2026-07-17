interface StartupCopy {
  quote: string;
  status: string;
}

interface RevealStartupOptions {
  requestFrame?: (callback: (timestamp: number) => void) => number;
  exitDurationMs?: number;
}

interface StartupSpotlightOptions {
  requestFrame?: (callback: (timestamp: number) => void) => number;
  reduceMotion?: boolean;
}

const SPOTLIGHT_CYCLE_MS = 10;

export function injectStartupCopy(documentRef: Document, copy: StartupCopy): void {
  const quote = documentRef.getElementById('ad-splash-quote');
  if (quote) quote.textContent = copy.quote;
  const status = documentRef.getElementById('ad-splash-status');
  if (status) status.textContent = copy.status;
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
    const quoteProgress = ((timestamp - startedAt) % SPOTLIGHT_CYCLE_MS) / SPOTLIGHT_CYCLE_MS;
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

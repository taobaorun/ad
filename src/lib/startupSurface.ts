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
  minimumVisibleMs?: number;
  startedAtMs?: number;
  now?: () => number;
}

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
export async function revealStartup(
  documentRef: Document,
  options: RevealStartupOptions = {},
): Promise<void> {
  const requestFrame = options.requestFrame ?? requestAnimationFrame;
  const exitDurationMs = options.exitDurationMs ?? 260;
  const minimumVisibleMs = options.minimumVisibleMs ?? 0;
  const now = options.now ?? (() => performance.now());
  const startedAtMs = options.startedAtMs ?? now();

  await new Promise<void>((resolve) => {
    requestFrame(() => requestFrame(() => resolve()));
  });

  const remainingVisibleMs = minimumVisibleMs - (now() - startedAtMs);
  if (remainingVisibleMs > 0) {
    await new Promise<void>((resolve) => setTimeout(resolve, remainingVisibleMs));
  }

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

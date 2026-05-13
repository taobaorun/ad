import { useEffect } from 'react';
import { useProfiles, type ToastPayload } from '@/store/profiles';
import { Button } from './ui/button';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { notifyActivation, RESTART_COMMAND } from '@/lib/notifications';
import { Copy, X } from 'lucide-react';

const TOAST_TTL_MS = 8000;

export function ActivateToast() {
  const toasts = useProfiles((s) => s.toasts);

  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex flex-col gap-2">
      {toasts.map((toast) => (
        <ToastCard key={toast.id} toast={toast} />
      ))}
    </div>
  );
}

function ToastCard({ toast }: { toast: ToastPayload }) {
  const dismiss = useProfiles((s) => s.dismissToast);

  useEffect(() => {
    void notifyActivation(toast.profileName);
    const t = setTimeout(() => dismiss(toast.id), TOAST_TTL_MS);
    return () => clearTimeout(t);
    // dismiss is a stable store action; toast.id and profileName drive identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [toast.id]);

  return (
    <div
      role="status"
      aria-live="polite"
      className="pointer-events-auto w-96 rounded-lg border border-border bg-card p-4 shadow-lg"
    >
      <div className="flex items-start gap-3">
        <div className="flex-1">
          <div className="font-semibold">Activated: {toast.profileName}</div>
          <p className="mt-1 text-sm text-muted-foreground">
            Restart Claude Code to apply the new env.
          </p>
          {toast.detected.length > 0 ? (
            <>
              <p className="mt-2 text-xs font-medium">Running Claude processes:</p>
              <ul className="mt-1 space-y-0.5 text-xs text-muted-foreground">
                {toast.detected.map((p) => (
                  <li key={p.pid} className="font-mono">
                    {p.pid} {p.cmd.slice(0, 60)}
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p className="mt-2 text-xs text-muted-foreground">
              No running Claude processes detected.
            </p>
          )}
          <div className="mt-3 flex gap-2">
            <Button size="sm" variant="outline" onClick={() => void writeText(RESTART_COMMAND)}>
              <Copy className="h-3.5 w-3.5" />
              Copy restart command
            </Button>
          </div>
        </div>
        <button
          type="button"
          onClick={() => dismiss(toast.id)}
          aria-label="Dismiss"
          className="rounded p-1 text-muted-foreground hover:bg-muted"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  );
}

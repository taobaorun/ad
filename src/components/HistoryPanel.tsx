import { useEffect, useState } from 'react';
import { tauri, type ActivationLogEntry } from '@/lib/tauri';
import { Button } from './ui/button';
import { RotateCcw, RefreshCw } from 'lucide-react';
import { useProfiles } from '@/store/profiles';

export function HistoryPanel() {
  const [entries, setEntries] = useState<ActivationLogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const loadAll = useProfiles((s) => s.loadAll);

  async function refresh() {
    setBusy(true);
    setError(null);
    try {
      setEntries(await tauri.readHistory(50));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  async function onRestore(backup: string | null | undefined) {
    if (!backup) return;
    if (
      !window.confirm(
        'Restore this backup? Your current settings.json will itself be backed up first.',
      )
    )
      return;
    setBusy(true);
    try {
      await tauri.restoreBackup(backup);
      await loadAll();
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-border p-3">
        <h2 className="text-sm font-semibold">Activation history</h2>
        <Button variant="ghost" size="sm" onClick={() => void refresh()} disabled={busy}>
          <RefreshCw className="h-4 w-4" />
          Refresh
        </Button>
      </div>
      {error && (
        <div className="bg-destructive/10 px-3 py-1.5 text-xs text-destructive">{error}</div>
      )}
      <div className="flex-1 overflow-y-auto p-3">
        {entries.length === 0 ? (
          <div className="text-sm text-muted-foreground">
            No history yet. Activate a profile to start.
          </div>
        ) : (
          <ul className="space-y-1">
            {entries.map((e, idx) => (
              <li
                key={idx}
                className="flex items-center gap-3 rounded border border-border bg-card px-3 py-2 text-sm"
              >
                <span className="font-mono text-xs text-muted-foreground">
                  {new Date(e.ts).toLocaleString()}
                </span>
                <span className="flex-1">
                  <span className="text-muted-foreground">{e.from ?? '—'}</span>
                  <span className="mx-2">→</span>
                  <span className="font-medium">{e.to}</span>
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!e.backupPath || busy}
                  onClick={() => void onRestore(e.backupPath)}
                  title={e.backupPath ?? 'no backup'}
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                  Restore
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

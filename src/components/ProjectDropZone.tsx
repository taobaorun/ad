import { useEffect, useState } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { useProjects } from '@/store/projects';
import { Upload } from 'lucide-react';

/**
 * Drop zone for adding a single project via drag-and-drop (D12 second path).
 *
 * Tauri 2's webview emits drag/drop events globally — we listen and only
 * react when our zone is the visual target (we don't actually hit-test the
 * coordinates; the user's intent is clear when they drop on AD's window).
 */
export function ProjectDropZone() {
  const [hover, setHover] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const addProject = useProjects((s) => s.addProject);

  useEffect(() => {
    let unlisten: (() => void) | null = null;

    void (async () => {
      const u = await getCurrentWebview().onDragDropEvent(async (event) => {
        const payload = event.payload;
        if (payload.type === 'over' || payload.type === 'enter') {
          setHover(true);
          return;
        }
        if (payload.type === 'leave') {
          setHover(false);
          return;
        }
        if (payload.type === 'drop') {
          setHover(false);
          const paths = payload.paths;
          if (paths.length === 0) return;
          if (paths.length > 1) {
            setError('drop a single folder, not multiple');
            window.setTimeout(() => setError(null), 2500);
            return;
          }
          const first = paths[0];
          if (!first) return;
          try {
            await addProject(first);
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
            window.setTimeout(() => setError(null), 2500);
          }
        }
      });
      unlisten = u;
    })();

    return () => {
      if (unlisten) unlisten();
    };
  }, [addProject]);

  return (
    <div
      role="region"
      aria-label="Drop a folder here"
      className={
        'flex h-12 items-center justify-center gap-2 rounded border border-dashed text-xs transition-colors ' +
        (hover
          ? 'border-clay bg-clay/10 text-clay'
          : 'border-border text-muted-foreground')
      }
    >
      <Upload className="h-3.5 w-3.5" />
      {error ? (
        <span className="text-rust">{error}</span>
      ) : (
        <span>Drop a single folder anywhere on this window to add</span>
      )}
    </div>
  );
}

import { useState } from 'react';
import { Dialog } from './ui/dialog';
import { Button } from './ui/button';
import { useUiSettings } from '@/store/uiSettings';
import { Settings as SettingsIcon } from 'lucide-react';

/**
 * Advanced / preferences dialog (M5).
 *
 * Currently exposes a single toggle: whether to show the legacy
 * [Activate (legacy)] button in ProfileEditor. The legacy command itself
 * (`activate_profile`) stays registered and unchanged — this only controls
 * UI visibility.
 */
export function AdvancedSettingsButton() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        title="Advanced settings"
        aria-label="Advanced settings"
        className="rounded p-1 text-muted-foreground hover:bg-muted"
      >
        <SettingsIcon className="h-4 w-4" />
      </button>
      <AdvancedSettingsDialog open={open} onOpenChange={setOpen} />
    </>
  );
}

function AdvancedSettingsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const show = useUiSettings((s) => s.showLegacyActivation);
  const setShow = useUiSettings((s) => s.setShowLegacyActivation);

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Advanced settings" size="md">
      <div className="flex flex-col gap-4 text-sm">
        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            Legacy
          </h3>
          <label className="flex items-start gap-3 rounded border border-border p-3">
            <input
              type="checkbox"
              checked={show}
              onChange={(e) => setShow(e.target.checked)}
              className="mt-0.5"
            />
            <div>
              <div className="font-medium">Show legacy global activation</div>
              <p className="mt-1 text-xs text-muted-foreground">
                Adds an <code>[Activate]</code> button to the profile editor that overwrites your
                global <code>~/.claude/settings.json</code>. This is the v0.1 behavior — it affects{' '}
                <em>all</em> Claude Code instances and is deprecated in favor of the per-project
                Apply flow on the right.
              </p>
            </div>
          </label>
        </section>

        <div className="flex justify-end">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

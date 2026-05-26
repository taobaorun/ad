/**
 * Modal for switching the template that initialized a project.
 *
 * Author: taobaorun
 *
 * Picks a template from the global pool, calls apply_profile_to_project
 * with all three layers (shared / local / env), and handles the three
 * apply outcomes (Applied / NeedsResolution / GitDirtyBlocked) inline.
 *
 * On success, calls onApplied so the host can refresh both the editor
 * (key bump → reload) and the projects store (currentProfileId update).
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog } from './ui/dialog';
import { Button } from './ui/button';
import { useProfiles } from '@/store/profiles';
import { tauri } from '@/lib/tauri';
import { InlineConflictResolver } from './InlineConflictResolver';
import type { Conflict, Resolution } from '@/lib/projectTypes';
import type { ProfileFile } from '@/lib/profileSchema';
import { AlertTriangle } from 'lucide-react';

export interface SwitchTemplateDialogProps {
  open: boolean;
  projectPath: string;
  currentProfileId: string | null;
  onOpenChange: (open: boolean) => void;
  /** Called after a successful apply — host should reload its data. */
  onApplied: () => void;
}

type LayerKey = 'shared' | 'local' | 'env';
const ALL_LAYERS: readonly LayerKey[] = ['shared', 'local', 'env'] as const;
// Default: local + env (writes to settings.local.json, gitignored). Shared is
// opt-in because it touches settings.json — most users don't want CC config
// committed to their repo.
const DEFAULT_LAYERS: readonly LayerKey[] = ['local', 'env'] as const;

export function SwitchTemplateDialog({
  open,
  projectPath,
  currentProfileId,
  onOpenChange,
  onApplied,
}: SwitchTemplateDialogProps) {
  const { t } = useTranslation();
  const profiles = useProfiles((s) => s.profiles);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [layers, setLayers] = useState<Set<LayerKey>>(new Set(DEFAULT_LAYERS));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pendingConflicts, setPendingConflicts] = useState<{
    layer: string;
    conflicts: Conflict[];
  } | null>(null);
  const [resolutions, setResolutions] = useState<Record<string, Resolution>>({});
  const [dirtyAcked, setDirtyAcked] = useState(false);
  const [dirtyBlocked, setDirtyBlocked] = useState<string | null>(null);

  function reset() {
    setSelectedId(null);
    setLayers(new Set(DEFAULT_LAYERS));
    setBusy(false);
    setError(null);
    setPendingConflicts(null);
    setResolutions({});
    setDirtyAcked(false);
    setDirtyBlocked(null);
  }

  function toggleLayer(layer: LayerKey) {
    const next = new Set(layers);
    if (next.has(layer)) next.delete(layer);
    else next.add(layer);
    setLayers(next);
  }

  async function applyTemplate(templateId: string) {
    if (layers.size === 0) {
      setError(t('switchTemplate.layers.noneSelected'));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // Iterate ALL_LAYERS to keep a stable order (shared → local → env) so
      // backups and conflict reporting are consistent regardless of which Set
      // mutation order produced.
      const orderedLayers = ALL_LAYERS.filter((l) => layers.has(l));
      const outcome = await tauri.applyProfileToProject(templateId, projectPath, {
        layers: orderedLayers,
        resolutions,
        overwriteDirtyWarningAcked: dirtyAcked,
      });
      if (outcome.kind === 'applied') {
        onApplied();
        onOpenChange(false);
        reset();
      } else if (outcome.kind === 'needsResolution') {
        setPendingConflicts({ layer: outcome.layer, conflicts: outcome.conflicts });
        setResolutions((prev) => {
          const next = { ...prev };
          for (const c of outcome.conflicts) {
            if (!next[c.keyPath]) next[c.keyPath] = { kind: 'keepExisting' };
          }
          return next;
        });
      } else {
        setDirtyBlocked(outcome.message);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  const selected = profiles.find((p) => p.id === selectedId) ?? null;

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v) reset();
        onOpenChange(v);
      }}
      title={t('switchTemplate.title')}
      description={t('switchTemplate.description')}
      size="lg"
    >
      {!selectedId ? (
        <TemplateList
          profiles={profiles}
          currentProfileId={currentProfileId}
          onPick={(id) => setSelectedId(id)}
        />
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
          <ChosenTemplateBanner profile={selected} onCancel={reset} />

          <LayerPicker layers={layers} onToggle={toggleLayer} />

          {dirtyBlocked && (
            <div className="rounded border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-700 dark:text-amber-300">
              <div className="mb-1 flex items-center gap-1.5 font-medium">
                <AlertTriangle className="h-3.5 w-3.5" />
                {t('switchTemplate.dirtyTitle')}
              </div>
              <p className="text-xs opacity-80">{dirtyBlocked}</p>
              <label className="mt-2 flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={dirtyAcked}
                  onChange={(e) => setDirtyAcked(e.target.checked)}
                />
                {t('switchTemplate.dirtyAck')}
              </label>
            </div>
          )}

          {pendingConflicts && (
            <InlineConflictResolver
              conflicts={pendingConflicts.conflicts}
              resolutions={resolutions}
              onChange={setResolutions}
            />
          )}

          {error && (
            <div className="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {error}
            </div>
          )}

          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
            <Button variant="outline" size="sm" onClick={reset} disabled={busy}>
              {t('switchTemplate.cancel')}
            </Button>
            <Button
              size="sm"
              onClick={() => void applyTemplate(selected!.id)}
              disabled={busy || layers.size === 0 || (dirtyBlocked != null && !dirtyAcked)}
            >
              {busy
                ? t('switchTemplate.applying')
                : pendingConflicts
                  ? t('switchTemplate.applyWithResolutions')
                  : t('switchTemplate.apply')}
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}

function LayerPicker({
  layers,
  onToggle,
}: {
  layers: Set<LayerKey>;
  onToggle: (layer: LayerKey) => void;
}) {
  const { t } = useTranslation();
  const items: { key: LayerKey; gitTracked: boolean }[] = [
    { key: 'shared', gitTracked: true },
    { key: 'local', gitTracked: false },
    { key: 'env', gitTracked: false },
  ];
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
        padding: '12px 14px',
        borderRadius: 8,
        border: '0.5px solid var(--ds-line)',
        background: 'var(--ds-bg-card)',
      }}
    >
      <div
        className="font-mono uppercase"
        style={{ fontSize: 10.5, letterSpacing: '0.08em', color: 'var(--ds-fg-4)' }}
      >
        {t('switchTemplate.layers.title')}
      </div>
      {items.map((item) => {
        const checked = layers.has(item.key);
        return (
          <label
            key={item.key}
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              gap: 10,
              cursor: 'pointer',
              padding: '4px 2px',
            }}
          >
            <input
              type="checkbox"
              checked={checked}
              onChange={() => onToggle(item.key)}
              style={{ marginTop: 3 }}
            />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span style={{ fontSize: 13, fontWeight: 500, color: 'hsl(var(--foreground))' }}>
                  {t(`switchTemplate.layers.${item.key}.label`)}
                </span>
                {item.gitTracked && (
                  <span
                    className="font-mono"
                    style={{
                      fontSize: 10,
                      padding: '1px 6px',
                      borderRadius: 4,
                      background: 'var(--ds-warning-soft)',
                      color: 'var(--ds-warning)',
                      border: '0.5px solid rgba(194,65,12,0.18)',
                    }}
                  >
                    {t('switchTemplate.layers.gitTracked')}
                  </span>
                )}
              </div>
              <div style={{ fontSize: 11.5, color: 'var(--ds-fg-3)', marginTop: 2 }}>
                {t(`switchTemplate.layers.${item.key}.desc`)}
              </div>
            </div>
          </label>
        );
      })}
    </div>
  );
}

function TemplateList({
  profiles,
  currentProfileId,
  onPick,
}: {
  profiles: ProfileFile[];
  currentProfileId: string | null;
  onPick: (id: string) => void;
}) {
  const { t } = useTranslation();
  if (profiles.length === 0) {
    return (
      <div className="py-8 text-center text-sm text-muted-foreground">
        {t('switchTemplate.empty')}
      </div>
    );
  }
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6, maxHeight: 360, overflowY: 'auto' }}>
      {profiles.map((p) => {
        const isCurrent = p.id === currentProfileId;
        return (
          <button
            key={p.id}
            type="button"
            onClick={() => onPick(p.id)}
            style={{
              display: 'grid',
              gridTemplateColumns: 'auto 1fr auto',
              alignItems: 'center',
              gap: 12,
              padding: '10px 14px',
              borderRadius: 8,
              border: '0.5px solid var(--ds-line)',
              background: 'var(--ds-bg-card)',
              cursor: 'pointer',
              textAlign: 'left',
              fontFamily: 'inherit',
            }}
          >
            <span style={{ width: 10, height: 10, borderRadius: '50%', background: p.color }} />
            <div>
              <div style={{ fontSize: 13.5, fontWeight: 500, color: 'hsl(var(--foreground))' }}>
                {p.displayName}
              </div>
              {p.description && (
                <div style={{ fontSize: 11.5, color: 'var(--ds-fg-3)', marginTop: 2 }}>
                  {p.description}
                </div>
              )}
            </div>
            {isCurrent && (
              <span
                className="font-mono"
                style={{
                  fontSize: 10.5,
                  padding: '2px 7px',
                  borderRadius: 5,
                  background: 'var(--ds-accent-soft)',
                  color: 'var(--ds-accent)',
                  border: '0.5px solid var(--ds-accent-line)',
                }}
              >
                {t('switchTemplate.currentTag')}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

function ChosenTemplateBanner({ profile, onCancel }: { profile: ProfileFile | null; onCancel: () => void }) {
  const { t } = useTranslation();
  if (!profile) return null;
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        padding: '10px 14px',
        borderRadius: 8,
        background: 'var(--ds-bg-inset)',
        border: '0.5px solid var(--ds-line)',
      }}
    >
      <span style={{ width: 10, height: 10, borderRadius: '50%', background: profile.color }} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 500, color: 'hsl(var(--foreground))' }}>
          {profile.displayName}
        </div>
        {profile.description && (
          <div style={{ fontSize: 11.5, color: 'var(--ds-fg-3)', marginTop: 2 }}>
            {profile.description}
          </div>
        )}
      </div>
      <Button variant="ghost" size="sm" onClick={onCancel}>
        {t('switchTemplate.changeTemplate')}
      </Button>
    </div>
  );
}

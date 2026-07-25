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

  const inPickerStep = !selectedId;
  const footer = inPickerStep ? null : (
    <div className="flex justify-end gap-2">
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
  );

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
      footer={footer}
    >
      {inPickerStep ? (
        <TemplateList
          profiles={profiles}
          currentProfileId={currentProfileId}
          onPick={(id) => setSelectedId(id)}
        />
      ) : (
        <div className="flex flex-col gap-3.5">
          <ChosenTemplateBanner profile={selected} onCancel={reset} />

          <LayerPicker layers={layers} onToggle={toggleLayer} />

          {dirtyBlocked && (
            <div className="rounded-md border border-warning/40 bg-warning/10 p-3 text-xs text-foreground">
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
            <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {error}
            </div>
          )}
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
    <div className="flex flex-col gap-2 rounded-md border border-border bg-muted/30 px-3.5 py-3">
      <div className="font-mono text-[10.5px] uppercase tracking-wider text-muted-foreground">
        {t('switchTemplate.layers.title')}
      </div>
      {items.map((item) => {
        const checked = layers.has(item.key);
        return (
          <label key={item.key} className="flex cursor-pointer items-start gap-2.5 px-0.5 py-1">
            <input
              type="checkbox"
              checked={checked}
              onChange={() => onToggle(item.key)}
              className="mt-[3px]"
            />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="text-[13px] font-medium text-foreground">
                  {t(`switchTemplate.layers.${item.key}.label`)}
                </span>
                {item.gitTracked && (
                  <span className="rounded border border-warning/30 bg-warning/10 px-1.5 py-px font-mono text-[10px] text-foreground">
                    {t('switchTemplate.layers.gitTracked')}
                  </span>
                )}
              </div>
              <div className="mt-0.5 text-[11.5px] text-muted-foreground">
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
    <div className="flex flex-col gap-1.5">
      {profiles.map((p) => {
        const isCurrent = p.id === currentProfileId;
        return (
          <button
            key={p.id}
            type="button"
            onClick={() => onPick(p.id)}
            className="grid cursor-pointer grid-cols-[auto_1fr_auto] items-center gap-3 rounded-md border border-border bg-muted/30 px-3.5 py-2.5 text-left transition-colors hover:border-primary/60"
          >
            <span className="h-2.5 w-2.5 rounded-full" style={{ background: p.color }} />
            <div>
              <div className="text-[13.5px] font-medium text-foreground">{p.displayName}</div>
              {p.description && (
                <div className="mt-0.5 text-[11.5px] text-muted-foreground">{p.description}</div>
              )}
            </div>
            {isCurrent && (
              <span className="rounded border border-primary/30 bg-primary/10 px-1.5 py-0.5 font-mono text-[10.5px] text-primary">
                {t('switchTemplate.currentTag')}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

function ChosenTemplateBanner({
  profile,
  onCancel,
}: {
  profile: ProfileFile | null;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  if (!profile) return null;
  return (
    <div className="flex items-center gap-2.5 rounded-md border border-border bg-muted/40 px-3.5 py-2.5">
      <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: profile.color }} />
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-foreground">{profile.displayName}</div>
        {profile.description && (
          <div className="mt-0.5 text-[11.5px] text-muted-foreground">{profile.description}</div>
        )}
      </div>
      <Button variant="ghost" size="sm" onClick={onCancel}>
        {t('switchTemplate.changeTemplate')}
      </Button>
    </div>
  );
}

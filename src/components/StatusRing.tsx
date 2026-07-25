/**
 * Status ring — visual indicator for project apply state.
 *
 * Four states (see ExecPlan D3):
 *   - sync   = applied <24h && git clean  → success solid
 *   - dirty  = git dirty / has conflicts  → warning solid
 *   - recent = applied >=24h              → information solid
 *   - never  = no lastApplied             → subtle dashed
 *
 * Same color language as the tray menubar dot so the UI stays
 * consistent.
 */

import type { Project, ProjectStatus } from '@/lib/projectTypes';
import i18n from '@/i18n';

export type RingState = 'sync' | 'dirty' | 'recent' | 'never';

const TWENTY_FOUR_HOURS_MS = 24 * 60 * 60 * 1000;

export function ringStateFor(project: Project, status: ProjectStatus | null): RingState {
  if (!project.lastApplied) return 'never';
  if (status?.gitDirty === true) return 'dirty';
  const appliedAt = new Date(project.lastApplied.timestamp).getTime();
  if (Number.isNaN(appliedAt)) return 'recent';
  return Date.now() - appliedAt < TWENTY_FOUR_HOURS_MS ? 'sync' : 'recent';
}

/** Tailwind class for the row border. Pair with `border-2`. */
export function ringBorderClass(state: RingState): string {
  switch (state) {
    case 'sync':
      return 'border-success';
    case 'dirty':
      return 'border-warning';
    case 'recent':
      return 'border-info';
    case 'never':
      return 'border-dashed border-border';
  }
}

/** Tailwind text-color class matching the ring. */
export function ringTextClass(state: RingState): string {
  switch (state) {
    case 'sync':
      return 'text-success';
    case 'dirty':
      return 'text-warning';
    case 'recent':
      return 'text-info';
    case 'never':
      return 'text-muted-foreground';
  }
}

/** Short status text shown under the project name. */
export function ringStatusText(state: RingState, project: Project): string {
  if (state === 'never') return i18n.t('status.neverApplied');
  const layers = project.lastApplied?.layers.join('+') ?? '';
  const ago = relativeTime(project.lastApplied!.timestamp);
  if (state === 'dirty') return `⚠ git dirty · ${layers} · ${ago}`;
  if (state === 'sync') return `✓ ${layers} · ${ago}`;
  return `${layers} · ${ago}`;
}

function relativeTime(iso: string): string {
  const ts = new Date(iso).getTime();
  if (Number.isNaN(ts)) return iso;
  const diff = Math.max(0, Date.now() - ts);
  const m = 60_000,
    h = 60 * m,
    d = 24 * h;
  if (diff < m) return i18n.t('status.justNow');
  if (diff < h) return i18n.t('status.minutesAgo', { n: Math.floor(diff / m) });
  if (diff < d) return i18n.t('status.hoursAgo', { n: Math.floor(diff / h) });
  if (diff < 7 * d) return i18n.t('status.daysAgo', { n: Math.floor(diff / d) });
  return iso.slice(0, 10);
}

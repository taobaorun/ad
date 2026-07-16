/**
 * Inline conflict resolver (M3, resolves TD-005).
 *
 * Replaces the old read-only conflict list in ApplyDialog with three
 * radio choices per conflict: keep existing, use incoming, custom.
 * Custom opens a JSON text area whose value is validated before being
 * promoted to a Resolution.
 *
 * Stateless — caller owns the `resolutions` map and merges updates from
 * `onChange`.
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Conflict, Resolution } from '@/lib/projectTypes';

type Choice = 'keep' | 'use' | 'custom';

function choiceOf(r: Resolution | undefined): Choice | null {
  if (!r) return null;
  if (r.kind === 'keepExisting') return 'keep';
  if (r.kind === 'useIncoming') return 'use';
  return 'custom';
}

export function InlineConflictResolver({
  conflicts,
  resolutions,
  onChange,
}: {
  conflicts: Conflict[];
  resolutions: Record<string, Resolution>;
  onChange: (next: Record<string, Resolution>) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="rounded-md border border-primary/40 bg-primary/5 p-3">
      <div className="mb-2 flex items-center gap-2 text-sm font-semibold text-primary">
        {t(conflicts.length === 1 ? 'conflict.headerOne' : 'conflict.headerMany', { count: conflicts.length })}
      </div>
      <ul className="flex flex-col gap-3">
        {conflicts.map((c) => (
          <ConflictRow
            key={c.keyPath}
            conflict={c}
            choice={choiceOf(resolutions[c.keyPath])}
            resolution={resolutions[c.keyPath]}
            onPick={(r) => onChange({ ...resolutions, [c.keyPath]: r })}
          />
        ))}
      </ul>
    </div>
  );
}

function ConflictRow({
  conflict,
  choice,
  resolution,
  onPick,
}: {
  conflict: Conflict;
  choice: Choice | null;
  resolution: Resolution | undefined;
  onPick: (r: Resolution) => void;
}) {
  const { t } = useTranslation();
  const initial =
    resolution?.kind === 'custom' ? JSON.stringify(resolution.value, null, 2) : '';
  const [customText, setCustomText] = useState<string>(initial);
  const [customError, setCustomError] = useState<string | null>(null);

  function pickKeep() { onPick({ kind: 'keepExisting' }); }
  function pickUse()  { onPick({ kind: 'useIncoming' }); }
  function pickCustomFromText(text: string) {
    setCustomText(text);
    try {
      const parsed = JSON.parse(text);
      setCustomError(null);
      onPick({ kind: 'custom', value: parsed });
    } catch (e) {
      setCustomError(e instanceof Error ? e.message : String(e));
      // Don't promote a Resolution while invalid; caller's `resolutions`
      // map keeps whatever was last valid.
    }
  }

  return (
    <li className="rounded border border-border bg-background p-2.5">
      <div className="mb-1.5 font-mono text-[12px] font-medium text-foreground">
        {conflict.keyPath}
      </div>
      <div className="flex flex-col gap-1">
        <label className={radioRowClass(choice === 'keep')}>
          <input
            type="radio"
            name={`conflict-${conflict.keyPath}`}
            checked={choice === 'keep'}
            onChange={pickKeep}
          />
          <span className="w-20 text-xs">{t('conflict.keepOld')}</span>
          <code className="flex-1 truncate font-mono text-[11px] text-muted-foreground">
            {valuePreview(conflict.existing)}
          </code>
        </label>
        <label className={radioRowClass(choice === 'use')}>
          <input
            type="radio"
            name={`conflict-${conflict.keyPath}`}
            checked={choice === 'use'}
            onChange={pickUse}
          />
          <span className="w-20 text-xs">{t('conflict.useNew')}</span>
          <code className="flex-1 truncate font-mono text-[11px] text-muted-foreground">
            {valuePreview(conflict.incoming)}
          </code>
        </label>
        <label className={radioRowClass(choice === 'custom')}>
          <input
            type="radio"
            name={`conflict-${conflict.keyPath}`}
            checked={choice === 'custom'}
            onChange={() => pickCustomFromText(customText || '""')}
          />
          <span className="w-20 text-xs">{t('conflict.custom')}</span>
          <span className="flex-1 text-[11px] text-muted-foreground">
            {t('conflict.editJson')}
          </span>
        </label>
        {choice === 'custom' && (
          <div className="ml-6 mt-1">
            <textarea
              value={customText}
              onChange={(e) => pickCustomFromText(e.target.value)}
              placeholder={t('conflict.jsonPlaceholder')}
              rows={3}
              className="w-full rounded border border-input bg-background p-2 font-mono text-[11px] outline-none focus:border-primary focus:ring-2 focus:ring-ring/30"
            />
            {customError && (
              <div className="mt-1 text-[11px] text-destructive">
                {t('conflict.invalidJson', { message: customError })}
              </div>
            )}
          </div>
        )}
      </div>
    </li>
  );
}

function radioRowClass(active: boolean): string {
  return (
    'flex cursor-pointer items-center gap-2 rounded px-2 py-1 transition-colors ' +
    (active ? 'bg-muted' : 'hover:bg-muted/40')
  );
}

function valuePreview(v: unknown): string {
  if (v === null) return 'null';
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  try {
    const s = JSON.stringify(v);
    return s.length > 80 ? s.slice(0, 77) + '…' : s;
  } catch {
    return String(v);
  }
}

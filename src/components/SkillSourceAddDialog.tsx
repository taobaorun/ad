import { useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { formatAgentErrorMessage } from '@/lib/agentErrors';
import type { SkillSourcePreviewProgress, SkillSourceRequest } from '@/lib/skillCatalogTypes';
import { OperationProgress } from './OperationProgress';
import { Button } from './ui/button';
import { Dialog } from './ui/dialog';

interface SkillSourceAddDialogProps {
  open: boolean;
  busy: boolean;
  progress: SkillSourcePreviewProgress | null;
  startedAt: number | null;
  resourceMode?: boolean;
  onOpenChange: (open: boolean) => void;
  onPreview: (request: SkillSourceRequest) => Promise<void>;
}

export function SkillSourceAddDialog({
  open,
  busy,
  progress,
  startedAt,
  resourceMode = false,
  onOpenChange,
  onPreview,
}: SkillSourceAddDialogProps) {
  const { t } = useTranslation();
  const [displayName, setDisplayName] = useState('');
  const [sourceType, setSourceType] = useState<'git' | 'local'>('git');
  const [location, setLocation] = useState('');
  const [branch, setBranch] = useState('');
  const [subdirectory, setSubdirectory] = useState('');
  const [error, setError] = useState<string | null>(null);
  const submittingRef = useRef(false);

  function updateLocation(value: string) {
    setLocation(value);
    if (!displayName) {
      const inferred = value
        .replace(/\.git\s*$/, '')
        .split(/[/:]/)
        .filter(Boolean)
        .at(-1);
      if (inferred) setDisplayName(inferred);
    }
  }

  async function submit() {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setError(null);
    try {
      await onPreview({
        displayName,
        sourceType,
        location,
        branch: sourceType === 'git' && branch ? branch : undefined,
        subdirectory: subdirectory || undefined,
        autoUpdate: false,
      });
    } catch (reason) {
      setError(formatAgentErrorMessage(reason));
    } finally {
      submittingRef.current = false;
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      closeDisabled={busy}
      title={t(resourceMode ? 'resourceCenter.addDialog.title' : 'settings.skills.addDialog.title')}
      description={t(
        resourceMode
          ? 'resourceCenter.addDialog.description'
          : 'settings.skills.addDialog.description',
      )}
      footer={
        busy ? undefined : (
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => onOpenChange(false)}
            >
              {t('common.cancel')}
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={busy || !displayName || !location}
              onClick={() => void submit()}
            >
              {busy
                ? t('settings.skills.addDialog.inspecting')
                : error
                  ? t('settings.skills.addDialog.retry')
                  : t('settings.skills.addDialog.preview')}
            </Button>
          </div>
        )
      }
    >
      {busy && progress && startedAt !== null ? (
        <OperationProgress
          label={t(`settings.skills.addDialog.progress.${progress.phase}`)}
          startedAt={startedAt}
          hint={t('settings.skills.addDialog.progress.hint')}
        />
      ) : (
        <>
          <Field label={t('settings.skills.addDialog.name')}>
            <input
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              className={inputClass}
            />
          </Field>
          <Field label={t('settings.skills.addDialog.type')}>
            <div className="flex gap-2">
              {(['git', 'local'] as const).map((type) => (
                <Button
                  key={type}
                  type="button"
                  size="sm"
                  variant={sourceType === type ? 'secondary' : 'outline'}
                  onClick={() => setSourceType(type)}
                >
                  {t(`settings.skills.addDialog.type${type === 'git' ? 'Git' : 'Local'}`)}
                </Button>
              ))}
            </div>
          </Field>
          <Field
            label={
              sourceType === 'git'
                ? t('settings.skills.addDialog.gitUrl')
                : t('settings.skills.addDialog.path')
            }
          >
            <input
              value={location}
              onChange={(event) => updateLocation(event.target.value)}
              className={inputClass}
            />
          </Field>
          {sourceType === 'git' && (
            <Field label={t('settings.skills.addDialog.branch')}>
              <input
                value={branch}
                onChange={(event) => setBranch(event.target.value)}
                className={inputClass}
              />
            </Field>
          )}
          <Field label={t('settings.skills.addDialog.subdirectory')}>
            <input
              value={subdirectory}
              onChange={(event) => setSubdirectory(event.target.value)}
              className={inputClass}
            />
          </Field>
          {!resourceMode && (
            <p className="text-[11px] text-muted-foreground">
              {t('settings.skills.addDialog.backendIdHint')}
            </p>
          )}
          {error && (
            <div role="alert" className="mt-3 text-xs text-destructive">
              {error}
            </div>
          )}
        </>
      )}
    </Dialog>
  );
}

const inputClass =
  'w-full rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs outline-none focus:border-primary focus:ring-2 focus:ring-ring/30';

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="mb-3 block">
      <span className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </span>
      {children}
    </label>
  );
}

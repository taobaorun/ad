import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog } from './ui/dialog';
import { Button } from './ui/button';
import { useUiSettings } from '@/store/uiSettings';
import { Settings as SettingsIcon } from 'lucide-react';
import { setLanguage, type Lang } from '@/i18n';

/**
 * Advanced / preferences dialog (M5).
 *
 * Currently exposes a single toggle: whether to show the legacy
 * [Activate (legacy)] button in ProfileEditor. The legacy command itself
 * (`activate_profile`) stays registered and unchanged — this only controls
 * UI visibility.
 */
export function AdvancedSettingsButton() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        title={t('advanced.openTitle')}
        aria-label={t('advanced.openTitle')}
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
  const { t, i18n } = useTranslation();
  const show = useUiSettings((s) => s.showLegacyActivation);
  const setShow = useUiSettings((s) => s.setShowLegacyActivation);

  const currentLang = (i18n.language as Lang) === 'zh' ? 'zh' : 'en';

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('advanced.dialogTitle')} size="md">
      <div className="flex flex-col gap-4 text-sm">
        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            {t('advanced.language')}
          </h3>
          <p className="mb-2 text-xs text-muted-foreground">{t('advanced.languageDesc')}</p>
          <div className="flex gap-2">
            <LangButton on={currentLang === 'zh'} onClick={() => setLanguage('zh')}>
              {t('advanced.langZh')}
            </LangButton>
            <LangButton on={currentLang === 'en'} onClick={() => setLanguage('en')}>
              {t('advanced.langEn')}
            </LangButton>
          </div>
        </section>

        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            {t('advanced.legacy')}
          </h3>
          <label className="flex items-start gap-3 rounded border border-border p-3">
            <input
              type="checkbox"
              checked={show}
              onChange={(e) => setShow(e.target.checked)}
              className="mt-0.5"
            />
            <div>
              <div className="font-medium">{t('advanced.showLegacyTitle')}</div>
              <p className="mt-1 text-xs text-muted-foreground">{t('advanced.showLegacyDesc')}</p>
            </div>
          </label>
        </section>

        <div className="flex justify-end">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('advanced.close')}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

function LangButton({
  on, onClick, children,
}: { on: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        'inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-xs ' +
        (on
          ? 'border-foreground bg-foreground text-background'
          : 'border-border bg-background text-foreground hover:border-clay')
      }
    >
      {children}
    </button>
  );
}

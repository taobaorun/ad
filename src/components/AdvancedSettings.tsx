import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog } from './ui/dialog';
import { Button } from './ui/button';
import { useUiSettings } from '@/store/uiSettings';
import { Settings as SettingsIcon } from 'lucide-react';
import { setLanguage, type Lang } from '@/i18n';
import type { TerminalBackendId } from '@/lib/tauri';

const TERMINAL_BACKENDS: TerminalBackendId[] = ['ghostty', 'cmux', 'apple-terminal', 'custom'];

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
  const terminal = useUiSettings((s) => s.terminal);
  const setTerminal = useUiSettings((s) => s.setTerminal);

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

        <section data-section="terminal">
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            {t('terminal.section')}
          </h3>
          <div className="mb-3">
            <div className="mb-2 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
              {t('terminal.backendLabel')}
            </div>
            <div className="grid grid-cols-2 gap-2">
              {TERMINAL_BACKENDS.map((id) => {
                const selected = terminal.backend === id;
                return (
                  <button
                    key={id}
                    type="button"
                    onClick={() => setTerminal({ backend: id })}
                    className={
                      'flex flex-col gap-1 rounded-md border p-2.5 text-left transition-colors ' +
                      (selected
                        ? 'border-clay bg-clay/10 text-foreground'
                        : 'border-border bg-background hover:border-clay/60')
                    }
                  >
                    <span className="text-sm font-medium">{t(`terminal.backend.${id}`)}</span>
                    <span className="text-[11px] leading-tight text-muted-foreground">
                      {t(`terminal.backendHint.${id}`)}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="mb-3">
            <div className="mb-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
              {t('terminal.claudeBinLabel')}
            </div>
            <input
              type="text"
              value={terminal.claudeBinPath}
              onChange={(e) => setTerminal({ claudeBinPath: e.target.value })}
              placeholder={t('terminal.claudeBinPlaceholder')}
              className="w-full rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs text-foreground outline-none focus:border-clay"
            />
            <p className="mt-1 text-[11px] text-muted-foreground">{t('terminal.claudeBinHint')}</p>
          </div>

          {terminal.backend === 'custom' && (
            <div>
              <div className="mb-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
                {t('terminal.customTemplateLabel')}
              </div>
              <textarea
                value={terminal.customCommand}
                onChange={(e) => setTerminal({ customCommand: e.target.value })}
                rows={3}
                placeholder="open -na WezTerm.app --args start --cwd {{cwd}} -- {{cmd}}"
                className="w-full resize-y rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs text-foreground outline-none focus:border-clay"
              />
              <p className="mt-1 text-[11px] text-muted-foreground">{t('terminal.customTemplateHint')}</p>
            </div>
          )}
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

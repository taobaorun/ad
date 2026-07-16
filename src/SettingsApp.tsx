/**
 * Standalone settings window content. Loaded by `main.tsx` when the URL hash
 * is `#/settings`. Renders a sidebar + sectioned content area instead of a
 * modal — matches the cmux-style preferences UX.
 *
 * State is the existing `useUiSettings` zustand store, which persists to
 * `localStorage`. Cross-window sync is handled in `uiSettings.ts` via the
 * `storage` event so the main window picks up changes immediately.
 */

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { DEFAULT_GLOBAL_SHORTCUT, useUiSettings } from '@/store/uiSettings';
import { setLanguage, type Lang } from '@/i18n';
import type { TerminalBackendId } from '@/lib/tauri';
import { Keyboard, Languages, Terminal as TerminalIcon, History, Puzzle } from 'lucide-react';
import { SkillSourcesSection } from '@/components/SkillSources';
import { applyDocumentTheme } from '@/lib/theme';

const TERMINAL_BACKENDS: TerminalBackendId[] = ['ghostty', 'cmux', 'apple-terminal', 'custom'];

type SectionId = 'general' | 'terminal' | 'skills' | 'shortcuts' | 'legacy';

export function SettingsApp() {
  const { t } = useTranslation();
  const [section, setSection] = useState<SectionId>('general');

  const dark = useUiSettings((s) => s.darkMode);
  useEffect(() => {
    applyDocumentTheme(dark);
  }, [dark]);

  useEffect(() => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        document.getElementById('ad-splash')?.remove();
      });
    });
  }, []);

  const nav: { id: SectionId; label: string; icon: ReactNode }[] = [
    { id: 'general', label: t('settings.nav.general'), icon: <Languages className="h-4 w-4" /> },
    { id: 'terminal', label: t('settings.nav.terminal'), icon: <TerminalIcon className="h-4 w-4" /> },
    { id: 'skills', label: t('settings.skills.nav'), icon: <Puzzle className="h-4 w-4" /> },
    { id: 'shortcuts', label: t('settings.nav.shortcuts'), icon: <Keyboard className="h-4 w-4" /> },
    { id: 'legacy', label: t('settings.nav.legacy'), icon: <History className="h-4 w-4" /> },
  ];

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <aside className="flex w-52 shrink-0 flex-col border-r border-border bg-muted/30">
        <div className="px-4 py-4 text-sm font-semibold">{t('settings.title')}</div>
        <nav className="flex flex-col gap-0.5 px-2">
          {nav.map((item) => {
            const active = item.id === section;
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => setSection(item.id)}
                className={
                  'flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[13px] transition-colors ' +
                  (active
                    ? 'bg-foreground/10 text-foreground'
                    : 'text-muted-foreground hover:bg-foreground/5 hover:text-foreground')
                }
              >
                {item.icon}
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>
      </aside>

      <main className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-2xl px-8 py-8">
          {section === 'general' && <GeneralSection />}
          {section === 'terminal' && <TerminalSection />}
          {section === 'skills' && <SkillSourcesSection />}
          {section === 'shortcuts' && <ShortcutsSection />}
          {section === 'legacy' && <LegacySection />}
        </div>
      </main>
    </div>
  );
}

function SectionHeader({ title, desc }: { title: string; desc?: string }) {
  return (
    <header className="mb-5">
      <h1 className="text-xl font-semibold leading-tight">{title}</h1>
      {desc && <p className="mt-1 text-sm text-muted-foreground">{desc}</p>}
    </header>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="mb-5">
      <div className="mb-1.5 text-[12px] font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </div>
      {children}
      {hint && <p className="mt-1.5 text-[11.5px] text-muted-foreground">{hint}</p>}
    </div>
  );
}

function GeneralSection() {
  const { t, i18n } = useTranslation();
  const currentLang = (i18n.language as Lang) === 'zh' ? 'zh' : 'en';
  return (
    <>
      <SectionHeader
        title={t('settings.general.title')}
        desc={t('settings.general.desc')}
      />
      <Field label={t('advanced.language')} hint={t('advanced.languageDesc')}>
        <div className="flex gap-2">
          <LangButton on={currentLang === 'zh'} onClick={() => setLanguage('zh')}>
            {t('advanced.langZh')}
          </LangButton>
          <LangButton on={currentLang === 'en'} onClick={() => setLanguage('en')}>
            {t('advanced.langEn')}
          </LangButton>
        </div>
      </Field>
    </>
  );
}

function TerminalSection() {
  const { t } = useTranslation();
  const terminal = useUiSettings((s) => s.terminal);
  const setTerminal = useUiSettings((s) => s.setTerminal);

  return (
    <>
      <SectionHeader
        title={t('settings.terminal.title')}
        desc={t('settings.terminal.desc')}
      />

      <Field label={t('terminal.backendLabel')}>
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
      </Field>

      {terminal.backend === 'custom' && (
        <Field label={t('terminal.customTemplateLabel')} hint={t('terminal.customTemplateHint')}>
          <textarea
            value={terminal.customCommand}
            onChange={(e) => setTerminal({ customCommand: e.target.value })}
            rows={3}
            placeholder="open -na WezTerm.app --args start --cwd {{cwd}} -- {{cmd}}"
            className="w-full resize-y rounded-md border border-border bg-background px-2.5 py-1.5 font-mono text-xs text-foreground outline-none focus:border-clay"
          />
        </Field>
      )}
    </>
  );
}

/**
 * Convert a captured `KeyboardEvent` into Tauri's shortcut string format,
 * e.g. ⌥⌘A → `"Alt+Cmd+KeyA"`. Modifier-only presses return `null` so we
 * keep recording until a real key arrives.
 */
function formatShortcut(e: KeyboardEvent): string | null {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (e.metaKey) mods.push('Cmd');

  const code = e.code;
  // Codes like "ControlLeft", "AltRight", "MetaLeft", "ShiftLeft" are modifier
  // keypresses — wait for the real key.
  if (
    code.startsWith('Control') ||
    code.startsWith('Alt') ||
    code.startsWith('Shift') ||
    code.startsWith('Meta') ||
    code === 'OSLeft' ||
    code === 'OSRight'
  ) {
    return null;
  }
  if (mods.length === 0) return null; // Plain keys would clash with typing.
  return [...mods, code].join('+');
}

/**
 * Render a Tauri shortcut string as a human-readable label.
 * `"Alt+Cmd+KeyA"` → `⌥⌘A`.
 */
function prettyShortcut(binding: string): string {
  return binding
    .split('+')
    .map((part) => {
      switch (part) {
        case 'Ctrl':
          return '⌃';
        case 'Alt':
          return '⌥';
        case 'Shift':
          return '⇧';
        case 'Cmd':
        case 'Super':
          return '⌘';
        default:
          if (part.startsWith('Key')) return part.slice(3);
          if (part.startsWith('Digit')) return part.slice(5);
          if (part.startsWith('Arrow')) return part.slice(5);
          return part;
      }
    })
    .join('');
}

function ShortcutsSection() {
  const { t } = useTranslation();
  const gs = useUiSettings((s) => s.globalShortcut);
  const setGlobalShortcut = useUiSettings((s) => s.setGlobalShortcut);

  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const recordingRef = useRef(recording);
  recordingRef.current = recording;

  useEffect(() => {
    if (!recording) return;
    const onKeyDown = (e: KeyboardEvent) => {
      // Capture everything, even keys the browser would normally consume.
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') {
        setRecording(false);
        return;
      }
      const formatted = formatShortcut(e);
      if (!formatted) return;
      setRecording(false);
      void setGlobalShortcut({ binding: formatted, enabled: true }).then((err) =>
        setError(err),
      );
    };
    window.addEventListener('keydown', onKeyDown, { capture: true });
    return () => window.removeEventListener('keydown', onKeyDown, { capture: true });
  }, [recording, setGlobalShortcut]);

  async function onToggleEnabled(next: boolean) {
    setError(await setGlobalShortcut({ enabled: next }));
  }

  async function onReset() {
    setError(
      await setGlobalShortcut({ enabled: true, binding: DEFAULT_GLOBAL_SHORTCUT }),
    );
  }

  return (
    <>
      <SectionHeader
        title={t('settings.shortcuts.title')}
        desc={t('settings.shortcuts.desc')}
      />

      <label className="mb-5 flex cursor-pointer items-start gap-3 rounded-md border border-border p-3">
        <input
          type="checkbox"
          checked={gs.enabled}
          onChange={(e) => void onToggleEnabled(e.target.checked)}
          className="mt-0.5"
        />
        <div>
          <div className="text-sm font-medium">{t('settings.shortcuts.enableTitle')}</div>
          <p className="mt-1 text-xs text-muted-foreground">
            {t('settings.shortcuts.enableDesc')}
          </p>
        </div>
      </label>

      <Field
        label={t('settings.shortcuts.bindingLabel')}
        hint={
          recording
            ? t('settings.shortcuts.recordingHint')
            : t('settings.shortcuts.bindingHint')
        }
      >
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => setRecording((r) => !r)}
            disabled={!gs.enabled}
            className={
              'inline-flex min-w-[140px] items-center justify-center gap-2 rounded-md border px-3 py-1.5 font-mono text-sm transition-colors ' +
              (recording
                ? 'border-clay bg-clay/10 text-foreground'
                : 'border-border bg-background hover:border-clay disabled:cursor-not-allowed disabled:opacity-50')
            }
          >
            {recording ? t('settings.shortcuts.recording') : prettyShortcut(gs.binding)}
          </button>
          <button
            type="button"
            onClick={() => void onReset()}
            className="rounded-md border border-border bg-background px-3 py-1.5 text-xs text-muted-foreground hover:border-clay hover:text-foreground"
          >
            {t('settings.shortcuts.reset')}
          </button>
        </div>
      </Field>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error}
        </div>
      )}
    </>
  );
}

function LegacySection() {
  const { t } = useTranslation();
  const show = useUiSettings((s) => s.showLegacyActivation);
  const setShow = useUiSettings((s) => s.setShowLegacyActivation);
  return (
    <>
      <SectionHeader
        title={t('settings.legacy.title')}
        desc={t('settings.legacy.desc')}
      />
      <label className="flex cursor-pointer items-start gap-3 rounded-md border border-border p-3">
        <input
          type="checkbox"
          checked={show}
          onChange={(e) => setShow(e.target.checked)}
          className="mt-0.5"
        />
        <div>
          <div className="text-sm font-medium">{t('advanced.showLegacyTitle')}</div>
          <p className="mt-1 text-xs text-muted-foreground">{t('advanced.showLegacyDesc')}</p>
        </div>
      </label>
    </>
  );
}

function LangButton({
  on,
  onClick,
  children,
}: {
  on: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
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

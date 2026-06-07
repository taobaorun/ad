import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

import en from './locales/en.json';
import zh from './locales/zh.json';

export type Lang = 'zh' | 'en';

const STORAGE_KEY = 'ad.lang.v1';
const DEFAULT_LANG: Lang = 'zh';

function readPersisted(): Lang {
  try {
    const v = window.localStorage.getItem(STORAGE_KEY);
    if (v === 'zh' || v === 'en') return v;
  } catch {
    // localStorage unavailable (privacy / quota) — fall through to default.
  }
  return DEFAULT_LANG;
}

void i18n.use(initReactI18next).init({
  resources: {
    zh: { translation: zh },
    en: { translation: en },
  },
  lng: readPersisted(),
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
  returnNull: false,
  // Resources are bundled inline — no async loading — so Suspense is
  // unnecessary and would just blank the screen until a boundary handles it.
  react: { useSuspense: false },
});

export function setLanguage(lng: Lang): void {
  void i18n.changeLanguage(lng);
  try {
    window.localStorage.setItem(STORAGE_KEY, lng);
  } catch {
    // ignore
  }
}

// Cross-window sync: when Settings changes the language, the main window
// picks it up via the storage event (fires only in OTHER same-origin windows).
if (typeof window !== 'undefined') {
  window.addEventListener('storage', (e) => {
    if (e.key !== STORAGE_KEY || !e.newValue) return;
    if (e.newValue === 'zh' || e.newValue === 'en') {
      void i18n.changeLanguage(e.newValue);
    }
  });
}

export default i18n;

export const THEME_COLORS = {
  mocha: {
    background: '#1e1e2e',
    foreground: '#cdd6f4',
  },
  latte: {
    background: '#eff1f5',
    foreground: '#4c4f69',
  },
} as const;

/** Keep the root class and pre-paint inline colors aligned after a mode change. */
export function applyDocumentTheme(dark: boolean, root = document.documentElement): void {
  const colors = dark ? THEME_COLORS.mocha : THEME_COLORS.latte;
  root.classList.toggle('dark', dark);
  root.style.backgroundColor = colors.background;
  root.style.color = colors.foreground;
}

import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';

import { describe, expect, it } from 'vitest';

const themeCss = readFileSync('src/styles/globals.css', 'utf8');
const tailwindConfig = readFileSync('tailwind.config.ts', 'utf8');
const indexHtml = readFileSync('index.html', 'utf8');
const themeGuide = readFileSync('docs/design-docs/theme-system.md', 'utf8');
const startupSurface = readFileSync('src/lib/startupSurface.ts', 'utf8');

describe('theme contract', () => {
  it('defines the official Latte and Mocha foundation as RGB channels', () => {
    expect(themeCss).toContain('--ctp-base: 239 241 245;');
    expect(themeCss).toContain('--ctp-sapphire: 32 159 181;');
    expect(themeCss).toContain('--ctp-text: 76 79 105;');

    expect(themeCss).toContain('.dark {');
    expect(themeCss).toContain('--ctp-base: 30 30 46;');
    expect(themeCss).toContain('--ctp-sapphire: 116 199 236;');
    expect(themeCss).toContain('--ctp-text: 205 214 244;');
  });

  it('maps product roles to palette tokens instead of flavor-specific values', () => {
    expect(themeCss).toContain('--color-bg-canvas: var(--ctp-base);');
    expect(themeCss).toContain('--color-bg-pane: var(--ctp-mantle);');
    expect(themeCss).toContain('--color-bg-chrome: var(--ctp-crust);');
    expect(themeCss).toContain('--color-action-primary: var(--ctp-sapphire);');
    expect(themeCss).toContain('--color-link: var(--ctp-blue);');
    expect(themeCss).toContain('--color-info: var(--ctp-sky);');
    expect(themeCss).toContain('--color-success: var(--ctp-green);');
    expect(themeCss).toContain('--color-warning: var(--ctp-yellow);');
    expect(themeCss).toContain('--color-danger: var(--ctp-red);');
    expect(themeCss).toContain('--color-on-danger: var(--ctp-base);');
    expect(themeCss).toContain('--destructive-foreground: var(--color-on-danger);');
  });

  it('keeps normal action labels readable in Latte', () => {
    expect(contrastRatio('#11111b', '#209fb5')).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio('#eff1f5', '#d20f39')).toBeGreaterThanOrEqual(4.5);
  });

  it('gives native controls a single semantic focus indicator', () => {
    expect(themeCss).toContain(':root :is(button, a[href], input, textarea, select):focus-visible');
    expect(themeCss).toContain(':root [data-input-shell]:focus-within');
    expect(themeCss).toContain(':root [data-input-shell] :is(input, textarea):focus-visible');
    expect(themeCss).toContain('--ring: var(--color-link);');
    expect(themeCss).toContain('box-shadow: 0 0 0 2px rgb(var(--ring));');
    expect(themeCss).toContain('outline: 2px solid transparent;');
    expect(themeCss).not.toContain('outline: 1px solid rgb(var(--color-text-primary));');
    expect(contrastRatio('#1e66f5', '#eff1f5')).toBeGreaterThanOrEqual(3);
    expect(contrastRatio('#1e66f5', '#ccd0da')).toBeGreaterThanOrEqual(3);
    expect(contrastRatio('#89b4fa', '#1e1e2e')).toBeGreaterThanOrEqual(3);
    expect(contrastRatio('#89b4fa', '#313244')).toBeGreaterThanOrEqual(3);
  });

  it('derives compatibility aliases and Tailwind opacity from semantic roles', () => {
    expect(themeCss).toContain('--ds-bg-sidebar: rgb(var(--color-bg-pane));');
    expect(themeCss).toContain('--ds-accent: rgb(var(--color-action-primary));');
    expect(tailwindConfig).toContain('rgb(var(--background) / <alpha-value>)');
    expect(tailwindConfig).toContain('rgb(var(--color-success) / <alpha-value>)');
  });

  it('does not retain the previous Zinc, Indigo, or Anthropic foundations', () => {
    expect(themeCss).not.toMatch(/Zinc\/Indigo|#0a0a0b|#5b5bd6|#8c8cf5/i);
    expect(tailwindConfig).not.toMatch(/Anthropic palette|#D97757|#B04A3F|#788C5D/i);
    expect(themeCss).not.toContain('--ds-clay');
    expect(tailwindConfig).not.toMatch(/\b(?:clay|rust|olive|oat|slate2):/);
  });

  it('uses matching Catppuccin roots before the React bundle loads', () => {
    expect(indexHtml).toContain("var bg = dark ? '#1e1e2e' : '#eff1f5';");
    expect(indexHtml).toContain("var fg = dark ? '#cdd6f4' : '#4c4f69';");
    expect(indexHtml).not.toMatch(/#0a0a0b|#ffffff|#09090b/i);
  });

  it('renders the branded startup surface before React loads', () => {
    const publicLogo = readFileSync('public/ad-logo.png');
    const sourceLogo = readFileSync('src-tauri/icons/128x128.png');
    expect(indexHtml).toContain('src="/ad-logo.png"');
    expect(publicLogo.subarray(0, 8).toString('hex')).toBe('89504e470d0a1a0a');
    expect(createHash('sha256').update(publicLogo).digest('hex')).toBe(
      createHash('sha256').update(sourceLogo).digest('hex'),
    );
    expect(indexHtml).toContain('alt=""');
    expect(indexHtml).toContain('aria-hidden="true"');
    expect(indexHtml).toContain('id="ad-splash-quote"');
    expect(indexHtml).toContain('data-i18n-key="startup.loadingQuote"');
    expect(indexHtml).toContain('data-i18n-key="startup.loadingQuote">AD</p>');
    expect(indexHtml).not.toContain('>AD is loading');
    expect(indexHtml).not.toContain('Initializing AD');
  });

  it('keeps the startup surface accessible while it covers the app', () => {
    expect(indexHtml.match(/role="status"/g)).toHaveLength(1);
    expect(indexHtml).toContain('id="ad-splash-status"');
    expect(indexHtml).toContain('data-i18n-key="startup.loadingStatus"');
    expect(indexHtml).toContain('aria-live="polite"');
    expect(indexHtml).toContain('aria-atomic="true"');
    expect(indexHtml).toContain('<div id="root" inert aria-hidden="true"></div>');
  });

  it('provides a WKWebView-safe Canvas spotlight, exit, and reduced-motion fallbacks', () => {
    expect(indexHtml).toContain('font-family: ui-serif, Georgia,');
    expect(indexHtml).toContain('font-size: 23px');
    expect(indexHtml).toContain('letter-spacing: 0.02em');
    expect(indexHtml).toContain('id="ad-splash-quote-canvas"');
    expect(indexHtml).toContain('aria-hidden="true"');
    expect(indexHtml).toContain('canvas.transferControlToOffscreen()');
    expect(indexHtml).toContain('new Worker(workerUrl)');
    expect(indexHtml).toMatch(/worker\.postMessage\(\s*\{\s*type:\s*'start'/);
    expect(indexHtml).toContain('worker.terminate()');
    expect(indexHtml).toContain('URL.revokeObjectURL(workerUrl)');
    expect(indexHtml).toContain("canvas.getContext('2d')");
    expect(indexHtml).toContain('context.createLinearGradient');
    expect(indexHtml).toContain('self.setInterval(drawFrame, frameIntervalMs)');
    expect(indexHtml).toContain('window.setInterval(drawFrame, frameIntervalMs)');
    expect(indexHtml).toContain("window.addEventListener('focus', drawFrame)");
    expect(indexHtml).toContain(
      "document.addEventListener('visibilitychange', handleVisibilityChange)",
    );
    expect(indexHtml).not.toContain('window.requestAnimationFrame(drawFrame)');
    expect(indexHtml).toContain("matchMedia('(prefers-reduced-motion: reduce)')");
    expect(indexHtml).toContain('if (!canvas.isConnected)');
    expect(indexHtml).toContain('window.clearInterval(frameTimer)');
    expect(indexHtml).toContain('const sweepDurationMs = 300');
    expect(indexHtml).not.toContain('.ad-splash-quote-base');
    expect(indexHtml).not.toContain('.ad-splash-quote-spotlight');
    expect(indexHtml).not.toContain('.ad-splash-quote-character');
    expect(indexHtml).not.toContain('animationDelay');
    expect(indexHtml).not.toContain('clip-path:');
    expect(indexHtml).not.toContain('-webkit-background-clip: text');
    expect(indexHtml).not.toContain('-webkit-text-fill-color: transparent');
    expect(startupSurface).not.toContain('export function startStartupSpotlight');
    expect(indexHtml).toContain('function animateStartupQuote');
    expect(indexHtml).not.toContain('ad-splash-quote-text');
    expect(indexHtml).not.toContain('ad-splash-ambient');
    expect(indexHtml).not.toContain('@keyframes ad-ambient-sweep');
    expect(indexHtml).not.toContain('function adAnimateSpotlight');
    expect(indexHtml).toContain('@media (prefers-reduced-motion: reduce)');
    expect(indexHtml).toContain('#ad-splash.ad-splash-exit');
    expect(indexHtml).toContain('transition: opacity 260ms ease');
    expect(indexHtml).toContain('#root.ad-app-enter');
  });

  it('keeps the non-highlighted startup quote readable in Latte and Mocha', () => {
    expect(contrastRatio('#5c5f77', '#eff1f5')).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio('#a6adc8', '#1e1e2e')).toBeGreaterThanOrEqual(4.5);
  });

  it('documents every implemented product semantic token', () => {
    const semanticTokens = [...themeCss.matchAll(/^\s*(--color-[\w-]+):/gm)].map(
      ([, token]) => token,
    );

    expect(semanticTokens.length).toBeGreaterThan(0);
    for (const token of semanticTokens) expect(themeGuide).toContain(`\`${token}\``);
  });
});

function contrastRatio(foreground: string, background: string): number {
  const foregroundLuminance = relativeLuminance(foreground);
  const backgroundLuminance = relativeLuminance(background);
  const lighter = Math.max(foregroundLuminance, backgroundLuminance);
  const darker = Math.min(foregroundLuminance, backgroundLuminance);
  return (lighter + 0.05) / (darker + 0.05);
}

function relativeLuminance(hex: string): number {
  const red = linearChannel(hex, 1);
  const green = linearChannel(hex, 3);
  const blue = linearChannel(hex, 5);
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}

function linearChannel(hex: string, offset: number): number {
  const channel = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
  return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
}

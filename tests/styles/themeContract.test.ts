import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

const themeCss = readFileSync('src/styles/globals.css', 'utf8');
const tailwindConfig = readFileSync('tailwind.config.ts', 'utf8');
const indexHtml = readFileSync('index.html', 'utf8');

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
  });

  it('derives compatibility aliases and Tailwind opacity from semantic roles', () => {
    expect(themeCss).toContain('--ds-bg-sidebar: rgb(var(--color-bg-pane));');
    expect(themeCss).toContain('--ds-accent: rgb(var(--color-action-primary));');
    expect(tailwindConfig).toContain("rgb(var(--background) / <alpha-value>)");
    expect(tailwindConfig).toContain("rgb(var(--color-success) / <alpha-value>)");
  });

  it('does not retain the previous Zinc, Indigo, or Anthropic foundations', () => {
    expect(themeCss).not.toMatch(/Zinc\/Indigo|#0a0a0b|#5b5bd6|#8c8cf5/i);
    expect(tailwindConfig).not.toMatch(/Anthropic palette|#D97757|#B04A3F|#788C5D/i);
  });

  it('uses matching Catppuccin roots before the React bundle loads', () => {
    expect(indexHtml).toContain("var bg = dark ? '#1e1e2e' : '#eff1f5';");
    expect(indexHtml).toContain("var fg = dark ? '#cdd6f4' : '#4c4f69';");
    expect(indexHtml).not.toMatch(/#0a0a0b|#ffffff|#09090b/i);
  });
});

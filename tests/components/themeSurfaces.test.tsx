import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

const rawTailwindColor =
  /\b(?:bg|text|border|ring|outline|shadow)-(?:black|white|gray|zinc|slate|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|clay|rust|olive|oat|slate2)(?:[-/][\w.]+)?\b/g;
const rawCssColor = /#[0-9a-f]{3,8}\b|rgba?\(\s*\d|hsla?\(/gi;
const productSourceFiles = ['src/App.tsx', 'src/SettingsApp.tsx', ...tsxFiles('src/components')];

describe('themed product surfaces', () => {
  it('uses semantic roles instead of raw product-chrome colors', () => {
    const violations = productSourceFiles.flatMap((path) => {
      const source = readFileSync(path, 'utf8');
      return [...source.matchAll(rawTailwindColor), ...source.matchAll(rawCssColor)].map(
        (match) => `${path}:${lineNumber(source, match.index)} ${match[0]}`,
      );
    });

    expect(violations).toEqual([]);
  });

  it('defines every consumed design-system compatibility variable', () => {
    const themeCss = readFileSync('src/styles/globals.css', 'utf8');
    const consumed = productSourceFiles.flatMap((path) => [
      ...readFileSync(path, 'utf8').matchAll(/var\((--ds-[\w-]+)/g),
    ]).map(([, variable]) => variable);
    const defined = new Set(
      [...themeCss.matchAll(/^\s*(--ds-[\w-]+):/gm)].map(([, variable]) => variable),
    );

    expect([...new Set(consumed)].filter((variable) => !defined.has(variable))).toEqual([]);
  });

  it('keeps small success copy readable while the icon carries status color', () => {
    const source = readFileSync('src/components/AgentSettingsEditor.tsx', 'utf8');

    expect(source).toContain(
      'className="flex items-center gap-1 whitespace-nowrap text-xs text-foreground"',
    );
    expect(source).toContain('<CheckCircle2 className="h-3.5 w-3.5 text-success" />');
  });
});

function tsxFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return tsxFiles(path);
    return entry.isFile() && path.endsWith('.tsx') ? [path] : [];
  });
}

function lineNumber(source: string, index: number | undefined): number {
  return source.slice(0, index ?? 0).split('\n').length;
}

import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

const rawTailwindColor =
  /\b(?:bg|text|border|ring|outline|shadow)-(?:black|white|gray|zinc|slate|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|clay|rust|olive|oat|slate2)(?:[-/][\w.]+)?\b/g;
const rawCssColor = /#[0-9a-f]{3,8}\b|rgba?\(\s*\d|hsla?\(/gi;

describe('themed product surfaces', () => {
  it('uses semantic roles instead of raw product-chrome colors', () => {
    const sourceFiles = ['src/App.tsx', 'src/SettingsApp.tsx', ...tsxFiles('src/components')];
    const violations = sourceFiles.flatMap((path) => {
      const source = readFileSync(path, 'utf8');
      return [...source.matchAll(rawTailwindColor), ...source.matchAll(rawCssColor)].map(
        (match) => `${path}:${lineNumber(source, match.index)} ${match[0]}`,
      );
    });

    expect(violations).toEqual([]);
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

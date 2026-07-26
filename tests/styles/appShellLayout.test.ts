import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

const appSource = readFileSync('src/App.tsx', 'utf8');

describe('main app shell layout', () => {
  it('contains overflow at the viewport and lets both split panes shrink', () => {
    expect(appSource).toContain(
      '<div className="flex h-full w-full min-w-0 flex-col overflow-hidden">',
    );
    expect(appSource).toContain('<div className="flex min-w-0 flex-1 overflow-hidden">');
    expect(appSource).toContain('<main className="flex min-w-0 flex-1 overflow-hidden">');
  });
});

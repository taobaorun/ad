#!/usr/bin/env node
// Fail CI if frontend bundle exceeds the budget. Tunable via BUNDLE_BUDGET_MB env.
import { readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const DIST = 'dist';
const BUDGET_MB = Number(process.env.BUNDLE_BUDGET_MB ?? 8);

function sizeBytes(dir) {
  let total = 0;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name);
    if (entry.isDirectory()) total += sizeBytes(p);
    else if (entry.name.endsWith('.js') || entry.name.endsWith('.css')) total += statSync(p).size;
  }
  return total;
}

try {
  const bytes = sizeBytes(DIST);
  const mb = bytes / (1024 * 1024);
  console.log(`Bundle (js+css uncompressed): ${mb.toFixed(2)} MB (budget ${BUDGET_MB} MB)`);
  if (mb > BUDGET_MB) {
    console.error(`✗ Over budget by ${(mb - BUDGET_MB).toFixed(2)} MB`);
    process.exit(1);
  }
  console.log('✓ Within budget');
} catch (e) {
  console.error('check-bundle-size failed:', e.message);
  process.exit(1);
}

#!/usr/bin/env node
/**
 * WCAG AA gate. The estate already computes palette pairs into dist/contrast-report.txt; this gate refuses a preset
 * whose report has a failing pair, a preset Darkhold imports but never reports, and any imported palette the shell
 * cannot select. It is deliberately text-based: a model without vision can check it, and a reviewer can read the diff.
 */
import assert from 'node:assert/strict';
import fs from 'node:fs';

const report = fs.readFileSync('node_modules/@polaris/ui/dist/contrast-report.txt', 'utf8');
const tokens = fs.readFileSync('src/styles/tokens.css', 'utf8');
const theme = fs.readFileSync('src/lib/theme.ts', 'utf8');
const imported = [...tokens.matchAll(/@import '@polaris\/ui\/palettes\/([^.]+)\.css'/g)].map((m) => m[1]);
const presets = [...new Set([...theme.matchAll(/'([a-z-]+)'/g)].map((m) => m[1])
  .filter((v) => imported.includes(v)))];

const failures = report.split(/\r?\n/).filter((line) => /\bfail\b/i.test(line));
assert.equal(failures.length, 0, `contrast failures: ${failures.join(' | ')}`);
for (const preset of presets) {
  assert.match(report, new RegExp(`## ${preset}\\b`), `no contrast report section for ${preset}`);
}
for (const palette of imported) {
  assert.ok(presets.includes(palette), `palette imported but not selectable: ${palette}`);
}
console.log(`contrast: ${presets.length} imported presets, 0 failures (${presets.join(', ')})`);

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
const out = execFileSync(process.execPath, ['scripts/contrast-check.mjs'], { encoding: 'utf8' });
assert.match(out, /0 failures/);
console.log('contrast-check: green');

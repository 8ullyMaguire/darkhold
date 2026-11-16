import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
const out = execFileSync(process.execPath, ['scripts/feature-inventory.mjs', '--check'], { encoding: 'utf8' });
assert.match(out, /none lost/);
console.log('feature-inventory: baseline green');

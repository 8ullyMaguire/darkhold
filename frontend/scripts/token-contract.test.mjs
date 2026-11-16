import assert from 'node:assert/strict';
import fs from 'node:fs';

const css = fs.readFileSync('src/styles/tokens.css', 'utf8');
for (const token of [
  '--density-row', '--tap-min', '--provenance-hosted', '--provenance-referenced',
  '--provenance-held', '--provenance-unknown', '--status-live', '--status-held',
  '--status-private', '--motion-fast',
]) {
  assert.match(css, new RegExp(`${token}\\s*:`), `missing token ${token}`);
}
const components = fs.readdirSync('src/lib/components').filter((f) => f.endsWith('.svelte'));
for (const file of components) {
  const text = fs.readFileSync(`src/lib/components/${file}`, 'utf8');
  assert.doesNotMatch(text, /#[0-9a-fA-F]{3,8}\b/, `literal colour in ${file}`);
}
console.log('token-contract: complete, no literal component colours');

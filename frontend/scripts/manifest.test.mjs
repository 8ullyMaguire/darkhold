import assert from 'node:assert/strict';
import fs from 'node:fs';

const manifest = JSON.parse(fs.readFileSync('static/manifest.webmanifest', 'utf8'));
const router = fs.readFileSync('src/lib/router.ts', 'utf8');
const iconSources = manifest.icons.map((icon) => icon.src);
for (const src of ['/icon-192.png', '/icon-512.png', '/icon-maskable-512.png']) {
  assert.ok(iconSources.includes(src), `manifest is missing ${src}`);
  assert.ok(fs.existsSync(`static${src}`), `${src} is referenced but not a file`);
}
assert.ok(manifest.icons.some((icon) => icon.purpose === 'maskable'), 'no maskable icon');
assert.equal(manifest.display, 'standalone');
assert.equal(manifest.start_url, manifest.scope);
assert.ok(manifest.shortcuts.length >= 2, 'shortcuts are the install-time destinations');
for (const shortcut of manifest.shortcuts) {
  assert.match(router, new RegExp(`'${shortcut.url}':`), `shortcut ${shortcut.url} is not a route`);
}
assert.match(router, new RegExp(`'${manifest.share_target.action}':`), 'share target is not a route');
console.log('manifest: icons, maskable icon, shortcuts, share target all present');

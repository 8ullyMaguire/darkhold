#!/usr/bin/env node
/**
 * The preservation contract for the redesign (plan phase 0).
 *
 * Baselines are deliberately dumb sets of strings. A redesign can move a component or rename a CSS class; it may not
 * silently drop a destination, a route, or a requirement id. Run with --write once, then only with --check in CI.
 */
import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd().endsWith('/frontend') ? process.cwd() : path.join(process.cwd(), 'frontend');
const repo = path.dirname(root);
const write = process.argv.includes('--write');

function pathsFromRouter() {
  const src = fs.readFileSync(path.join(root, 'src/lib/router.ts'), 'utf8');
  return [...src.matchAll(/'(\/[^']*)'/g)].map((m) => m[1]).filter((p) => !p.includes('${') && p.length > 1);
}

function navFromApp() {
  const src = fs.readFileSync(path.join(root, 'src/App.svelte'), 'utf8');
  return [...src.matchAll(/href:\s*'(\/[^']*)'/g)].map((m) => m[1]);
}

function routeEntries() {
  const src = fs.readFileSync(path.join(repo, 'crates/app/src/routes/inventory.rs'), 'utf8');
  return [...src.matchAll(/path:\s*"([^"]+)"/g)].map((m) => m[1]);
}

/** A CSV reader that respects quoted commas AND quoted newlines; the ledger has both in requirement text. */
function readCsv(text) {
  const rows = [];
  let row = [];
  let field = '';
  let quoted = false;
  const src = text.replace(/^\uFEFF/, '');
  for (let i = 0; i < src.length; i++) {
    const ch = src[i];
    if (ch === '"') {
      if (quoted && src[i + 1] === '"') { field += '"'; i += 1; }
      else quoted = !quoted;
    } else if (ch === ',' && !quoted) {
      row.push(field); field = '';
    } else if (ch === '\n' && !quoted) {
      row.push(field); field = '';
      if (row.some((f) => f.trim())) rows.push(row);
      row = [];
    } else if (ch !== '\r' || quoted) {
      field += ch;
    }
  }
  row.push(field);
  if (row.some((f) => f.trim())) rows.push(row);
  return rows;
}

function requirementIds() {
  const rows = readCsv(fs.readFileSync(path.join(repo, 'docs/requirements.csv'), 'utf8'));
  const header = rows[0].map((h) => h.trim());
  const col = header.indexOf('id');
  if (col < 0) throw new Error('requirements.csv has no id column');
  return rows.slice(1).map((r) => r[col]?.trim()).filter(Boolean);
}

const current = {
  routes: [...new Set(routeEntries())].sort(),
  nav: [...new Set(navFromApp())].sort(),
  clientPaths: [...new Set(pathsFromRouter())].sort(),
  requirements: requirementIds(),
};

const baselineDir = path.join(root, 'scripts/baselines');
fs.mkdirSync(baselineDir, { recursive: true });
const baselineFile = path.join(baselineDir, 'feature-inventory-v1.json');

if (write) {
  fs.writeFileSync(baselineFile, JSON.stringify(current, null, 2) + '\n');
  console.log(`inventory: wrote ${current.routes.length} routes, ${current.nav.length} nav links, ${current.clientPaths.length} client paths, ${current.requirements.length} requirements`);
  process.exit(0);
}

const baseline = JSON.parse(fs.readFileSync(baselineFile, 'utf8'));
let failures = 0;
for (const key of ['routes', 'nav', 'clientPaths', 'requirements']) {
  const missing = baseline[key].filter((x) => !current[key].includes(x));
  if (missing.length) {
    failures += missing.length;
    console.error(`inventory: ${key} lost ${missing.length}: ${missing.slice(0, 8).join(', ')}`);
  }
}
if (new Set(current.requirements).size !== current.requirements.length) {
  failures++;
  console.error('inventory: duplicate requirement id');
}
if (failures) process.exit(1);
console.log(`inventory: ${current.routes.length} routes, ${current.nav.length} nav links, ${current.clientPaths.length} client paths, ${current.requirements.length} requirements — none lost`);

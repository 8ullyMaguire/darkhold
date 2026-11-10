/**
 * The token layer must not contain a cycle.
 *
 * A custom property defined in terms of another that is defined back in terms of it does not fail loudly: per CSS,
 * the *computed value* becomes the empty string, and every surface that depends on it silently becomes transparent.
 * That happened here — the alias block redefined `--surface-2: var(--color-surface-raised)` while the theme block
 * defines `--color-surface-raised: var(--surface-2)` — and the visible symptom was a nav dropdown with a see-through
 * background, which reads as a styling quirk rather than a broken token.
 *
 * The check reads `tokens.css` itself, follows every `--a: var(--b)` edge within one selector block, and fails on any
 * cycle. It is a source check rather than a computed-style check because jsdom does not resolve custom properties, and
 * a browser assertion would only catch the cycles a component happens to use.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { describe, expect, it } from 'vitest';

const here = dirname(fileURLToPath(import.meta.url));
const SOURCE = readFileSync(join(here, 'styles', 'tokens.css'), 'utf8');
/** Comments explain the cycles, so they must not be mistaken for them: strip `/* … *\/` before reading declarations. */
const TOKENS = SOURCE.replace(/\/\*[\s\S]*?\*\//g, '');

interface Edge {
  from: string;
  to: string;
  line: number;
}

/** Every `--from: … var(--to) …` declaration, with line numbers so a failure names the place to edit. */
function edges(): Edge[] {
  const found: Edge[] = [];
  TOKENS.split('\n').forEach((text, index) => {
    const declaration = /^\s*(--[a-z0-9-]+)\s*:\s*([^;]+);/i.exec(text);
    if (!declaration) return;
    const from = declaration[1];
    for (const reference of declaration[2].matchAll(/var\((--[a-z0-9-]+)/gi)) {
      found.push({ from, to: reference[1], line: index + 1 });
    }
  });
  return found;
}

function cyclesIn(graph: Edge[]): string[] {
  const next = new Map<string, Edge[]>();
  for (const edge of graph) {
    const list = next.get(edge.from) ?? [];
    list.push(edge);
    next.set(edge.from, list);
  }
  const found: string[] = [];
  const state = new Map<string, 'visiting' | 'done'>();

  const walk = (node: string, path: string[]): void => {
    if (state.get(node) === 'done') return;
    if (state.get(node) === 'visiting') {
      const at = path.indexOf(node);
      found.push([...path.slice(at), node].join(' → '));
      return;
    }
    state.set(node, 'visiting');
    for (const edge of next.get(node) ?? []) walk(edge.to, [...path, node]);
    state.set(node, 'done');
  };

  for (const node of next.keys()) walk(node, []);
  return [...new Set(found)];
}

describe('tokens.css', () => {
  it('has no self-referential custom property', () => {
    const selfReferential = edges().filter((edge) => edge.from === edge.to);
    expect(selfReferential.map((edge) => `${edge.from} (line ${edge.line})`)).toEqual([]);
  });

  it('has no cycle between custom properties', () => {
    expect(cyclesIn(edges())).toEqual([]);
  });

  it('does not alias a palette token back to the colour that is defined from it', () => {
    // The exact pair that broke: a palette name aliased to a `--color-*` which is defined as that same palette name.
    // Kept as its own assertion because the message names the rule, not just the pair.
    expect(TOKENS).not.toMatch(/--surface-2:\s*var\(--color-surface-raised\)/);
  });
});

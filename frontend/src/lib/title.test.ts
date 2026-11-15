import { describe, expect, it } from 'vitest';
import { matchRoute, type RouteId } from './router';
import { ROUTE_NAMES, TAGLINE, titleFor } from './title';

/**
 * The defect these tests exist for: nine addresses answered with one `<title>`, because nothing in the
 * SPA ever assigned `document.title`. A test that only checked "the app sets a title" would pass on the
 * broken tree (the shell already had one), so every case here is about DISTINCTNESS, which is the
 * property that was missing.
 */

function matchFor(id: RouteId) {
  const params: Record<string, string> = {};
  if (id === 'pseud-profile') params.handle = 'usuki';
  if (id === 'doc-page') params.slug = 'getting-started';
  return { id, path: '/x', params };
}

describe('the tab title', () => {
  it('gives every route a title of its own', () => {
    const ids = Object.keys(ROUTE_NAMES) as RouteId[];
    const titles = ids.map((id) => titleFor(matchFor(id)));
    const seen = new Map<string, string>();
    for (const [i, title] of titles.entries()) {
      expect(title.trim(), `route ${ids[i]} has no title`).not.toBe('');
      const other = seen.get(title);
      expect(other, `route ${ids[i]} and ${other} share the title "${title}"`).toBeUndefined();
      seen.set(title, ids[i]);
    }
  });

  it('keeps the tagline for the front page, which is the site', () => {
    expect(titleFor(matchFor('home'))).toBe(TAGLINE);
  });

  it('names a page in its own words on a client-side navigation', () => {
    // The whole point: two real paths that used to be indistinguishable.
    expect(titleFor(matchRoute('/discover'))).toBe('Discover — Darkhold');
    expect(titleFor(matchRoute('/community'))).toBe('Community — Darkhold');
    expect(titleFor(matchRoute('/discover'))).not.toBe(titleFor(matchRoute('/community')));
  });

  it('puts the handle in a public profile title', () => {
    const title = titleFor(matchRoute('/pseud/usuki'));
    expect(title).toBe('@usuki — Darkhold');
  });

  it('reads a docs slug as a sentence, not as an address', () => {
    expect(titleFor(matchRoute('/docs/getting-started'))).toBe('Getting started — Darkhold');
  });

  it('title-cases a slug without inventing words', () => {
    // A slug is the address; the tab repeats it rather than guessing a nicer name for it.
    expect(titleFor(matchRoute('/docs/next-steps'))).toBe('Next steps — Darkhold');
  });

  it('says so when a path is not a page', () => {
    expect(titleFor(matchRoute('/no-such-path'))).toBe('Page not found — Darkhold');
  });
});

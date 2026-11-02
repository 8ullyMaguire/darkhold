import { describe, expect, it } from 'vitest';

import {
  moduleNavEntries,
  visibleNav,
  type ModuleCatalogue,
  type NavLink,
} from './modules';

function catalogue(rows: Array<[string, boolean, boolean]>): ModuleCatalogue {
  return {
    core: ['accounts'],
    modules: rows.map(([id, enabled, enabledForCaller]) => ({
      id,
      enabled,
      enabled_for_caller: enabledForCaller,
      nav: [],
    })),
  };
}

const NAV: NavLink[] = [
  { href: '/discover', label: 'Discover', module: 'discovery' },
  { href: '/arena', label: 'Arena', module: 'arena' },
  { href: '/write', label: 'Write' },
];

describe('visibleNav', () => {
  it('hides the entry whose module the instance switched off', () => {
    // The bug M65-06 exists for: the link rendered and the destination answered 404, and nothing
    // counted it as a failure because the nav never asked what the instance runs.
    const visible = visibleNav(NAV, catalogue([['discovery', true, true], ['arena', false, false]]));
    expect(visible.map((item) => item.href)).toEqual(['/discover', '/write']);
  });

  it("hides an entry the reader switched off, not only one the operator switched off", () => {
    // `enabled` true and `enabled_for_caller` false is §59.6's override: the module runs, this
    // reader asked for it not to. Rendering the link would offer a surface they opted out of.
    const visible = visibleNav(NAV, catalogue([['discovery', true, true], ['arena', true, false]]));
    expect(visible.map((item) => item.href)).toEqual(['/discover', '/write']);
  });

  it('hides an entry whose module the catalogue does not list at all', () => {
    // A build without the surface is the same 404 as a disabled module, and this is what makes the
    // mapping safe to write before the module exists.
    const visible = visibleNav(NAV, catalogue([['discovery', true, true]]));
    expect(visible.map((item) => item.href)).toEqual(['/discover', '/write']);
  });

  it('renders everything when the catalogue could not be fetched', () => {
    // Fail open, deliberately: a hidden destination breaks the site for every reader because one
    // request timed out, which is worse and affects more people than a link that 404s.
    expect(visibleNav(NAV, null).map((item) => item.href)).toEqual([
      '/discover',
      '/arena',
      '/write',
    ]);
  });

  it('never drops an entry that belongs to no module', () => {
    const visible = visibleNav(NAV, catalogue([]));
    expect(visible.map((item) => item.href)).toEqual(['/write']);
  });

  it('preserves order and does not mutate the input', () => {
    const input: NavLink[] = [...NAV];
    const visible = visibleNav(input, catalogue([['discovery', true, true], ['arena', true, true]]));
    expect(visible.map((item) => item.href)).toEqual(input.map((item) => item.href));
    expect(input).toHaveLength(3);
  });
});

describe('moduleNavEntries', () => {
  it('is empty when no module has declared an entry — which is every row today', () => {
    expect(moduleNavEntries(catalogue([['quiz', true, true]]))).toEqual([]);
  });

  it('collects the entries of enabled modules only, once per href', () => {
    const payload: ModuleCatalogue = {
      core: [],
      modules: [
        {
          id: 'quiz',
          enabled: true,
          enabled_for_caller: true,
          nav: [{ label: 'Quiz', href: '/quiz', slot: 'reader-sidebar' }],
        },
        {
          id: 'arena',
          enabled: false,
          enabled_for_caller: false,
          nav: [{ label: 'Arena', href: '/arena', slot: 'dashboard' }],
        },
        {
          id: 'tasting',
          enabled: true,
          enabled_for_caller: true,
          // The same destination declared twice is one link, not two.
          nav: [{ label: 'Quiz', href: '/quiz', slot: 'dashboard' }],
        },
      ],
    };
    expect(moduleNavEntries(payload).map((entry) => entry.href)).toEqual(['/quiz']);
  });

  it('contributes nothing when the catalogue is unavailable', () => {
    expect(moduleNavEntries(null)).toEqual([]);
  });
});

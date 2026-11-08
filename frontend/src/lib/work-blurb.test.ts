import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import { WorkBlurb } from '@polaris/ui';

/**
 * M72-10: the blurb's contract is the *order*, not the look.
 *
 * Two products listing the same work must put the same facts in the same places — title, byline, meta chips,
 * warnings, tags, summary — so a reader moving from one to the other is not re-learning a layout. The assertion
 * reads the rendered element order rather than snapshotting the markup: a snapshot breaks on a class rename and
 * passes on a field moving, which is exactly backwards.
 */
const full = {
  href: '/works/42',
  title: 'The Long Way Home',
  authors: ['Ada Lovelace', 'Grace Hopper'],
  meta: ['Teen And Up Audiences', 'Complete', '4,210 words'],
  warnings: 'No Archive Warnings Apply',
  tags: [
    { label: 'Slow Burn', href: '/tags/slow-burn' },
    { label: 'Found Family' },
  ],
  summary: 'A story about coming back.',
};

const order = (container: HTMLElement) =>
  Array.from(container.querySelectorAll('h3, .byline, .meta, .warnings, .tags, .summary')).map((el) =>
    el.classList[0],
  );

describe('WorkBlurb', () => {
  it('puts the fields in the family order', () => {
    const { container } = render(WorkBlurb, { props: { work: full } });
    expect(order(container)).toEqual(['title', 'byline', 'meta', 'warnings', 'tags', 'summary']);
  });

  it('links the title only when the work has somewhere to go', () => {
    const linked = render(WorkBlurb, { props: { work: full } });
    expect(linked.getByRole('link', { name: 'The Long Way Home' }).getAttribute('href')).toBe('/works/42');

    // Scoped to this render's container: testing-library's bound queries search `document.body`, so a second
    // render in the same test would otherwise find the first render's link and "prove" the opposite.
    const bare = render(WorkBlurb, { props: { work: { ...full, href: undefined } } });
    expect(bare.container.querySelector('.title a')).toBeNull();
  });

  it('joins authors with "and", not a comma list', () => {
    const { container } = render(WorkBlurb, { props: { work: full } });
    const byline = container.querySelector('.byline')?.textContent?.replace(/\s+/g, ' ').trim();
    expect(byline).toBe('by Ada Lovelace and Grace Hopper');
  });

  it('omits absent fields instead of rendering empty rows', () => {
    const { container } = render(WorkBlurb, {
      props: { work: { title: 'Bare', summary: undefined, tags: [], meta: [], authors: [] } },
    });
    // A blurb with only a title is a title and nothing else — no empty byline, no empty chip row.
    expect(order(container)).toEqual(['title']);
  });

  it('clamps the summary in inline size instead of slicing the string', () => {
    const { container } = render(WorkBlurb, { props: { work: full, size: 'inline' } });
    // The full text stays in the DOM (CSS clamps it): slicing would break the sentence a reader is deciding on.
    const summary = container.querySelector('.summary')?.textContent;
    expect(summary).toBe(full.summary);
    expect(container.querySelector('article')?.getAttribute('data-size')).toBe('inline');
  });
});

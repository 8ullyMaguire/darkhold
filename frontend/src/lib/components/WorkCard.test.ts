import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/svelte';
import WorkCard, { type WorkSummary } from './WorkCard.svelte';

/** A minimal work summary for the metric bar to render from. */
function sampleWork(overrides: Partial<WorkSummary> = {}): WorkSummary {
  return {
    id: 'w-1',
    title: 'A Test Work',
    authorDisplayName: 'Alice',
    completion: 'complete',
    rating: 'general',
    centralRelationships: [],
    wordCount: 5000,
    units: 3,
    ...overrides,
  };
}

describe('WorkCard', () => {
  it('shows the metric bar when metrics are present', () => {
    const { getByText } = render(WorkCard, {
      props: { work: sampleWork({ metrics: { views: 1200, kudos: 42, reactions: 5, complete_reads: 10, bookmarks: 3, collection_adds: 2, reviews: 1 } }) },
    });
    expect(getByText('1.2k views')).toBeTruthy();
    expect(getByText('42 kudos')).toBeTruthy();
  });

  it('hides the metric bar when the owner opted out', () => {
    const { queryByText } = render(WorkCard, {
      props: { work: sampleWork({ metrics: null }) },
    });
    expect(queryByText('views')).toBeFalsy();
  });

  it('formats large counts with a k suffix', () => {
    const { getByText } = render(WorkCard, {
      props: {
        work: sampleWork({
          metrics: { views: 12345, kudos: 6789, reactions: 12, complete_reads: 5, bookmarks: 3, collection_adds: 2, reviews: 1 },
        }),
        variant: 'full',
      },
    });
    expect(getByText('12k views')).toBeTruthy();
    expect(getByText('6.8k kudos')).toBeTruthy();
  });

  it('renders the blurb in the family order — title, byline, chips, summary', () => {
    // M72-06: the card no longer owns this order; `WorkBlurb` does. The assertion reads document order because
    // that is the contract two products have to agree on — a snapshot would break on a class rename and pass on a
    // field moving, which is exactly backwards.
    const { container } = render(WorkCard, {
      props: {
        work: sampleWork({ summary: 'A summary that should come after the chips.' }),
        variant: 'full',
      },
    });

    const title = container.querySelector('h3');
    const byline = container.querySelector('.byline');
    const meta = container.querySelector('.meta');
    const summary = container.querySelector('.summary');
    for (const [name, node] of Object.entries({ title, byline, meta, summary })) {
      expect(node, `${name} missing from the card`).toBeTruthy();
    }

    const before = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(before(title!, byline!)).toBe(true);
    expect(before(byline!, meta!)).toBe(true);
    expect(before(meta!, summary!)).toBe(true);
  });
});


describe('M45-76 provenance', () => {
  it('names the configured window only for a sweep abandonment', () => {
    const { getByText } = render(WorkCard, {
      props: {
        work: sampleWork({
          completion: 'abandoned',
          completion_source: 'sweep',
        }),
        abandonAfterMonths: 7,
      },
    });
    expect(getByText('Abandoned · no update in 7 months')).toBeTruthy();
  });

  it('keeps the plain label for an owner abandonment', () => {
    const { getByText, queryByText } = render(WorkCard, {
      props: {
        work: sampleWork({
          completion: 'abandoned',
          completion_source: 'owner',
        }),
        abandonAfterMonths: 7,
      },
    });
    expect(getByText('Abandoned')).toBeTruthy();
    expect(queryByText('Abandoned · no update in 7 months')).toBeFalsy();
  });

  it('says unmeasured when availability is not supplied', () => {
    const { getByText } = render(WorkCard, { props: { work: sampleWork() } });
    expect(getByText('unmeasured')).toBeTruthy();
  });

  it('prints an explicit availability on the card', () => {
    const { getByText } = render(WorkCard, {
      props: { work: sampleWork(), availability: 'Reference only · text not held' },
    });
    expect(getByText('Reference only · text not held')).toBeTruthy();
  });
});

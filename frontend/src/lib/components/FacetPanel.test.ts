import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import FacetPanel from './FacetPanel.svelte';

const facets = [
  {
    facet: 'length',
    buckets: [
      { key: 'short', label: 'Short', count: 12 },
      { key: 'long', label: 'Long', count: 3 },
    ],
    known: 15,
    unknown: 2,
    when_unknown: '2 works have no measured length',
  },
  {
    facet: 'status',
    buckets: [{ key: 'complete', label: 'Complete', count: 9 }],
    known: 9,
    unknown: 0,
    when_unknown: null,
  },
];

describe('FacetPanel', () => {
  it('renders every group with its measured counts and the set they describe', () => {
    const { getByText, getAllByRole } = render(FacetPanel, {
      props: {
        facets,
        facetsOver: 'the catalogue under the word window',
        ageMs: 4500,
      },
    });
    expect(getByText('Short')).toBeTruthy();
    expect(getByText('12')).toBeTruthy();
    expect(getByText('Complete')).toBeTruthy();
    expect(getByText('the catalogue under the word window')).toBeTruthy();
    expect(getByText(/4\.5s ago/)).toBeTruthy();
    expect(getByText('2 works have no measured length')).toBeTruthy();
    expect(getAllByRole('group')).toHaveLength(2);
  });

  it('formats large counts with separators and calls a fresh count current', () => {
    const { getByText } = render(FacetPanel, {
      props: {
        facets: [{
          facet: 'status',
          buckets: [{ key: 'complete', label: 'Complete', count: 58_042 }],
          known: 58_042,
          unknown: 0,
          when_unknown: null,
        }],
        ageMs: 0,
      },
    });
    expect(getByText('58,042')).toBeTruthy();
    expect(getByText('Counts: now')).toBeTruthy();
  });

  it('leads the fandom facet with coverage from the one aggregate the API already returns', () => {
    const { getByText } = render(FacetPanel, {
      props: {
        facets: [{
          facet: 'fandom',
          buckets: [{ key: 'svsss', label: 'SVSSS', count: 12 }],
          known: 12,
          unknown: 3,
          when_unknown: null,
        }],
      },
    });
    expect(getByText('12 of 15 works are tagged (80%)')).toBeTruthy();
  });

  it('shows an em dash while counts are pending, never a zero that would be a measurement', () => {
    const { getAllByText, queryByText } = render(FacetPanel, {
      props: { facets, pending: true },
    });
    expect(getAllByText('—')).toHaveLength(3);
    expect(queryByText('12')).toBeNull();
    expect(queryByText('0')).toBeNull();
  });

  it('reports the facet and bucket that were chosen, in the query vocabulary', async () => {
    const onselect = vi.fn();
    const { getByText } = render(FacetPanel, { props: { facets, onselect } });
    await fireEvent.click(getByText('Long'));
    expect(onselect).toHaveBeenCalledWith('length', 'long');
  });
});

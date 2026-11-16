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

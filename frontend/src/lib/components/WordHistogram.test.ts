import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import WordHistogram from './WordHistogram.svelte';

const histogram = {
  unit: 'words',
  buckets: [
    { index: 0, low: 0, high: 999, count: 12, share: 0.6 },
    { index: 1, low: 1000, high: 4999, count: 8, share: 0.4 },
  ],
  measured: 20,
  unknown: 3,
  total: 23,
  when_unknown: '3 works have no measured length',
};

describe('WordHistogram', () => {
  it('draws the server buckets with their own edges and share, and states the unmeasured rows', () => {
    const { getByText, getAllByRole } = render(WordHistogram, { props: { histogram } });
    expect(getByText('Under 1,000 words')).toBeTruthy();
    expect(getByText('1,000 – 4,999 words')).toBeTruthy();
    expect(getByText('3 works have no measured length')).toBeTruthy();
    const bars = getAllByRole('button');
    expect(bars).toHaveLength(2);
    expect(bars[0].getAttribute('data-count')).toBe('12');
    expect(bars[0].style.height).toBe('60%');
  });

  it('reports the inclusive edges of the bar that was chosen', async () => {
    const onselect = vi.fn();
    const { getAllByRole } = render(WordHistogram, { props: { histogram, onselect } });
    await fireEvent.click(getAllByRole('button')[1]);
    expect(onselect).toHaveBeenCalledWith(1000, 4999);
  });

  it('formats large bucket counts with separators', () => {
    const big = {
      ...histogram,
      buckets: [{ index: 0, low: 0, high: 999, count: 12_345, share: 1 }],
    };
    const { getByText } = render(WordHistogram, { props: { histogram: big } });
    expect(getByText('12,345')).toBeTruthy();
  });

  it('shows an em dash rather than a zero while the distribution is pending', () => {
    const { getAllByText } = render(WordHistogram, { props: { histogram, pending: true } });
    expect(getAllByText('—')).toHaveLength(2);
  });
});

import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import QueryStatePanel from './QueryStatePanel.svelte';

const state = {
  query: 'story AND words:>=1000',
  chips: [
    {
      kind: 'min_words',
      value: '1000',
      label: '1000+ words',
      editable_in_query: true,
      removable: true,
      reason: null,
    },
    {
      kind: 'pinned:ai_disclosure',
      value: 'disclose',
      label: 'AI disclosure on',
      editable_in_query: false,
      removable: false,
      reason: 'the query grammar has no field for an AI statement',
    },
    {
      kind: 'sort',
      value: 'relevance',
      label: 'sorted by relevance',
      editable_in_query: false,
      removable: true,
      reason: 'the query grammar has no sort field; the ordering control owns it',
    },
  ],
  unexpressed_note: 'One content filter is applied outside the query.',
};

describe('QueryStatePanel', () => {
  it('renders the server chips with their reasons and the note', () => {
    const { getByText } = render(QueryStatePanel, { props: { state } });
    expect(getByText('1000+ words')).toBeTruthy();
    expect(getByText('AI disclosure on')).toBeTruthy();
    expect(getByText('the query grammar has no field for an AI statement')).toBeTruthy();
    expect(getByText('One content filter is applied outside the query.')).toBeTruthy();
  });

  it('gives no remove control to a pinned chip that says why it cannot be removed', () => {
    const onremove = vi.fn();
    const { getByText, getAllByRole } = render(QueryStatePanel, { props: { state, onremove } });
    // Only the two removable chips get buttons.
    expect(getAllByRole('button').filter((b) => b.getAttribute('aria-label')?.startsWith('Remove'))).toHaveLength(2);
    expect(getByText('AI disclosure on').closest('li')?.querySelector('button')).toBeNull();
  });

  it('reports the chip chosen for removal', async () => {
    const onremove = vi.fn();
    const { getByRole } = render(QueryStatePanel, { props: { state, onremove } });
    await fireEvent.click(getByRole('button', { name: 'Remove 1000+ words' }));
    expect(onremove).toHaveBeenCalledWith('min_words', '1000');
  });

  it('offers the canonical query as an explicit action, not a silent overwrite', async () => {
    const onusequery = vi.fn();
    const { getByRole, getByText } = render(QueryStatePanel, { props: { state, onusequery } });
    expect(getByText('story AND words:>=1000')).toBeTruthy();
    await fireEvent.click(getByRole('button', { name: 'Use this query' }));
    expect(onusequery).toHaveBeenCalledWith('story AND words:>=1000');
  });
});

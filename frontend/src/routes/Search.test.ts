import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import Search from './Search.svelte';

const searchWorks = vi.fn();
const fetchSearchHistory = vi.fn();
const clearSearchHistory = vi.fn();
const fetchWordHistogram = vi.fn();
const createCatalogueView = vi.fn();
const createSearchAlert = vi.fn();

vi.mock('../lib/api', () => ({
  searchWorks: (...args: unknown[]) => searchWorks(...args),
  fetchSearchHistory: (...args: unknown[]) => fetchSearchHistory(...args),
  clearSearchHistory: (...args: unknown[]) => clearSearchHistory(...args),
  fetchWordHistogram: (...args: unknown[]) => fetchWordHistogram(...args),
  ApiError: class ApiError extends Error {
    isAuthRequired = false;
  },
  createCatalogueView: (...args: unknown[]) => createCatalogueView(...args),
  createSearchAlert: (...args: unknown[]) => createSearchAlert(...args),
}));

vi.mock('../lib/session.svelte', () => ({
  session: { isSignedIn: true },
}));

beforeEach(() => {
  searchWorks.mockReset();
  searchWorks.mockResolvedValue({ items: [] });
  fetchSearchHistory.mockReset();
  fetchSearchHistory.mockResolvedValue({ items: [], recording: true });
  clearSearchHistory.mockReset();
  clearSearchHistory.mockResolvedValue({ cleared: 0 });
  fetchWordHistogram.mockReset();
  fetchWordHistogram.mockResolvedValue({ unit: 'words', buckets: [], measured: 0, unknown: 0, total: 0 });
  createCatalogueView.mockReset();
  createSearchAlert.mockReset();
});

describe('Search page', () => {
  it('renders search input and filter options', () => {
    render(Search);
    expect(
      screen.getByPlaceholderText('e.g. fandom:"Harry Potter" AND tag:draco'),
    ).toBeInTheDocument();
    expect(screen.getByText('Field')).toBeInTheDocument();
  });

  it('shows the empty state before any search', () => {
    render(Search);
    expect(screen.getByText(/search works/i)).toBeInTheDocument();
  });

  it('renders filter field options', () => {
    render(Search);
    expect(screen.getByText('Fandom')).toBeInTheDocument();
    expect(screen.getByText('Tag')).toBeInTheDocument();
    expect(screen.getByText('Rating')).toBeInTheDocument();
  });

  it('saves a zero-result query as a view and attaches one daily alert', async () => {
    createCatalogueView.mockResolvedValue({ id: 'view-1' });
    createSearchAlert.mockResolvedValue({ id: 'alert-1', status: 'created' });
    render(Search);
    await fireEvent.input(screen.getByLabelText('Search query'), {
      target: { value: 'tag:no-such-tag' },
    });
    await fireEvent.click(screen.getByText('Search'));
    await waitFor(() => expect(screen.getByText('No matches found')).toBeTruthy());
    await fireEvent.click(screen.getByText('Save and get new matches'));
    await waitFor(() =>
      expect(createCatalogueView).toHaveBeenCalledWith({
        name: 'tag:no-such-tag',
        kind: 'catalogue',
        params: 'tag:no-such-tag',
        scope: 'private',
      }),
    );
    expect(createSearchAlert).toHaveBeenCalledWith({
      saved_search_id: 'view-1',
      frequency: 'daily',
    });
    expect(await screen.findByText(/a daily alert is on/i)).toBeTruthy();
  });
});

describe('Search page word-count range', () => {
  async function type(label: string, value: string) {
    await fireEvent.input(screen.getByLabelText(label), { target: { value } });
  }

  async function submit() {
    await fireEvent.click(screen.getByText('Search'));
  }

  it('offers both bounds, each independently optional', () => {
    render(Search);
    // Two inputs rather than a slider: "over 10k, no upper bound" has to be
    // expressible, and a slider cannot leave one end open.
    expect(screen.getByLabelText('Min words')).toBeInTheDocument();
    expect(screen.getByLabelText('Max words')).toBeInTheDocument();
    expect(screen.getByLabelText('Min words')).toHaveAttribute('placeholder', 'Any');
  });

  it('sends a minimum as words:>=', async () => {
    render(Search);
    await type('Min words', '10000');
    await submit();
    await waitFor(() => expect(searchWorks).toHaveBeenCalled());
    expect(searchWorks.mock.calls[0][0]).toBe('words:>=10000');
  });

  it('sends a maximum as words:<=', async () => {
    render(Search);
    await type('Max words', '50000');
    await submit();
    await waitFor(() => expect(searchWorks).toHaveBeenCalled());
    expect(searchWorks.mock.calls[0][0]).toBe('words:<=50000');
  });

  it('combines both bounds as one range', async () => {
    render(Search);
    await type('Min words', '1000');
    await type('Max words', '5000');
    await submit();
    await waitFor(() => expect(searchWorks).toHaveBeenCalled());
    // One `..` term rather than two comparisons: it is the spelling the query
    // language documents, and the server expands it to the same pair.
    expect(searchWorks.mock.calls[0][0]).toBe('words:1000..5000');
  });

  it('ANDs the range with free text', async () => {
    render(Search);
    await type('Search query', 'winter');
    await type('Min words', '10000');
    await submit();
    await waitFor(() => expect(searchWorks).toHaveBeenCalled());
    expect(searchWorks.mock.calls[0][0]).toBe('winter AND words:>=10000');
  });

  it('sends nothing while both bounds are still empty', async () => {
    render(Search);
    // Focusing the fields and leaving them blank must not produce `words:>=`,
    // which the server rejects with a 422. The reader would see an error for a
    // field they never filled in.
    await fireEvent.click(screen.getByLabelText('Min words'));
    await fireEvent.click(screen.getByLabelText('Max words'));
    await submit();
    expect(searchWorks).not.toHaveBeenCalled();
  });

  it('ignores a bound that is not a number', async () => {
    render(Search);
    // `type="number"` is a hint, not a guarantee: a pasted value can still
    // reach the handler, and the query must not carry it either way.
    await type('Min words', 'ten thousand');
    await submit();
    expect(searchWorks).not.toHaveBeenCalled();
  });

  it('drops a range whose bounds are the wrong way round', async () => {
    render(Search);
    // The server rejects `1000..5000` with a 422, and it is right to: the
    // range can never match. Rather than let the reader submit a query that is
    // guaranteed to fail, the form leaves the range out -- the same shape as
    // any other unreadable value, and the free text below still searches.
    await type('Min words', '5000');
    await type('Max words', '1000');
    await submit();
    expect(searchWorks).not.toHaveBeenCalled();
  });

  it('clears the range and searches again', async () => {
    render(Search);
    await type('Min words', '10000');
    await submit();
    await waitFor(() => expect(searchWorks).toHaveBeenCalledTimes(1));

    await fireEvent.click(screen.getByText('Clear'));
    await waitFor(() => expect(screen.getByLabelText('Min words')).toHaveValue(null));
    // Cleared means cleared: no dangling `words:>=` left in the next query.
    await submit();
    expect(searchWorks).toHaveBeenCalledTimes(1);
  });
});

describe('Search page presets', () => {
  it('the short-and-complete preset sets both controls and runs, without a second click', async () => {
    render(Search);
    await fireEvent.click(screen.getByText('Complete, under 10k'));

    // Both halves in one query: the bound the reader would have typed and the completion the
    // dropdown would have meant. `searchWorks` is called once, because a preset that only fills
    // the form leaves the reader pressing Search to find out what it did.
    expect(searchWorks).toHaveBeenCalledTimes(1);
    expect(searchWorks).toHaveBeenCalledWith('completion:"complete" AND words:<=10000', 20);
  });

  it('the preset speaks completion, not status', async () => {
    render(Search);
    await fireEvent.click(screen.getByText('Complete, under 10k'));

    const query = searchWorks.mock.calls[0][0] as string;
    // `status:` renders `works.lifecycle` server-side, where "Complete" is not a value: the page
    // sent that until 2026-11-02 and every filter it offered matched nothing.
    expect(query).not.toContain('status:');
    expect(query).toContain('completion:"complete"');
  });

  it('applying the preset twice does not double the completion term', async () => {
    render(Search);
    const preset = screen.getByText('Complete, under 10k');
    await fireEvent.click(preset);
    await fireEvent.click(preset);

    const last = searchWorks.mock.calls.at(-1)?.[0] as string;
    expect(last.match(/completion:/g)?.length).toBe(1);
    expect(last.match(/words:/g)?.length).toBe(1);
  });

  it('renders provenance and status from the server on each hit (M71-03)', async () => {
    searchWorks.mockResolvedValue({
      items: [{
        work_id: 'probe-1',
        title: 'The Quiet Archive',
        author_handle: 'archivist',
        word_count: 42_300,
        score: 7,
        provenance: 'referenced',
        status: 'live',
      }],
    });
    render(Search);
    await fireEvent.input(screen.getByPlaceholderText(/fandom:/), { target: { value: 'quiet' } });
    await fireEvent.click(screen.getByText('Search'));
    expect(await screen.findByText('The Quiet Archive')).toBeInTheDocument();
    expect(screen.getByText('Points elsewhere')).toBeInTheDocument();
    expect(screen.getByText('Live')).toBeInTheDocument();
    expect(screen.getByText(/42,300 words/)).toBeInTheDocument();
    expect(screen.getByText(/7 points/)).toBeInTheDocument();
  });
});

describe('Search page history', () => {
  function entry(query: string) {
    return { query, result_count: 3, searched_at: '2026-11-02T10:00:00Z' };
  }

  it('renders this reader\'s recent searches as chips', async () => {
    fetchSearchHistory.mockResolvedValue({
      items: [entry('time travel'), entry('fandom:"Naruto"')],
      recording: true,
    });
    render(Search);

    expect(await screen.findByText('time travel')).toBeInTheDocument();
    expect(screen.getByText('fandom:"Naruto"')).toBeInTheDocument();
    expect(screen.getByLabelText('Recent searches')).toBeInTheDocument();
  });

  it('renders no history row at all when there is none, signed out included', async () => {
    // The 401 an anonymous caller gets is swallowed into "none": a search page must not open
    // with an error banner for a feature nobody asked for.
    fetchSearchHistory.mockRejectedValue(new Error('401'));
    render(Search);
    await waitFor(() => expect(fetchSearchHistory).toHaveBeenCalled());

    expect(screen.queryByLabelText('Recent searches')).not.toBeInTheDocument();
    expect(screen.queryByText('Clear')).not.toBeInTheDocument();
  });

  it('re-runs a query from a chip without retyping it', async () => {
    fetchSearchHistory.mockResolvedValue({ items: [entry('time travel')], recording: true });
    render(Search);

    await fireEvent.click(await screen.findByText('time travel'));

    expect(searchWorks).toHaveBeenCalledWith('time travel', 20);
    expect((screen.getByPlaceholderText(/fandom:/) as HTMLInputElement).value).toBe('time travel');
  });

  it('Clear forgets every entry and empties the row', async () => {
    fetchSearchHistory.mockResolvedValue({ items: [entry('one')], recording: true });
    render(Search);
    await screen.findByText('one');

    // The refetch after the delete is what the row then renders.
    fetchSearchHistory.mockResolvedValue({ items: [], recording: true });
    await fireEvent.click(screen.getByText('Clear'));

    expect(clearSearchHistory).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByLabelText('Recent searches')).not.toBeInTheDocument());
  });

  it('says so when recording is off, because the entries shown are still stored', async () => {
    fetchSearchHistory.mockResolvedValue({ items: [entry('kept before')], recording: false });
    render(Search);

    expect(await screen.findByText('kept before')).toBeInTheDocument();
    expect(screen.getByText(/not being kept/i)).toBeInTheDocument();
  });

  it('re-reads the history after a search, so the server owns dedupe and the cap', async () => {
    fetchSearchHistory.mockResolvedValue({ items: [], recording: true });
    render(Search);
    await waitFor(() => expect(fetchSearchHistory).toHaveBeenCalledTimes(1));

    await fireEvent.input(screen.getByPlaceholderText(/fandom:/), {
      target: { value: 'a query' },
    });
    await fireEvent.click(screen.getByText('Search'));

    await waitFor(() => expect(fetchSearchHistory.mock.calls.length).toBeGreaterThan(1));
  });
});

describe('M71-11 facet counts on the search page', () => {
  it('renders the server counts and turns a facet click into a query term', async () => {
    searchWorks.mockResolvedValue({
      items: [],
      facets: [
        {
          facet: 'length',
          buckets: [{ key: 'under_1k', label: 'Under 1,000 words', count: 7 }],
          known: 7,
          unknown: 0,
        },
      ],
      facets_over: 'the catalogue under the word window',
      facets_computed_at_ms_ago: 1200,
    });
    render(Search);
    await fireEvent.input(screen.getByPlaceholderText(/fandom:/), { target: { value: 'story' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByLabelText('Facet counts')).toBeInTheDocument();
    expect(screen.getByText('the catalogue under the word window')).toBeInTheDocument();
    expect(screen.getByText('7')).toBeInTheDocument();

    await fireEvent.click(screen.getByText('Under 1,000 words'));
    await waitFor(() => expect(searchWorks.mock.calls[1][0]).toContain('words:<=999'));
  });
});

describe('M71-12 word histogram on the search page', () => {
  it('draws the server distribution and a bar click fills the existing word range', async () => {
    fetchWordHistogram.mockResolvedValue({
      unit: 'words',
      buckets: [{ index: 0, low: 1000, high: 4999, count: 8, share: 0.4 }],
      measured: 20,
      unknown: 3,
      total: 23,
      when_unknown: '3 works have no measured length',
    });
    render(Search);
    await fireEvent.input(screen.getByPlaceholderText(/fandom:/), { target: { value: 'story' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByLabelText('Word count distribution')).toBeInTheDocument();
    expect(screen.getByText('3 works have no measured length')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: /1,000 – 4,999 words/ }));

    await waitFor(() =>
      expect(searchWorks.mock.calls[1][0]).toContain('words:1000..4999'),
    );
    expect(screen.getByLabelText('Min words')).toHaveValue(1000);
    expect(screen.getByLabelText('Max words')).toHaveValue(4999);
  });
});

describe('M71-15 query state on the search page', () => {
  it('renders the server chips, hides an unavailable ordering control, and removes a word chip', async () => {
    searchWorks.mockResolvedValue({
      items: [],
      state: {
        query: 'story AND words:>=1000',
        chips: [
          {
            kind: 'min_words',
            value: '1000',
            label: '1000+ words',
            editable_in_query: true,
            removable: true,
          },
          {
            kind: 'sort',
            value: 'relevance',
            label: 'sorted by relevance',
            editable_in_query: false,
            removable: true,
          },
        ],
        unexpressed_note: 'One content filter is applied outside the query.',
      },
    });
    render(Search);
    await fireEvent.input(screen.getByPlaceholderText(/fandom:/), { target: { value: 'story' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByLabelText('Search state')).toBeInTheDocument();
    expect(screen.getByText('One content filter is applied outside the query.')).toBeInTheDocument();
    expect(screen.getByText('sorted by relevance')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Remove sorted by relevance' })).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: 'Remove 1000+ words' }));
    await waitFor(() => expect(searchWorks.mock.calls[1][0]).not.toContain('words:>=1000'));
    expect(screen.getByLabelText('Min words')).toHaveValue(null);
  });
});

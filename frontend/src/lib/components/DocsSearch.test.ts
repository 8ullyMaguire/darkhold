/**
 * DocsSearch: the palette's two sources, tested.
 *
 * The dialog searches the help pages in the browser and the estate index over the network. The network half is
 * the one that regresses quietly — a wrong URL, a missing debounce, an exception on a failed fetch — and it is
 * invisible in a screenshot, so it is tested here against the DOM.
 *
 * The rules pinned below:
 *   1. typing two characters asks the estate index once, through the product's own route
 *   2. a hit is shown with its site, because "which site" is the reason the estate exists
 *   3. a failed fetch shows nothing and does not throw — a palette that errors on every keystroke is worse
 *      than one that suggests nothing
 *   4. one character does not ask: a single letter matches half the index
 */
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import DocsSearch from './DocsSearch.svelte';

const HIT = {
  site: 'darkhold',
  kind: 'work',
  doc_id: '2b0e1f2a-0000-4000-8000-000000000001',
  title: 'A Work About Harry',
};

function type(value: string) {
  const input = screen.getByRole('searchbox');
  return fireEvent.input(input, { target: { value } });
}

describe('DocsSearch', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ data: [HIT] }), { status: 200 }))
    );
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('asks the estate index through the product route and shows the site', async () => {
    render(DocsSearch, { props: { open: true } });
    await type('harry');

    await waitFor(() => expect(screen.getByText('A Work About Harry')).toBeInTheDocument());
    expect(screen.getByText(/darkhold · work/)).toBeInTheDocument();

    const [url] = (fetch as unknown as { mock: { calls: string[][] } }).mock.calls[0];
    expect(url).toBe('/api/v1/estate/search?q=harry&limit=5');
  });

  it('does not ask for a single character', async () => {
    render(DocsSearch, { props: { open: true } });
    await type('h');
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(fetch).not.toHaveBeenCalled();
  });

  it('shows nothing, and does not throw, when the index is down', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('boom', { status: 503 })));
    render(DocsSearch, { props: { open: true } });
    await type('harry');

    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(screen.queryByText('A Work About Harry')).not.toBeInTheDocument();
  });
});

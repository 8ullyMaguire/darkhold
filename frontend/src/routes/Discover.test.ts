import { render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import Discover from './Discover.svelte';

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return {
    ...actual,
    fetchDiscoveryFeed: vi.fn(),
    clearTasteProfile: vi.fn(),
    recomputeTasteProfile: vi.fn(),
  };
});

import { fetchDiscoveryFeed } from '../lib/api';

const FEED = {
  items: [
    {
      work_id: 'work-1',
      title: 'Discovery Test',
      author_handle: 'alice',
      word_count: 3000,
      reason: 'Readers have rated it highly.',
      reason_code: 'rated_by_readers',
      match_percent: 87,
      rating: 'teen',
      tags: [],
      summary: 'A test work.',
      score: 0.8,
    },
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
  (fetchDiscoveryFeed as any).mockResolvedValue(FEED);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('Discover page', () => {
  it('renders the discovery feed', async () => {
    render(Discover);
    await waitFor(() => expect(screen.getAllByText(/Discovery Test/).length).toBeGreaterThan(0));
  });

  it('draws feed hits as dense Archive rows, not cards', async () => {
    const { container } = render(Discover);
    await waitFor(() => expect(screen.getAllByText(/Discovery Test/).length).toBeGreaterThan(0));

    expect(container.querySelectorAll('.feed .feed-row')).toHaveLength(1);
    expect(container.querySelectorAll('.feed .card')).toHaveLength(0);
    // The eyebrow carries DIFFERENT facts than the title: author and the server's
    // own word count, formatted. Repeating the title here doubled every row.
    expect(container.querySelector('.feed-work')?.textContent?.replace(/\s+/g, ' ').trim())
      .toBe('Readers have rated it highly. · by alice · 87% match · 3,000 words');
    expect(container.querySelector('.feed-work')?.textContent).not.toContain('Discovery Test');
    expect(container.textContent).toContain("That's the whole feed for now.");
  });

  it('fetches the feed on mount', async () => {
    render(Discover);
    await waitFor(() => expect(fetchDiscoveryFeed).toHaveBeenCalled());
  });

  it('shows the author handle', async () => {
    render(Discover);
    await waitFor(() => expect(screen.getByText(/by alice/)).toBeInTheDocument());
  });

  it('links to the work page', async () => {
    render(Discover);
    await waitFor(() => expect(screen.getAllByText(/Discovery Test/).length).toBeGreaterThan(0));
    const link = screen.getByRole('link', { name: /Discovery Test/ });
    expect(link).toHaveAttribute('href', '/works/work-1');
  });
});

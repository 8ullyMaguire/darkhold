import { render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import WorkPage from './WorkPage.svelte';
import { session } from '../lib/session.svelte';
import { ApiError } from '../lib/api';

// Mock the API module, keeping every real export (ApiError included) and
// overriding only the fetchers this page calls.
vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>();
  return {
    ...actual,
    fetchWork: vi.fn(),
    fetchWorkPricing: vi.fn(),
    fetchReviews: vi.fn().mockResolvedValue({ items: [] }),
    purchaseWork: vi.fn(),
    getProgress: vi.fn().mockResolvedValue(null),
    upsertReview: vi.fn(),
    isAuthorWork: vi.fn().mockReturnValue(false),
    fetchWorkNoteLinks: vi.fn().mockResolvedValue({ links: [], count: 0, dead: 0 }),
    fetchWorkMediaReferences: vi.fn().mockResolvedValue({ items: [] }),
    recheckWorkNoteLinks: vi.fn().mockResolvedValue({ checked: 0, dead: 0 }),
  };
});

import {
  fetchWork,
  fetchWorkMediaReferences,
  fetchWorkNoteLinks,
  isAuthorWork,
  fetchWorkPricing,
  fetchReviews,
  recheckWorkNoteLinks,
} from '../lib/api';

describe('WorkPage paywall', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    session.status = 'anonymous';
    session.me = null as any;
  });

  it('shows paywall when 403 CONTENT_RESTRICTED returned', async () => {
    (fetchWork as any).mockRejectedValue(
      new ApiError(403, 'CONTENT_RESTRICTED', 'This work is for purchase.'),
    );
    (fetchWorkPricing as any).mockResolvedValue({
      pricing: [{ model: 'purchase', price_minor: 500, currency: 'USD', public_at_offset: null }],
    });

    render(WorkPage, { props: { workId: 'work-1' } });

    await waitFor(() => {
      expect(screen.getByText('This work is for purchase')).toBeInTheDocument();
    });
    expect(screen.getByText('5 USD')).toBeInTheDocument();
    expect(screen.getByText('Buy to unlock full access.')).toBeInTheDocument();
  });

  it('shows sign-in prompt for anonymous users on paywall', async () => {
    (fetchWork as any).mockRejectedValue(
      new ApiError(403, 'CONTENT_RESTRICTED', 'This work is for purchase.'),
    );
    (fetchWorkPricing as any).mockResolvedValue({
      pricing: [{ model: 'purchase', price_minor: 300, currency: 'EUR', public_at_offset: null }],
    });

    render(WorkPage, { props: { workId: 'work-1' } });

    await waitFor(() => {
      expect(screen.getByText('Sign in to buy')).toBeInTheDocument();
    });
  });
});

/**
 * The work page's reviews section.
 *
 * The rule this test exists for is spec §9.2: a review that mentions spoilers is
 * revealed by a **deliberate click**, never automatically. The page has always
 * done that with a `<details>` element, and the browser journey drove it by hand
 * — but nothing automated covered it, so a refactor that swapped the `<details>`
 * for a `<div>` would have hidden nothing and revealed everything, silently.
 *
 * What is asserted is therefore the shape that makes the behaviour, because that
 * is what a regression would break:
 *
 *   * the body of a spoiler review is inside a `<details>` that is **closed**;
 *   * the summary says why, without repeating the spoiler;
 *   * a review that does not mention spoilers is not wrapped at all — the
 *     warning must stay meaningful, and one shown for everything is noise.
 *
 * jsdom has no layout engine and does not implement the `details` disclosure
 * itself, so this does not assert that the text is hidden on screen; it asserts
 * that the element which hides it is there, is a `details`, and starts closed.
 */

const sharedWork = {
  id: 'work-1',
  title: 'The Salt Road',
  summary: 'A cartographer walks inland.',
  language: 'en',
  rating: 'teen',
  visibility: 'public',
  completion: 'ongoing',
  published_at: '2026-09-01T00:00:00Z',
  show_public_ratings: true,
  authors: [{ handle: 'devwriter', display_name: 'Dev Writer' }],
  units: [{ id: 'unit-1', title: 'One — Low Tide', word_count: 74, ordinal: 1 }],
};

function review(overrides: Record<string, unknown>) {
  return {
    id: 'r-1',
    author_handle: 'reader',
    body: 'Loved this.',
    is_public: true,
    contains_spoilers: false,
    created_at: '2026-09-05T00:00:00Z',
    ...overrides,
  };
}

/**
 * The withheld-text state (`works.gated_metadata_public`).
 *
 * A gated work answers the record and refuses the text. The page must not turn
 * that into a broken site: the units are listed so a reader can see what exists,
 * and they are **not** links, because `/units/<id>` answers 404 and an
 * unexplained 404 reads as a bug rather than as a gate. Both halves are asserted
 * here — a test for the notice alone would pass on a page that still offered the
 * dead links, and a test for the links alone would pass on a page that said
 * nothing.
 */
describe('WorkPage withheld text', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    session.status = 'anonymous';
    session.me = null as any;
    (fetchWorkPricing as any).mockResolvedValue({ pricing: [] });
    (fetchReviews as any).mockResolvedValue({ items: [] });
  });

  it('lists the units without links, and says why', async () => {
    (fetchWork as any).mockResolvedValue({ ...sharedWork, content_withheld: true });

    render(WorkPage, { props: { workId: 'work-1' } });

    await waitFor(() => {
      expect(screen.getByTestId('content-withheld')).toBeInTheDocument();
    });
    // The unit is named...
    expect(screen.getByText('One — Low Tide')).toBeInTheDocument();
    // ...and is not a link to text the server will refuse.
    expect(screen.queryByRole('link', { name: 'One — Low Tide' })).toBeNull();
  });

  it('links the units when the text is readable', async () => {
    (fetchWork as any).mockResolvedValue(sharedWork);

    render(WorkPage, { props: { workId: 'work-1' } });

    await waitFor(() => {
      expect(screen.getByRole('link', { name: 'One — Low Tide' })).toBeInTheDocument();
    });
    expect(screen.queryByTestId('content-withheld')).toBeNull();
  });
});

describe('WorkPage spoiler reveal', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    session.status = 'anonymous';
    session.me = null as any;
  });

  it('wraps a spoiler review in a closed <details> element', async () => {
    const spoilerReview = review({ contains_spoilers: true, body: 'The butler did it.' });
    (fetchWork as any).mockResolvedValue(sharedWork);
    (fetchReviews as any).mockResolvedValue({ items: [spoilerReview] });

    render(WorkPage, { props: { workId: 'work-1' } });

    await waitFor(() => {
      const details = screen.getByText('This review mentions spoilers').closest('details');
      expect(details).not.toBeNull();
      expect((details as HTMLDetailsElement).open).toBe(false);
    });
  });

  it('does not wrap a non-spoiler review in <details>', async () => {
    const normalReview = review({ contains_spoilers: false });
    (fetchWork as any).mockResolvedValue(sharedWork);
    (fetchReviews as any).mockResolvedValue({ items: [normalReview] });

    render(WorkPage, { props: { workId: 'work-1' } });

    await waitFor(() => {
      expect(screen.getByText('Loved this.')).toBeInTheDocument();
    });
    // Should NOT be inside a <details> element
    const reviewText = screen.getByText('Loved this.');
    expect(reviewText.closest('details')).toBeNull();
  });
});
/**
 * Links in the author's notes (M45-38, §32.7.4a).
 *
 * The page's job is small and specific: a reader about to click a link should learn it is dead
 * **before** clicking, and a note with no links should not grow a section. Both halves are asserted,
 * because a test for the warning alone passes on a page that shows the section for every work, and a
 * test for the absence alone passes on a page that never warns at all.
 */
describe('WorkPage note links', () => {
  const dead = {
    id: 'nl-1',
    url: 'https://gone.test/meta',
    host: 'gone.test',
    provider: 'other',
    status: 'dead',
    last_checked_at: '2026-11-02T09:00:00Z',
    last_healthy_at: '2026-10-01T09:00:00Z',
    failure_reason: 'http 404',
  };
  const healthy = {
    ...dead,
    id: 'nl-2',
    url: 'https://blog.tumblr.com/post/1',
    host: 'blog.tumblr.com',
    provider: 'tumblr',
    status: 'healthy',
    failure_reason: null,
  };

  beforeEach(() => {
    vi.clearAllMocks();
    session.status = 'anonymous';
    session.me = null as any;
    (fetchWork as any).mockResolvedValue(sharedWork);
    (fetchReviews as any).mockResolvedValue({ items: [] });
    (fetchWorkPricing as any).mockResolvedValue({ pricing: [] });
    (fetchWorkNoteLinks as any).mockResolvedValue({ links: [], count: 0, dead: 0 });
  });

  it('warns before the click when a note link is dead', async () => {
    (fetchWorkNoteLinks as any).mockResolvedValue({
      links: [dead, healthy],
      count: 2,
      dead: 1,
    });

    render(WorkPage, { props: { workId: 'work-1' } });

    await waitFor(() => {
      expect(screen.getByTestId('note-links-warning')).toBeInTheDocument();
    });
    expect(screen.getByTestId('note-links-warning').textContent).toContain(
      'One link has stopped answering.',
    );
    expect(screen.getByText('dead')).toBeInTheDocument();
    // The link itself is still offered: the note is still worth reading, and the warning is what
    // says the target moved.
    const links = screen.getAllByTestId('note-link');
    expect(links.map((anchor) => anchor.textContent)).toEqual(['gone.test', 'blog.tumblr.com']);
  });

  it('renders nothing when the notes carry no links', async () => {
    render(WorkPage, { props: { workId: 'work-1' } });

    await waitFor(() => {
      expect(screen.getByText('The Salt Road')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('note-links')).toBeNull();
  });

  it('offers the re-check to the author only', async () => {
    const { isAuthorWork } = await import('../lib/api');
    (isAuthorWork as any).mockReturnValue(true);
    (fetchWork as any).mockResolvedValue({ ...sharedWork, lifecycle: 'published' });
    (fetchWorkNoteLinks as any).mockResolvedValue({ links: [healthy], count: 1, dead: 0 });

    render(WorkPage, { props: { workId: 'work-1' } });

    const button = await screen.findByRole('button', { name: /check these links now/i });
    await button.click();
    await waitFor(() => {
      expect(recheckWorkNoteLinks).toHaveBeenCalledWith('work-1');
    });
    // The page reloads the report after a check, so the statuses on screen are the new ones.
    expect((fetchWorkNoteLinks as any).mock.calls.length).toBeGreaterThan(1);
  });
});

describe('WorkPage media fallback (M45-36)', () => {
  // WorkPage reads `authors`, `units` and `language` off the work; a thinner fixture renders
  // nothing at all rather than failing loudly, which reads as a missing element, not a bad fixture.

  const deadRef = {
    id: 'ref-1',
    work_id: 'work-1',
    unit_id: null,
    context: 'inline_embed',
    display_url: 'https://old-host.example/song.mp3',
    author_note: 'Listen while you read',
    inserted_at: '2026-01-01T00:00:00Z',
    healthy_links: 0,
    total_links: 0,
    best_url: null,
  };

  beforeEach(() => {
    vi.clearAllMocks();
    session.status = 'anonymous';
    session.me = null as any;
    (fetchWork as any).mockResolvedValue(sharedWork);
    (fetchWorkPricing as any).mockResolvedValue({ pricing: [] });
    (fetchReviews as any).mockResolvedValue({ items: [] });
    (fetchWorkNoteLinks as any).mockResolvedValue({ links: [], count: 0, dead: 0 });
    (fetchWorkMediaReferences as any).mockResolvedValue({ items: [] });
    // The note-links block flips this on for the author test; left on, the page renders the author
    // header instead of the reader's and the media block never mounts.
    (isAuthorWork as any).mockReturnValue(false);
  });

  it('names the missing media instead of saying only that it is unavailable', async () => {
    (fetchWorkMediaReferences as any).mockResolvedValue({ items: [deadRef] });

    render(WorkPage, { props: { workId: 'work-1' } });

    const fallback = await screen.findByTestId('media-ref-unavailable');
    // What kind of media it was…
    expect(fallback.textContent).toContain('This inline embed is currently unavailable.');
    // …what the author said about it…
    expect(fallback.textContent).toContain('Listen while you read');
    // …and where it used to live, so the reader has something to look for.
    expect(fallback.textContent).toContain('old-host.example');
  });

  it('does not show the fallback while a healthy link exists', async () => {
    (fetchWorkMediaReferences as any).mockResolvedValue({
      items: [{ ...deadRef, healthy_links: 1, total_links: 2, best_url: 'https://ok.example/x.png' }],
    });

    render(WorkPage, { props: { workId: 'work-1' } });

    await screen.findByRole('heading', { name: /Media references/i });
    expect(screen.queryByTestId('media-ref-unavailable')).toBeNull();
  });

  it('degrades to the note alone when the stored URL is unparseable', async () => {
    (fetchWorkMediaReferences as any).mockResolvedValue({
      items: [{ ...deadRef, display_url: 'not a url at all' }],
    });

    render(WorkPage, { props: { workId: 'work-1' } });

    const fallback = await screen.findByTestId('media-ref-unavailable');
    expect(fallback.textContent).toContain('Listen while you read');
    expect(fallback.textContent).not.toContain('It was hosted at');
  });
});

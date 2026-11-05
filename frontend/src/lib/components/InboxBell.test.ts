/**
 * InboxBell: the number, and where it comes from.
 *
 * The bell reads two endpoints — a count and a stream — and the rules worth pinning are about their
 * relationship:
 *
 *   1. the first number comes from the endpoint, not from an increment;
 *   2. a stream event re-reads the count instead of adding one, because a count maintained by increments drifts
 *      the moment one event is missed, and this is the one number every product shows;
 *   3. an unreachable inbox keeps the last number rather than claiming zero;
 *   4. clicking a bell with unread items marks them read and zeroes the badge.
 */
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import InboxBell from './InboxBell.svelte';

type Listener = (event: Event) => void;

class FakeSource {
  static last: FakeSource | null = null;
  listeners = new Map<string, Listener[]>();
  closed = false;
  constructor(public url: string) {
    FakeSource.last = this;
  }
  addEventListener(type: string, fn: Listener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), fn]);
  }
  emit(type: string) {
    for (const fn of this.listeners.get(type) ?? []) fn(new Event(type));
  }
  close() {
    this.closed = true;
  }
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
}

describe('InboxBell', () => {
  beforeEach(() => {
    vi.stubGlobal('EventSource', FakeSource as unknown as typeof EventSource);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    FakeSource.last = null;
  });

  it('shows the count the endpoint reports, and subscribes to the stream', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ total: 3, by_site: { darkhold: 3 } }));
    vi.stubGlobal('fetch', fetchMock);

    render(InboxBell);

    await waitFor(() => expect(screen.getByText('3')).toBeInTheDocument());
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/inbox/unread-count', expect.anything());
    expect(FakeSource.last?.url).toBe('/api/v1/inbox/stream');
  });

  it('re-reads the count when the stream says something happened', async () => {
    let total = 1;
    const fetchMock = vi.fn(async () => jsonResponse({ total, by_site: {} }));
    vi.stubGlobal('fetch', fetchMock);

    render(InboxBell);
    await waitFor(() => expect(screen.getByText('1')).toBeInTheDocument());

    // A second event arrives upstream: the endpoint is the truth, so the badge follows the endpoint.
    total = 2;
    FakeSource.last?.emit('inbox');
    await waitFor(() => expect(screen.getByText('2')).toBeInTheDocument());
    expect(fetchMock.mock.calls.length).toBeGreaterThan(1);
  });

  it('keeps the last number when the inbox is unreachable', async () => {
    let unavailable = false;
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => jsonResponse(unavailable ? { total: 0, unavailable: true } : { total: 4, by_site: {} }))
    );

    render(InboxBell);
    await waitFor(() => expect(screen.getByText('4')).toBeInTheDocument());

    unavailable = true;
    FakeSource.last?.emit('inbox');
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.getByText('4')).toBeInTheDocument();
  });

  it('marks everything read and empties the badge when clicked', async () => {
    const calls: { url: string; method?: string }[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, init?: RequestInit) => {
        calls.push({ url, method: init?.method });
        if (url === '/api/v1/inbox/read') return jsonResponse({ marked: 5 });
        return jsonResponse({ total: 5, by_site: {} });
      })
    );

    render(InboxBell);
    await waitFor(() => expect(screen.getByText('5')).toBeInTheDocument());

    await fireEvent.click(screen.getByRole('button'));
    await waitFor(() => expect(screen.queryByText('5')).not.toBeInTheDocument());
    expect(calls.some((call) => call.url === '/api/v1/inbox/read' && call.method === 'POST')).toBe(true);
  });
});

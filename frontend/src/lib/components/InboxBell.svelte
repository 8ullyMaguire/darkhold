<script lang="ts">
  /**
   * The bell: one number, live, for the person who is signed in.
   *
   * The estate has one inbox (W4), and this is the first product to show it. Two sources, deliberately:
   *
   *  - `/api/v1/inbox/unread-count` for the truth on mount and after any doubt;
   *  - `/api/v1/inbox/stream` (server-sent events) for the number going up while the page is open.
   *
   * The stream is not the source of truth and never sets the count to a computed value: an event means "ask
   * again", because a count maintained by increments drifts the moment one event is missed — and the one number
   * every product shows is the last place to keep a guess.
   *
   * Clicking is the answer the person meant by "I have seen these": mark all read, then zero.
   */
  import { onMount } from 'svelte';

  // No initial prop: the number comes from the endpoint, and a prop would be a second source of truth that is
  // wrong the moment the page is a minute old.
  let count = $state(0);
  let open = $state(false);
  let live = $state(false);

  async function refresh() {
    try {
      const response = await fetch('/api/v1/inbox/unread-count', { headers: { accept: 'application/json' } });
      if (!response.ok) return;
      const body = await response.json();
      // `unavailable` means the inbox could not be reached; keep the last number rather than claiming zero.
      if (body?.unavailable) return;
      count = Number(body?.total ?? 0);
    } catch {
      // A bell that cannot reach the inbox keeps showing what it knows.
    }
  }

  async function markAllRead() {
    try {
      await fetch('/api/v1/inbox/read', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ all: true })
      });
      count = 0;
      open = false;
    } catch {
      /* the next refresh will tell the truth */
    }
  }

  onMount(() => {
    refresh();
    if (typeof EventSource === 'undefined') return;
    const source = new EventSource('/api/v1/inbox/stream');
    source.addEventListener('open', () => (live = true));
    source.addEventListener('inbox', () => {
      live = true;
      // Re-read rather than increment: the stream says something happened, the endpoint says how many.
      refresh();
    });
    // EventSource retries by itself; when it gives up entirely, fall back to a slow poll so the number is never
    // stale by more than a minute.
    source.addEventListener('error', () => (live = false));
    const poll = setInterval(refresh, 60_000);
    return () => {
      source.close();
      clearInterval(poll);
    };
  });
</script>

<div class="bell">
  <button
    type="button"
    class="bell-button"
    aria-expanded={open}
    aria-label={count > 0 ? `Inbox, ${count} unread` : 'Inbox, nothing unread'}
    onclick={() => (count > 0 ? markAllRead() : (open = !open))}
  >
    <span class="glyph" aria-hidden="true">✉</span>
    {#if count > 0}<span class="count">{count > 99 ? '99+' : count}</span>{/if}
    {#if live}<span class="live" title="live"></span>{/if}
  </button>
</div>

<style>
  .bell {
    position: relative;
    display: inline-flex;
  }
  .bell-button {
    position: relative;
    background: none;
    border: 0;
    padding: 0.35rem 0.5rem;
    cursor: pointer;
    color: inherit;
    font: inherit;
    border-radius: 0.375rem;
  }
  .bell-button:hover,
  .bell-button:focus-visible {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }
  .glyph {
    font-size: 1.05rem;
    line-height: 1;
  }
  .count {
    position: absolute;
    top: 0;
    right: 0;
    min-width: 1.05rem;
    padding: 0 0.25rem;
    border-radius: 999px;
    background: var(--accent, #b4453c);
    color: var(--color-accent-contrast, #fff);
    font-size: 0.7rem;
    line-height: 1.05rem;
    text-align: center;
  }
  .live {
    position: absolute;
    bottom: 2px;
    left: 2px;
    width: 0.35rem;
    height: 0.35rem;
    border-radius: 999px;
    background: #3f9d5a;
  }
</style>

<script lang="ts">
  /**
   * The bell: one number, live, for the person who is signed in.
   *
   * The transport moved to `@polaris/inbox-client` in W9 §3.5. Three sites show the same inbox, so the rules that keep the number honest
   * live in one package instead of in this file — and they are tested there, including the case this component used to get wrong.
   *
   * What stays here is what is darkhold's: the accessible name, the glyph, the badge, and the click.
   */
  import { onMount } from 'svelte';
  import { createInboxClient } from '@polaris/inbox-client';

  // No initial prop: the number comes from the endpoint, and a prop would be a second source of truth that is
  // wrong the moment the page is a minute old.
  let count = $state(0);
  let open = $state(false);
  let live = $state(false);

  // Created in onMount, not at module scope. Measured: at module scope the client captured `EventSource` before the test had stubbed it,
  // so the bell silently became poll-only — the same class of bug as the package's own poll-path defect, in the product this time.
  let inbox: ReturnType<typeof createInboxClient> | null = null;

  async function markAllRead() {
    await inbox?.markAllRead();
    open = false;
  }

  onMount(() => {
    inbox = createInboxClient();
    // The client owns refresh, the stream and the poll floor; this component owns the two numbers on screen.
    const unsubscribe = inbox.subscribe((state) => {
      count = state.total ?? 0;
      live = state.live;
    });
    const stop = inbox.start();
    return () => {
      unsubscribe();
      stop();
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

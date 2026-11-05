<script lang="ts">
  /**
   * The help/docs search, opened with Ctrl+K (or Cmd+K on a Mac) from
   * anywhere on the site, and linked from the docs pages.
   *
   * Search runs entirely client-side over the bundled docs registry
   * (`../lib/docs`): no server round-trip, works offline, and the same
   * scoring the docs index page uses.
   */
  import Dialog from './Dialog.svelte';
  import { DOCS, searchDocs, type DocEntry } from '../docs';

  let { open = false, onclose = () => {} }: { open?: boolean; onclose?: () => void } = $props();

  let query = $state('');
  let results = $state<readonly DocEntry[]>([]);
  let highlighted = $state(0);

  /**
   * Estate results, from the index behind `/api/v1/estate/search` — a server-side proxy to `polaris-search`, so
   * the browser never learns the service's address and the palette needs no second credential.
   *
   * Debounced because a palette asks on every keystroke and the index is a database: 180 ms is long enough to
   * collapse a fast typist and short enough not to feel laggy. A failed fetch empties the list rather than
   * showing an error — nobody wants a network banner inside a completion box.
   */
  type EstateHit = { site: string; kind: string; doc_id: string; title: string };
  let estate = $state<readonly EstateHit[]>([]);

  $effect(() => {
    const q = query.trim();
    if (q.length < 2) {
      estate = [];
      return;
    }
    const controller = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const res = await fetch(`/api/v1/estate/search?q=${encodeURIComponent(q)}&limit=5`, {
          signal: controller.signal,
          headers: { accept: 'application/json' }
        });
        if (!res.ok) {
          estate = [];
          return;
        }
        const body = await res.json();
        estate = (body?.data ?? []).map((h: EstateHit) => ({
          site: h.site,
          kind: h.kind,
          doc_id: h.doc_id,
          title: h.title
        }));
      } catch {
        estate = [];
      }
    }, 180);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  });

  function openEstate(hit: EstateHit) {
    query = '';
    onclose();
    history.pushState({}, '', `/works/${encodeURIComponent(hit.doc_id)}`);
    dispatchEvent(new PopStateEvent('popstate'));
  }

  /** All docs when the box is empty (a browsable index), matches otherwise. */
  $effect(() => {
    results = query.trim().length === 0 ? DOCS.slice(0, 8) : searchDocs(query, 8);
    highlighted = 0;
  });

  function go(slug: string) {
    query = '';
    onclose();
    history.pushState({}, '', `/docs/${slug}`);
    dispatchEvent(new PopStateEvent('popstate'));
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      highlighted = (highlighted + 1) % results.length;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      highlighted = (highlighted - 1 + results.length) % results.length;
    } else if (e.key === 'Enter' && results[highlighted]) {
      e.preventDefault();
      go(results[highlighted].slug);
    }
  }
</script>

<Dialog {open} title="Search the estate" description="Help pages, and works from every site in the index" {onclose}>
  <div class="docs-search">
    <input
      type="search"
      placeholder="Search help…"
      bind:value={query}
      onkeydown={onKeydown}
      aria-label="Search help pages"
      autocomplete="off"
    />
    {#if results.length > 0}
      <ul role="listbox" aria-label="Help pages">
        {#each results as doc, i (doc.slug)}
          <li class:result={i === highlighted}>
            <button
              role="option"
              aria-selected={i === highlighted}
              onclick={() => go(doc.slug)}
              onmousemove={() => (highlighted = i)}
            >
              <span class="title">{doc.title}</span>
              <span class="summary">{doc.summary}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    {#if estate.length > 0}
      <div class="estate">
        <p class="estate-heading">In the estate</p>
        <ul role="listbox" aria-label="Estate results">
          {#each estate as hit (hit.doc_id)}
            <li>
              <button onclick={() => openEstate(hit)}>
                <span class="title">{hit.title || '(untitled)'}</span>
                <span class="summary">{hit.site} · {hit.kind}</span>
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
    {#if results.length === 0 && estate.length === 0}
      <p class="no-results">
        Nothing matched. Try a shorter word, or read
        <a href="/docs/getting-started" onclick={() => go('getting-started')}>Getting started</a>.
      </p>
    {:else}
      <p class="hint">↑ ↓ to move · Enter to open · Esc to close</p>
    {/if}
  </div>
</Dialog>

<style>
  .docs-search input {
    width: 100%;
    box-sizing: border-box;
    padding: 0.6rem 0.75rem;
    font: inherit;
    border: 1px solid var(--border, #ccc);
    border-radius: 6px;
    margin-bottom: 0.75rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 50vh;
    overflow-y: auto;
  }
  li button {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    width: 100%;
    text-align: left;
    padding: 0.5rem 0.6rem;
    border: 0;
    border-radius: 6px;
    background: transparent;
    font: inherit;
    cursor: pointer;
  }
  li.result button,
  li button:hover {
    background: var(--surface-2, #eee);
  }
  .title {
    font-weight: 600;
  }
  .summary {
    color: var(--text-2, #666);
    font-size: 0.85rem;
  }
  .estate-heading {
    color: var(--text-2, #666);
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin: 0.75rem 0 0.25rem;
  }
  .no-results {
    color: var(--text-2, #666);
    padding: 0.5rem 0;
  }
  .hint {
    color: var(--text-2, #666);
    font-size: 0.75rem;
    margin: 0.5rem 0 0;
    text-align: center;
  }
</style>

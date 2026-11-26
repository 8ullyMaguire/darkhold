<script lang="ts">
  /**
   * Facet counts under the current filter set (M71-11's reader half).
   *
   * Two claims this component refuses to blur: a pending count is `—` and not `0`, and the numbers describe the
   * catalogue under the word/rating window rather than the page of hits the reader is looking at. Both facts are in the
   * search response; dropping either would make a memoised scan look like a fresh, exhaustive one.
   */
  export interface FacetBucketView {
    key: string;
    label: string;
    count: number;
  }

  export interface FacetGroupView {
    facet: string;
    buckets: FacetBucketView[];
    known: number;
    unknown: number;
    when_unknown?: string | null;
  }

  interface Props {
    facets?: FacetGroupView[];
    /** True while a new search is in flight; the previous counts are still displayed but are not current. */
    pending?: boolean;
    facetsOver?: string;
    /** Age of the memoised scan, in milliseconds. */
    ageMs?: number;
    /** Facets the query language can express. Others stay visible as counts but are not fake buttons. */
    selectable?: string[];
    onselect?: (facet: string, key: string) => void;
  }

  let {
    facets = [],
    pending = false,
    facetsOver = '',
    ageMs = 0,
    selectable,
    onselect,
  }: Props = $props();

  function canSelect(facet: string): boolean {
    return selectable ? selectable.includes(facet) : true;
  }

  function labelFor(facet: string): string {
    return facet.replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase());
  }

  function fmt(value: number): string {
    return value.toLocaleString('en-US');
  }

  function age(): string {
    return ageMs >= 1000 ? `${(ageMs / 1000).toFixed(1)}s ago` : 'now';
  }

  /** M71-13: the fandom facet leads with the one aggregate the API already publishes, not a typed-in copy claim. */
  function coverage(group: FacetGroupView): string | null {
    if (group.facet !== 'fandom') return null;
    const total = group.known + group.unknown;
    if (total <= 0) return 'Nothing to measure against';
    const percent = Math.round((group.known / total) * 100);
    return `${fmt(group.known)} of ${fmt(total)} works are tagged (${percent}%)`;
  }
</script>

<aside class="facets" aria-label="Facet counts" aria-busy={pending}>
  {#if facetsOver}
    <p class="over">{facetsOver}</p>
  {/if}
  <p class="age">Counts: {age()}</p>

  {#each facets as group (group.facet)}
    <section class="group" role="group" aria-label={labelFor(group.facet)} data-facet={group.facet}>
      <h3>{labelFor(group.facet)}</h3>
      {#if coverage(group)}
        <p class="coverage">{coverage(group)}</p>
      {/if}
      <ul>
        {#each group.buckets as bucket (bucket.key)}
          <li>
            {#if canSelect(group.facet)}
              <button type="button" onclick={() => onselect?.(group.facet, bucket.key)}>
                <span class="label">{bucket.label}</span>
                <span class="count" aria-label={pending ? 'Count pending' : `${bucket.count} works`}>
                  {pending ? '—' : fmt(bucket.count)}
                </span>
              </button>
            {:else}
              <span class="value">
                <span class="label">{bucket.label}</span>
                <span class="count" aria-label={pending ? 'Count pending' : `${bucket.count} works`}>
                  {pending ? '—' : fmt(bucket.count)}
                </span>
              </span>
            {/if}
          </li>
        {/each}
      </ul>
      {#if group.when_unknown}
        <p class="unknown">{group.when_unknown}</p>
      {/if}
    </section>
  {/each}
</aside>

<style>
  .facets {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    font-family: var(--font-interface);
    color: var(--text-primary);
  }
  .over,
  .age,
  .coverage,
  .unknown {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }
  .group {
    border-top: 1px solid var(--border-subtle, rgba(128, 128, 128, 0.35));
    padding-top: var(--space-2);
  }
  h3 {
    margin: 0 0 var(--space-1);
    font-size: var(--text-base);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  button {
    display: flex;
    width: 100%;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  button:hover,
  button:focus-visible {
    background: var(--surface-hover, rgba(128, 128, 128, 0.12));
  }
  .value {
    display: flex;
    width: 100%;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    color: var(--text-secondary);
  }
  .count {
    font-family: var(--font-mono);
    color: var(--text-secondary);
  }
</style>

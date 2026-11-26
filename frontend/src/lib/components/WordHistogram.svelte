<script lang="ts">
  /**
   * The word-count distribution under the current filter set (M71-12's reader half).
   *
   * The server owns the quantile edges and the shares. This component only draws them and hands the inclusive edges
   * back to the search page, which already knows how to turn a word range into a `words:` query term. Recomputing
   * buckets here would describe a different population; adding a third range input would give the query two owners.
   */
  export interface WordBucketView {
    index: number;
    low: number;
    high: number;
    count: number;
    share: number;
  }

  export interface WordHistogramView {
    unit: string;
    buckets: WordBucketView[];
    measured: number;
    unknown: number;
    total: number;
    when_unknown?: string | null;
  }

  interface Props {
    histogram: WordHistogramView;
    pending?: boolean;
    onselect?: (low: number, high: number) => void;
  }

  let { histogram, pending = false, onselect }: Props = $props();

  function fmt(value: number): string {
    return value.toLocaleString('en-US');
  }

  function label(bucket: WordBucketView): string {
    if (bucket.high >= 9_000_000_000_000_000_000) return `Over ${fmt(bucket.low)} words`;
    if (bucket.low === 0) return `Under ${fmt(bucket.high + 1)} words`;
    return `${fmt(bucket.low)} – ${fmt(bucket.high)} words`;
  }
</script>

<section class="histogram" aria-label="Word count distribution" aria-busy={pending}>
  <h3>Length distribution</h3>
  <p class="pen">
    {histogram.measured.toLocaleString('en-US')} measured of {histogram.total.toLocaleString('en-US')}
  </p>
  {#if histogram.when_unknown}
    <p class="unknown">{histogram.when_unknown}</p>
  {/if}
  <div class="bars">
    {#each histogram.buckets as bucket (bucket.index)}
      <button
        type="button"
        class="bar"
        style:height={`${Math.round(bucket.share * 100)}%`}
        aria-label={`${label(bucket)}: ${pending ? 'count pending' : `${bucket.count} works`}`}
        data-count={bucket.count}
        onclick={() => onselect?.(bucket.low, bucket.high)}
      >
        <span class="caption">{label(bucket)}</span>
        <span class="count">{pending ? '—' : fmt(bucket.count)}</span>
      </button>
    {/each}
  </div>
</section>

<style>
  .histogram {
    font-family: var(--font-interface);
    color: var(--text-primary);
  }
  h3 {
    margin: 0 0 var(--space-1);
    font-size: var(--text-base);
  }
  .pen,
  .unknown {
    margin: 0 0 var(--space-2);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }
  .bars {
    display: flex;
    align-items: flex-end;
    gap: var(--space-1);
    height: 9rem;
  }
  .bar {
    display: flex;
    flex: 1;
    flex-direction: column;
    justify-content: flex-end;
    min-height: 1.5rem;
    padding: var(--space-1);
    border: 1px solid var(--border-subtle, rgba(128, 128, 128, 0.35));
    background: var(--surface-raised, transparent);
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .bar:hover,
  .bar:focus-visible {
    border-color: currentColor;
  }
  .caption,
  .count {
    font-size: var(--text-xs, var(--text-sm));
  }
  .count {
    font-family: var(--font-mono);
  }

  /* Ten quantile columns cannot fit a phone. The server still owns the edges and
     the click still applies them; only the drawing changes to a stacked list. */
  @media (max-width: 48rem) {
    .bars {
      display: grid;
      grid-template-columns: minmax(0, 1fr);
      height: auto;
    }
    .bar {
      flex: none;
      flex-direction: row;
      align-items: center;
      justify-content: space-between;
      width: 100%;
      height: auto !important;
      min-height: 44px;
      padding: var(--space-2) var(--space-3);
    }
    .caption {
      white-space: normal;
      overflow-wrap: anywhere;
      text-align: left;
    }
  }
</style>

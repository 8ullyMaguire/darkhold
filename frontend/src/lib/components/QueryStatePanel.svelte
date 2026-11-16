<script lang="ts">
  /**
   * The query string and the facet controls are one state (M71-15).
   *
   * The server owns both the canonical string and the meaning of each chip. This panel never rebuilds either: it shows
   * what FilterState sent, offers an explicit action to adopt the string, and gives a remove control only to chips the
   * server says are removable. A pinned chip prints its reason instead of offering a button that could not work.
   */
  export interface QueryChipView {
    kind: string;
    value: string;
    label: string;
    editable_in_query: boolean;
    removable: boolean;
    reason?: string | null;
  }

  export interface SearchStateView {
    query: string;
    chips: QueryChipView[];
    unexpressed_note?: string | null;
  }

  interface Props {
    state: SearchStateView;
    onremove?: (kind: string, value: string) => void;
    onusequery?: (query: string) => void;
  }

  let { state, onremove, onusequery }: Props = $props();
</script>

<section class="query-state" aria-label="Search state">
  <div class="canonical">
    <code>{state.query || 'no filters'}</code>
    <button type="button" onclick={() => onusequery?.(state.query)}>Use this query</button>
  </div>

  {#if state.chips.length > 0}
    <ul class="chips">
      {#each state.chips as chip (chip.kind + ':' + chip.value)}
        <li class:pinned={!chip.removable}>
          <span class="label">{chip.label}</span>
          {#if chip.reason}
            <span class="reason">{chip.reason}</span>
          {/if}
          {#if chip.removable}
            <button
              type="button"
              aria-label={`Remove ${chip.label}`}
              onclick={() => onremove?.(chip.kind, chip.value)}
            >
              &times;
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if state.unexpressed_note}
    <p class="note">{state.unexpressed_note}</p>
  {/if}
</section>

<style>
  .query-state {
    font-family: var(--font-interface);
    color: var(--text-primary);
  }
  .canonical {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }
  code {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }
  button {
    border: 1px solid var(--border-subtle, rgba(128, 128, 128, 0.35));
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .chips li {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border-subtle, rgba(128, 128, 128, 0.35));
    font-size: var(--text-sm);
  }
  .chips li.pinned {
    border-style: dashed;
  }
  .reason,
  .note {
    font-size: var(--text-xs, var(--text-sm));
    color: var(--text-secondary);
  }
  .note {
    margin: var(--space-2) 0 0;
  }
</style>

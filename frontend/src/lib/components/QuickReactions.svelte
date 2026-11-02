<script lang="ts">
  /**
   * The labelled one-click bar (spec §9.4, M47-12).
   *
   * A different surface from `ReactionBar` (§35.1): that one is a single typed vote the reader
   * may change, this one is several labels the reader may hold at once, and neither replaces the
   * other. A reader may vote "insightful" and react "made me cry" to the same work.
   *
   * The vocabulary comes from the server, not from a copy here: the same list is what the write
   * route validates against, so a label the bar offers can never be one the server refuses.
   */
  import {
    addQuickReaction,
    fetchQuickReactions,
    removeQuickReaction,
    type QuickReactionsView,
  } from '../api';
  import { session } from '../session.svelte.ts';

  interface Props {
    workId: string;
  }

  let { workId }: Props = $props();

  let view = $state<QuickReactionsView | null>(null);
  let busy = $state(false);

  $effect(() => {
    void load();
  });

  async function load() {
    try {
      view = await fetchQuickReactions(workId);
    } catch {
      // A work page must not fail because a reaction tally did: the bar simply is not there.
      view = null;
    }
  }

  function countFor(label: string): number {
    return view?.items.find((item) => item.label === label)?.count ?? 0;
  }

  function held(label: string): boolean {
    return view?.mine.includes(label) ?? false;
  }

  async function toggle(label: string) {
    if (!view || busy) return;
    if (!session.isSignedIn) {
      // Nothing to optimistically update and no way to send it: the bar stays read-only and the
      // counts keep reflecting the instance rather than a press that did nothing.
      return;
    }
    busy = true;
    try {
      if (held(label)) await removeQuickReaction(workId, label);
      else await addQuickReaction(workId, label);
      // Re-read rather than patch locally: the server owns the count, and a local increment on a
      // reaction that was already there would double it.
      view = await fetchQuickReactions(workId);
    } catch {
      // Leave the last known tally on screen. A reaction that did not land is not worth an alert
      // over the work the reader is reading.
    } finally {
      busy = false;
    }
  }
</script>

{#if view?.enabled}
  <div class="quick-reactions" role="group" aria-label="Quick reactions">
    {#each view.vocabulary as [slug, label] (slug)}
      <button
        type="button"
        class="reaction"
        class:held={held(slug)}
        aria-pressed={held(slug)}
        disabled={busy || !session.isSignedIn}
        title={session.isSignedIn ? label : `${label} — sign in to react`}
        onclick={() => toggle(slug)}
      >
        <span class="label">{label}</span>
        {#if countFor(slug) > 0}
          <span class="count">{countFor(slug)}</span>
        {/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .quick-reactions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin: 1rem 0;
  }
  .reaction {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    background: var(--surface-2);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0.3rem 0.75rem;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .reaction:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .reaction:disabled {
    opacity: 0.7;
    cursor: default;
  }
  .reaction.held {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }
  .count {
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
</style>

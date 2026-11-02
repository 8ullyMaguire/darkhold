<script lang="ts">
  /**
   * §9.7.9's reading-goal ring.
   *
   * Draws an arc for `current / target` and the numbers underneath. Three rules from the spec shape
   * it rather than decorate it:
   *
   * - **Private and opt-in**: it renders nothing at all until the reader has set a goal, and the
   *   empty state is one quiet line pointing at the setting, not a prompt.
   * - **Neutral, never failure**: no red, no crossed-out state, no "you missed" and no countdown.
   *   A reader below the target sees a partly-filled ring and two numbers.
   * - **No streak**: there is no "days in a row" figure here, because a missed day costs nothing
   *   (§9.7.1) and a number that resets is the anxiety the spec forbids.
   */
  import { fetchReadingGoal, type ReadingGoalProgress } from '../api';

  interface Props {
    /** Injected for tests; the component fetches on mount otherwise. */
    load?: () => Promise<ReadingGoalProgress>;
  }

  let { load = () => fetchReadingGoal() }: Props = $props();

  let view = $state<ReadingGoalProgress | null>(null);
  let failed = $state(false);

  $effect(() => {
    void refresh();
  });

  async function refresh() {
    try {
      view = await load();
    } catch {
      // The library page must not fail over a ring: a goal that cannot be read is simply absent.
      failed = true;
      view = null;
    }
  }

  const NOUN: Record<string, { one: string; many: string }> = {
    chapters: { one: 'chapter', many: 'chapters' },
    words: { one: 'word', many: 'words' },
    works: { one: 'work', many: 'works' },
  };

  function noun(metric: string, count: number): string {
    const forms = NOUN[metric] ?? { one: metric, many: metric };
    return count === 1 ? forms.one : forms.many;
  }

  const RADIUS = 26;
  const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

  let fraction = $derived.by(() => {
    const goal = view?.goal;
    const current = view?.progress?.current ?? 0;
    if (!goal || goal.target <= 0) return 0;
    // Clamped: a count past the target is a full ring, not an arc that wraps onto itself.
    return Math.min(1, current / goal.target);
  });

  let dash = $derived(CIRCUMFERENCE * (1 - fraction));
</script>

{#if view?.goal && view.progress}
  <div class="reading-goal" aria-label="Reading goal">
    <svg viewBox="0 0 64 64" class="ring" role="img" aria-hidden="true">
      <circle class="track" cx="32" cy="32" r={RADIUS} />
      <circle
        class="fill"
        cx="32"
        cy="32"
        r={RADIUS}
        stroke-dasharray={CIRCUMFERENCE}
        stroke-dashoffset={dash}
        transform="rotate(-90 32 32)"
      />
    </svg>
    <div class="text">
      <p class="counts">
        <strong>{view.progress.current}</strong>
        <span class="of"> / {view.goal.target}</span>
        <span class="noun">{noun(view.goal.metric, view.goal.target)}</span>
      </p>
      <p class="window">{view.goal.period === 'daily' ? 'today' : 'this week'}</p>
      {#if view.goal.metric === 'words'}
        <p class="estimate">approximate — counted from chapter word counts</p>
      {/if}
    </div>
  </div>
{:else if !failed}
  <p class="no-goal">
    <a href="/settings">Set a reading goal</a>
  </p>
{/if}

<style>
  .reading-goal {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }
  .ring {
    width: 56px;
    height: 56px;
  }
  .track {
    fill: none;
    stroke: var(--border);
    stroke-width: 6;
  }
  .fill {
    fill: none;
    stroke: var(--accent);
    stroke-width: 6;
    stroke-linecap: round;
    transition: stroke-dashoffset 240ms ease;
  }
  .text {
    display: flex;
    flex-direction: column;
  }
  .counts {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .of {
    color: var(--text-muted);
  }
  .noun {
    margin-left: 0.25rem;
    color: var(--text-muted);
  }
  .window {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-muted);
  }
  .estimate {
    margin: 0;
    font-size: 0.72rem;
    color: var(--text-muted);
  }
  .no-goal {
    margin: 0;
    font-size: 0.85rem;
  }
</style>

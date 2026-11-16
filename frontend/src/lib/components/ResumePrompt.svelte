<script lang="ts">
  /**
   * "Resume where you left off", shown on a work page to a signed-in reader (M71-09).
   *
   * The domain makes the honesty decision: `resume.sentence` names the state and `resume.display` is either a real
   * percentage or an em dash. This component prints both. It never formats `position_permille` into a percentage,
   * because a template that computes a number cannot tell an unmeasured place from a measured zero.
   *
   * §9.3: when two places disagree the reader gets a choice, never a silent winner. The raw positions still travel
   * beside the decision, so they are the links; the choice is not a second ranking of them.
   */
  import type { ProgressResolution, ResumeView, ServerPosition } from '../api';

  interface Props {
    workId: string;
    resolution: ProgressResolution;
    /** The first unit, used when a position names no unit. */
    firstUnitId: string | null;
    /** Sent by the current server; absent only when talking to an older one. */
    resume?: ResumeView;
  }

  let { workId, resolution, firstUnitId, resume }: Props = $props();

  function href(unitId: string | null, position: ServerPosition): string {
    const target = unitId ?? firstUnitId;
    if (!target) return `/works/${workId}`;
    return `/works/${workId}/units/${target}#p-${position.position_permille}`;
  }

  function choices(): ServerPosition[] {
    if (resolution.kind === 'ask_the_reader') {
      return [resolution.mine, resolution.other].filter((p): p is ServerPosition => Boolean(p));
    }
    return resolution.position ? [resolution.position] : [];
  }

  function device(position: ServerPosition): string {
    return position.device ?? 'this device';
  }

  /** Only for a server that predates `resume`; never used to re-derive an honesty decision. */
  function legacyDescribe(position: ServerPosition): string {
    const percent = Math.round(position.position_permille / 10);
    return position.device ? `${percent}% — on ${position.device}` : `${percent}% in`;
  }
</script>

{#if resume}
  <aside class="resume" aria-label="Resume reading" data-resume-state={resume.state}>
    <p>{resume.sentence}</p>
    {#if resume.needs_a_choice}
      <ul>
        {#each choices() as position}
          <li>
            <a href={href(null, position)}><span>{device(position)}</span></a>
          </li>
        {/each}
        {#if resolution.kind !== 'ask_the_reader'}
          <li>
            <a href={firstUnitId ? `/works/${workId}/units/${firstUnitId}` : `/works/${workId}`}>
              Start from the top
            </a>
          </li>
        {/if}
      </ul>
    {:else if resume.state === 'current' && resolution.position}
      {@const stored = resolution.position}
      <a class="resume-link" href={href(null, stored)}>
        Resume reading <span class="display">{resume.display}</span>
      </a>
    {:else}
      <span class="display">{resume.display}</span>
    {/if}
  </aside>
{:else}
  <!-- Legacy shape, kept for an older API response. -->
  {#if resolution.kind === 'use_stored' && resolution.position}
    {@const stored = resolution.position}
    <aside class="resume" aria-label="Resume reading">
      <p>You were <strong>{legacyDescribe(stored)}</strong> last time you read this.</p>
      <a class="resume-link" href={href(null, stored)}>Resume reading</a>
    </aside>
  {:else if resolution.kind === 'ask_the_reader'}
    <aside class="resume" aria-label="Resume reading">
      <p>You stopped in two places. Which one would you like?</p>
      <ul>
        {#if resolution.mine}
          {@const mine = resolution.mine}
          <li><a href={href(null, mine)}>{legacyDescribe(mine)}</a></li>
        {/if}
        {#if resolution.other}
          {@const other = resolution.other}
          <li><a href={href(null, other)}>{legacyDescribe(other)}</a></li>
        {/if}
      </ul>
    </aside>
  {/if}
{/if}

<style>
  .resume {
    background: var(--color-surface);
    border: var(--border-width) solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-3) var(--space-4);
    margin-bottom: var(--space-4);
  }

  .resume p {
    margin: 0 0 var(--space-2);
    font-size: var(--text-sm);
  }

  .resume ul {
    margin: 0;
    padding-left: var(--space-4);
  }

  .resume-link {
    font-weight: 600;
  }

  .display {
    font-family: var(--font-mono, monospace);
  }
</style>

<script lang="ts">
  /**
   * M71-07: one status vocabulary for the operator console. Colour is a second signal, never the
   * only one — a red dot that only looks red is invisible to a reader who cannot see it.
   */
  interface Props {
    level: 'red' | 'amber' | 'green';
    label: string;
    fixHref?: string;
  }
  let { level, label, fixHref }: Props = $props();
</script>

<span class="mark {level}" role="status" aria-label={`${level}: ${label}`}>
  <span class="dot" aria-hidden="true"></span>
  <span class="word">{level}</span>
  <span class="label">{label}</span>
</span>
{#if level === 'red' && fixHref}
  <a class="fix" href={fixHref}>Fix</a>
{/if}

<style>
  .mark {
    display: inline-flex;
    align-items: baseline;
    gap: var(--space-2);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }
  .dot {
    width: 0.55em;
    height: 0.55em;
    border-radius: 50%;
    align-self: center;
    background: currentColor;
  }
  .mark.red { color: var(--status-danger); }
  .mark.amber { color: var(--status-warning); }
  .mark.green { color: var(--status-success); }
  .word { font-weight: 700; text-transform: uppercase; }
  .label { color: var(--text-2); }
  .fix {
    margin-left: var(--space-2);
    color: var(--text-link);
    text-decoration: underline;
  }
</style>

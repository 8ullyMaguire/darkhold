<script lang="ts">
  /**
   * M71-07: the operator console's one alert bar. It names every red state in words and gives each
   * one a fix link; colour is never the signal. Silent when there is nothing red.
   */
  interface Alert {
    label: string;
    href: string;
  }
  let { alerts }: { alerts: Alert[] } = $props();
</script>

{#if alerts.length > 0}
  <section class="alert-bar" role="alert" aria-label="Needs attention">
    <h2>Needs attention</h2>
    <ul>
      {#each alerts as alert (alert.href + alert.label)}
        <li>
          <span class="label">{alert.label}</span>
          <a href={alert.href} aria-label={`Fix ${alert.label}`}>Fix</a>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .alert-bar {
    margin: 0 0 var(--space-4);
    padding: var(--space-2) var(--space-3);
    border: var(--border-width) solid var(--status-danger);
    border-radius: var(--radius-sm);
    background: var(--surface-1);
    font-family: var(--font-interface);
    font-size: var(--text-sm);
  }
  h2 {
    margin: 0 0 var(--space-1);
    font-size: var(--text-base);
    color: var(--status-danger);
  }
  ul {
    margin: 0;
    padding-left: var(--space-4);
  }
  li {
    display: flex;
    gap: var(--space-2);
    align-items: baseline;
    justify-content: space-between;
  }
  a {
    color: var(--text-link);
    text-decoration: underline;
    min-height: var(--tap-min);
    display: inline-flex;
    align-items: center;
  }
</style>

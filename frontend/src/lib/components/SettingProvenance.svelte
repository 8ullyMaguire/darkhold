<script lang="ts">
  import { apiFetch, type SettingProvenanceView } from '../api';

  interface Props {
    label: string;
    view: SettingProvenanceView;
    onchange?: () => void;
  }
  let { label, view, onchange }: Props = $props();
  let clearing = $state(false);
  let failure = $state<unknown>(null);

  async function reset() {
    if (!view.reset) return;
    clearing = true;
    failure = null;
    try {
      await apiFetch(view.reset.path, { method: view.reset.method });
      onchange?.();
    } catch (error) {
      failure = error;
    } finally {
      clearing = false;
    }
  }
</script>

<div class="provenance">
  <span class="label">{label}</span>
  <span class="value">{view.value}</span>
  <span class="source" title="Where this value comes from">{view.source}</span>
  {#if view.note}<span class="note">{view.note}</span>{/if}
  {#if view.reset}
    <button type="button" class="reset" onclick={reset} disabled={clearing}>
      Reset to {view.reset.becomes}
    </button>
  {/if}
  {#if failure}<span class="failure" role="alert">Reset failed; the value is unchanged.</span>{/if}
</div>

<style>
  .provenance {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2);
    margin: 0 0 var(--space-1);
    font-size: var(--text-xs);
    color: var(--text-3);
  }
  .label { font-weight: 600; color: var(--text-2); }
  .value { font-family: var(--font-mono); color: var(--text-1); }
  .source {
    padding: 0 var(--space-1);
    border: 1px solid var(--row-rule);
    border-radius: var(--radius-sm);
    text-transform: lowercase;
  }
  .note { flex-basis: 100%; }
  .reset {
    background: none;
    border: 0;
    padding: 0 var(--space-1);
    color: var(--text-link);
    text-decoration: underline;
    cursor: pointer;
  }
  .failure { flex-basis: 100%; color: var(--status-danger); }
</style>

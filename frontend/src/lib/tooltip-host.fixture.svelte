<script lang="ts">
  // A fixture, not a product component: it exists so the Tooltip's contract can be tested the way a
  // page uses it. `createRawSnippet` captures its arguments when it is created, so it cannot show that
  // the description reaches the focusable child when the tooltip opens — a real snippet, written the way
  // a consumer writes one, can.
  import { Tooltip } from '@polaris/ui';

  let { text }: { text: string } = $props();
</script>

<Tooltip {text}>
  {#snippet tip(describedBy)}
    <button aria-describedby={describedBy}>Budget</button>
  {/snippet}
</Tooltip>

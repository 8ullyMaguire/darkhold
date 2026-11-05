<script lang="ts">
  /**
   * The reader's own stylesheet (M70-06), applied to their own document.
   *
   * Two rules, both about not lying to the reader:
   *
   *  - **The stylesheet is a `<link>`, not an inline `<style>`.** The server serves it as its own document with
   *    `text/css` and `private, no-cache`, so the browser's own cache rules decide when it changes. Inlining bytes
   *    the server validated for a different context is how `</style` becomes a vulnerability, and the server refuses
   *    that sequence precisely to keep this decision reversible.
   *  - **Absence is a silent no-op.** Most readers have no skin; asking and finding nothing must not produce an
   *    error, a flash, or a console complaint.
   */
  import { onMount } from 'svelte';

  const LINK_ID = 'reader-skin';

  onMount(() => {
    let cancelled = false;

    async function apply() {
      try {
        const response = await fetch('/api/v1/me/skin', { headers: { accept: 'application/json' } });
        // Signed out is the ordinary case, not a failure.
        if (!response.ok) return;
        const body = await response.json();
        if (cancelled || !body?.present) return;

        const existing = document.getElementById(LINK_ID);
        if (existing) return;
        const link = document.createElement('link');
        link.id = LINK_ID;
        link.rel = 'stylesheet';
        link.href = body.url ?? '/api/v1/me/skin.css';
        document.head.append(link);
      } catch {
        // A skin that cannot be fetched leaves the site's own interface in place.
      }
    }

    void apply();
    return () => {
      cancelled = true;
    };
  });
</script>

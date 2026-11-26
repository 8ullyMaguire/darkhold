<script lang="ts">
  /**
   * M71-03: one result row in The Archive (ADR 0033). Dense, text-first, and honest about two
   * independent facts: where the text lives (`provenance`) and what state the record is in
   * (`status`). Both come from the server; neither is guessed here.
   */
  interface Props {
    title: string;
    author: string;
    words: number;
    score: number;
    href: string;
    provenance: 'hosted' | 'hosted_locked' | 'referenced' | 'known';
    status: 'live' | 'held' | 'private';
  }
  let { title, author, words, score, href, provenance, status }: Props = $props();

  const provenanceWords: Record<Props['provenance'], { label: string; title: string }> = {
    hosted: { label: 'Hosted here', title: 'This instance holds readable text.' },
    hosted_locked: { label: 'Hosted, locked', title: 'Text is here but needs access.' },
    referenced: { label: 'Points elsewhere', title: 'No text here — this is a pointer to a source.' },
    known: { label: 'Catalogued only', title: 'A record without text or a source yet.' },
  };
  const statusWords: Record<Props['status'], { label: string; title: string }> = {
    live: { label: 'Live', title: 'Published for everyone.' },
    held: { label: 'Held', title: 'Not published yet.' },
    private: { label: 'Private', title: 'Published, but not public.' },
  };
  let provenanceView = $derived(provenanceWords[provenance] ?? provenanceWords.known);
  let statusView = $derived(statusWords[status] ?? statusWords.held);
</script>

<article class="work-row">
  <h3><a {href}>{title}</a></h3>
  <p class="byline">by {author}</p>
  <p class="meta">
    <span>{words.toLocaleString('en-US')} words</span>
    <span aria-hidden="true">·</span>
    <span>{score} points</span>
  </p>
  <p class="facts">
    <span class="provenance {provenance}" title={provenanceView.title}>{provenanceView.label}</span>
    <span class="status {status}" title={statusView.title}>{statusView.label}</span>
  </p>
</article>

<style>
  .work-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    column-gap: var(--space-3);
    row-gap: 2px;
    align-items: baseline;
    padding: var(--space-2) 0;
    min-height: var(--density-row);
    border-bottom: 1px solid var(--row-rule);
    font-family: var(--font-interface);
  }
  h3 {
    grid-column: 1;
    margin: 0;
    font-size: var(--text-base);
    font-weight: 600;
    line-height: 1.35;
    min-width: 0;
  }
  h3 a {
    display: inline-flex;
    align-items: center;
    min-height: 44px;
    color: var(--text-link);
    text-decoration: none;
  }
  h3 a:hover {
    text-decoration: underline;
  }
  .byline, .meta {
    grid-column: 1;
    margin: 0;
    color: var(--text-3);
    font-size: var(--text-sm);
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .facts {
    grid-column: 2;
    grid-row: 1 / span 3;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    white-space: nowrap;
    font-size: var(--text-xs);
  }
  .facts span {
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
    border: 1px solid var(--row-rule);
  }
  .provenance.hosted { color: var(--provenance-hosted); }
  .provenance.hosted_locked { color: var(--provenance-held); }
  .provenance.referenced { color: var(--provenance-referenced); }
  .provenance.known { color: var(--provenance-unknown); }
  .status.live { color: var(--status-live); border-color: var(--status-live); }
  .status.held { color: var(--status-held); border-color: var(--status-held); }
  .status.private { color: var(--status-private); border-color: var(--status-private); }
  @media (max-width: 480px) {
    .work-row { grid-template-columns: minmax(0, 1fr); }
    .facts { grid-column: 1; grid-row: auto; flex-wrap: wrap; white-space: normal; }
  }
</style>

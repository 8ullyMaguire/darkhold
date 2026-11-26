# docs/plans/fs-05-search-within-results.md — plan card

Source: `docs/plans/fs-05-search-within-results.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Work from `/home/alvaro/code/rust/darkhold`. Spec: `docs/spec-amendments/faceted-search-brief-2026-11-26.md`,
## Step 1 — Search.svelte (exact edits)
In `src/routes/Search.test.ts`, add:
node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts

## Copy-pasteable code / commands

let scopedTerm = $state('');

/** Copy/paste restores exactly the query that produced the visible result set. */
function syncUrl(fullQuery: string) {
  const url = new URL(window.location.href);
  if (fullQuery.trim()) url.searchParams.set('q', fullQuery.trim());
  else url.searchParams.delete('q');
  window.history.replaceState(window.history.state, '', `${url.pathname}${url.search}`);
}

onMount(() => {
  const fromUrl = new URL(window.location.href).searchParams.get('q');
  if (fromUrl) {
    query = fromUrl;
    void runSearch();
  }
  void loadHistory();
});

## Verification command and expected output

FS-05. Acceptance: DOM test shows the scoped term appended to the active query, narrowing the search, and the URL
## Step 2 — targeted test (exact test)
In `src/routes/Search.test.ts`, add:
node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
Expected: 28/28 pass, 0 svelte errors. Deploy only after the bundle changes; live proof: SPA-navigate to `/search`,

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.

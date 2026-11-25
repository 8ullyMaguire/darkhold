# FS-05 — search within the current result set

Work from `/home/alvaro/code/rust/darkhold`. Spec: `docs/spec-amendments/faceted-search-brief-2026-11-26.md`,
FS-05. Acceptance: DOM test shows the scoped term appended to the active query, narrowing the search, and the URL
round trip keeps both clauses.

## Decision

Reuse the existing query grammar. A scoped term is appended as a plain AND clause to the same canonical query that
already carries fandom/tag/status/word-range facets. No second filter model and no backend change: the backend's
AND semantics and dual-engine coverage already exist (`M10-03`, `M71-11`, `query_phase2`). The URL carries the full
canonical query in `?q=` so a copied link restores both the facets and the scoped term.

## Step 1 — Search.svelte (exact edits)

1. Add state beside `query`:

```ts
let scopedTerm = $state('');
```

2. Add a URL sync helper before `buildQuery`:

```ts
/** Copy/paste restores exactly the query that produced the visible result set. */
function syncUrl(fullQuery: string) {
  const url = new URL(window.location.href);
  if (fullQuery.trim()) url.searchParams.set('q', fullQuery.trim());
  else url.searchParams.delete('q');
  window.history.replaceState(window.history.state, '', `${url.pathname}${url.search}`);
}
```

3. In `onMount`, restore from the URL before loading history:

```ts
onMount(() => {
  const fromUrl = new URL(window.location.href).searchParams.get('q');
  if (fromUrl) {
    query = fromUrl;
    void runSearch();
  }
  void loadHistory();
});
```

4. In `runSearch`, after computing `fullQuery`, call `syncUrl(fullQuery)` before the empty-query return.

5. Add the scoped-submit handler after `onSubmit`:

```ts
function applyScopedSearch(event: Event) {
  event.preventDefault();
  const term = scopedTerm.trim();
  if (!term || loading) return;
  query = query.trim() ? `${query.trim()} AND ${term}` : term;
  scopedTerm = '';
  void runSearch();
}
```

6. After the main `<form>`, add:

```svelte
<form class="within-form" onsubmit={applyScopedSearch}>
  <label for="within-results">Search within these results</label>
  <div>
    <input
      id="within-results"
      type="search"
      bind:value={scopedTerm}
      placeholder="Add a word or phrase to the current search"
      disabled={!buildQuery().trim()}
    />
    <button type="submit" disabled={!buildQuery().trim() || loading}>Narrow</button>
  </div>
</form>
```

7. Add CSS in `<style>`:

```css
.within-form {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}
.within-form label {
  font-weight: 600;
}
.within-form > div {
  display: flex;
  flex: 1 1 18rem;
  gap: var(--space-2);
}
.within-form input {
  flex: 1;
}
```

## Step 2 — targeted test (exact test)

In `src/routes/Search.test.ts`, add:

```ts
it('narrows within the current result set and keeps both clauses in the URL', async () => {
  window.history.replaceState({}, '', '/');
  render(Search);
  await fireEvent.input(screen.getByLabelText('Search query'), { target: { value: 'winter' } });
  await fireEvent.click(screen.getByText('Search'));
  expect(searchWorks).toHaveBeenLastCalledWith('winter', 20);
  await fireEvent.input(screen.getByLabelText('Search within these results'), {
    target: { value: 'draco' },
  });
  await fireEvent.click(screen.getByText('Narrow'));
  expect(searchWorks).toHaveBeenLastCalledWith('winter AND draco', 20);
  expect(window.location.search).toContain(encodeURIComponent('winter AND draco'));
  expect(screen.getByLabelText('Search within these results')).toHaveValue('');
});
```

## Step 3 — verification

```bash
cd /home/alvaro/code/rust/darkhold/frontend
node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
node node_modules/svelte-check/bin/svelte-check --tsconfig ./tsconfig.json
```

Expected: 28/28 pass, 0 svelte errors. Deploy only after the bundle changes; live proof: SPA-navigate to `/search`,
run a query, narrow it, copy `window.location.search`, reload, and confirm the restored query box still contains
both clauses.

Execution note: direct `/search` is server-rendered; a copied `?q=` URL restores there, while the SPA Search page restores it when mounted through in-app navigation. Live proof used both paths.

## Step 4 — state

Mark FS-05 `implemented-verified-e2e`, regenerate the ledger counts, move the roadmap row to done, update the handoff
and DECISIONS, and commit.

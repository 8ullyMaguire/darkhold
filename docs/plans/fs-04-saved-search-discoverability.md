# FS-04 — saved searches where readers already are

Work from `/home/alvaro/code/rust/darkhold`. Spec: `docs/spec-amendments/faceted-search-brief-2026-11-26.md`,
FS-04. Acceptance is the roadmap row's DOM test plus a live signed-in HTTP proof.

## Step 1 — API client (exact code)

In `frontend/src/lib/api.ts`, after `fetchSavedViews` and `deleteSavedView`, add:

```ts
export interface CreateCatalogueViewInput {
  name: string;
  kind: 'catalogue';
  params: string;
  scope?: 'private' | 'public';
}

export function createCatalogueView(
  input: CreateCatalogueViewInput,
): Promise<SavedView> {
  return apiFetch<SavedView>('/saved-views', {
    method: 'POST',
    body: JSON.stringify(input),
  });
}

export interface CreateSearchAlertInput {
  saved_search_id: string;
  frequency: 'daily' | 'weekly';
}

export function createSearchAlert(
  input: CreateSearchAlertInput,
): Promise<{ id: string; status: string }> {
  return apiFetch<{ id: string; status: string }>('/search-alerts', {
    method: 'POST',
    body: JSON.stringify(input),
  });
}
```

Verify: `node node_modules/tsc/bin/tsc --noEmit --skipLibCheck` through `svelte-check` in step 4.

## Step 2 — Search page handler (exact code)

In `frontend/src/routes/Search.svelte`:

1. Import `createCatalogueView`, `createSearchAlert`, `ApiError` from `../lib/api` and `session` from
   `../lib/session.svelte`.
2. Add state:

```ts
let savingSearch = $state(false);
let saveNote = $state<string | null>(null);
```

3. Add the handler before `onSubmit`:

```ts
async function saveThisSearch() {
  const params = buildQuery().trim();
  if (!params || savingSearch) return;
  saveNote = null;
  if (!session.isSignedIn) {
    saveNote = 'Sign in to save this search and get new matches.';
    return;
  }
  savingSearch = true;
  try {
    const name = params.length > 60 ? `${params.slice(0, 57)}…` : params;
    const view = await createCatalogueView({ name, kind: 'catalogue', params, scope: 'private' });
    await createSearchAlert({ saved_search_id: view.id, frequency: 'daily' });
    saveNote = `Saved as “${name}”; a daily alert is on.`;
  } catch (failure) {
    saveNote =
      failure instanceof ApiError && failure.isAuthRequired
        ? 'Sign in to save this search and get new matches.'
        : `Could not save this search: ${failure instanceof Error ? failure.message : 'unknown error'}`;
  } finally {
    savingSearch = false;
  }
}
```

4. After the `QueryStatePanel` block and before the histogram, add the save strip. It is rendered for any non-empty
   query, including a zero-result query:

```svelte
{#if buildQuery().trim()}
  <div class="save-search" aria-label="Save this search">
    <button type="button" onclick={saveThisSearch} disabled={savingSearch}>
      {savingSearch ? 'Saving…' : 'Save this search'}
    </button>
    {#if saveNote}<p role="status">{saveNote}</p>{/if}
  </div>
{/if}
```

5. Change the zero-result `EmptyState` to include an action snippet:

```svelte
<EmptyState
  title="No matches found"
  description="Try a different query, or use fielded search like fandom:name, tag:name, mood:name."
>
  {#snippet action()}
    <button type="button" onclick={saveThisSearch} disabled={savingSearch}>
      Save and get new matches
    </button>
  {/snippet}
</EmptyState>
```

6. Add CSS inside the existing `<style>` block:

```css
.save-search {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.save-search button,
.save-search p {
  margin: 0;
}
```

## Step 3 — targeted test (exact code)

In `frontend/src/routes/Search.test.ts`, extend the existing `vi.mock('../lib/api', ...)` with:

```ts
const createCatalogueView = vi.fn();
const createSearchAlert = vi.fn();
```

and mock exports `createCatalogueView` / `createSearchAlert`. Add `vi.mock('../lib/session.svelte', () => ({
  session: { isSignedIn: true },
}))` before importing `Search` (hoisting handles order). Reset both mocks in `beforeEach` and add:

```ts
it('saves a zero-result query as a view and attaches one daily alert', async () => {
  createCatalogueView.mockResolvedValue({ id: 'view-1' });
  createSearchAlert.mockResolvedValue({ id: 'alert-1', status: 'created' });
  render(Search);
  await fireEvent.input(screen.getByLabelText('Search query'), { target: { value: 'tag:no-such-tag' } });
  await fireEvent.click(screen.getByText('Search'));
  await waitFor(() => expect(screen.getByText('No matches found')).toBeTruthy());
  await fireEvent.click(screen.getByText('Save and get new matches'));
  await waitFor(() => expect(createCatalogueView).toHaveBeenCalledWith({
    name: 'tag:no-such-tag', kind: 'catalogue', params: 'tag:no-such-tag', scope: 'private',
  }));
  expect(createSearchAlert).toHaveBeenCalledWith({ saved_search_id: 'view-1', frequency: 'daily' });
  expect(await screen.findByText(/a daily alert is on/i)).toBeTruthy();
});
```

## Step 4 — verification

```bash
cd /home/alvaro/code/rust/darkhold/frontend
node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
node node_modules/svelte-check/bin/svelte-check --tsconfig ./tsconfig.json
```

Expected: `Search.test.ts` all green; `svelte-check` 0 errors.

## Step 5 — live proof and state

Build/deploy only after the frontend bundle changes. Then, with the existing out-of-tree review session:

```bash
curl -s -H "Cookie: darkhold_session=$TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"name":"fs04-proof","kind":"catalogue","params":"tag:fs04-proof","scope":"private"}' \
  https://darkhold.polarisocial.xyz/api/v1/saved-views
```

Expected: 201 with an `id`. Then POST `/api/v1/search-alerts` with that id and `frequency: daily`; expected 200 and
`status: created`. Delete both rows afterwards. Update `docs/requirements.csv` (status change FS-04 to
`implemented-verified-e2e`, evidence naming the two HTTP codes and the DOM test), regenerate
`docs/FEATURE-LEDGER.md` count table, update the roadmap done table and handoff, and commit.

## Decision

`catalogue` views store the canonical query string verbatim, which is what a global search produces; a `library`
view would silently reinterpret the same text against shelf/tag/status columns. The alert is created immediately so
the reader does not have to discover a second control.

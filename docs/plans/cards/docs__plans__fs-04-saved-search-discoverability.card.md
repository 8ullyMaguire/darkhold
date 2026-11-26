# docs/plans/fs-04-saved-search-discoverability.md — plan card

Source: `docs/plans/fs-04-saved-search-discoverability.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Work from `/home/alvaro/code/rust/darkhold`. Spec: `docs/spec-amendments/faceted-search-brief-2026-11-26.md`,
In `frontend/src/lib/api.ts`, after `fetchSavedViews` and `deleteSavedView`, add:
In `frontend/src/routes/Search.svelte`:
   `../lib/session.svelte`.
In `frontend/src/routes/Search.test.ts`, extend the existing `vi.mock('../lib/api', ...)` with:
and mock exports `createCatalogueView` / `createSearchAlert`. Add `vi.mock('../lib/session.svelte', () => ({
node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
`status: created`. Delete both rows afterwards. Update `docs/requirements.csv` (status change FS-04 to
`docs/FEATURE-LEDGER.md` count table, update the roadmap done table and handoff, and commit.

## Copy-pasteable code / commands

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

let savingSearch = $state(false);
let saveNote = $state<string | null>(null);

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

## Verification command and expected output

FS-04. Acceptance is the roadmap row's DOM test plus a live signed-in HTTP proof.
Verify: `node node_modules/tsc/bin/tsc --noEmit --skipLibCheck` through `svelte-check` in step 4.
## Step 3 — targeted test (exact code)
In `frontend/src/routes/Search.test.ts`, extend the existing `vi.mock('../lib/api', ...)` with:
node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
Expected: `Search.test.ts` all green; `svelte-check` 0 errors.
Expected: 201 with an `id`. Then POST `/api/v1/search-alerts` with that id and `frequency: daily`; expected 200 and
`implemented-verified-e2e`, evidence naming the two HTTP codes and the DOM test), regenerate

## RED/GREEN evidence

Expected: `Search.test.ts` all green; `svelte-check` 0 errors.

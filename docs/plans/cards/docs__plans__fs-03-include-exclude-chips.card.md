# docs/plans/fs-03-include-exclude-chips.md — plan card

Source: `docs/plans/fs-03-include-exclude-chips.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Work from `/home/alvaro/code/rust/darkhold`. Spec: `docs/spec-amendments/faceted-search-brief-2026-11-26.md`,
## Step 1 — Search.svelte (exact edits)
In `src/routes/Search.test.ts`, add to the first describe block:
node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
Execution note: direct `/search` is served by `crates/app/src/routes/search_page.rs`, not the SPA. The live DOM proof therefore loads `/library` and follows the in-app Search link before interacting. No server-rendered facet code was changed.
Mark FS-03 `implemented-verified-e2e` in `docs/requirements.csv`, regenerate `docs/FEATURE-LEDGER.md` counts, move

## Copy-pasteable code / commands

let selectedFilters = $state<{ field: FilterField; value: string; excluded: boolean }[]>([]);

const term = f.field === 'status'
  ? `completion:"${COMPLETION_VALUES[f.value] ?? f.value}"`
  : `${f.field}:"${f.value}"`;
return f.excluded ? `-${term}` : term;

function flipFilter(index: number) {
  selectedFilters = selectedFilters.map((filter, i) =>
    i === index ? { ...filter, excluded: !filter.excluded } : filter,
  );
  void runSearch();
}

## Verification command and expected output

FS-03. Acceptance: the roadmap DOM test must show a chip flipping include -> exclude -> include, with no unfiltered
## Step 2 — targeted test (exact test)
In `src/routes/Search.test.ts`, add to the first describe block:
node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
Expected: 27/27 tests pass; `svelte-check` 0 errors. No backend change, so no migration. The frontend bundle is

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.

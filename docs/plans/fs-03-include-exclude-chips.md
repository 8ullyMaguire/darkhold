# FS-03 — include/exclude parity on Search filter chips

Work from `/home/alvaro/code/rust/darkhold`. Spec: `docs/spec-amendments/faceted-search-brief-2026-11-26.md`,
FS-03. Acceptance: the roadmap DOM test must show a chip flipping include -> exclude -> include, with no unfiltered
count leaking into the chip.

## Scope decision

The Search filter bar is the include/exclude surface: it owns fandom, tag, mood, rating, language and completion
chips. QueryStatePanel chips are server-owned numeric/rating/sort/pinned facts, not tag chips, and `MetadataChip` is a
selection control on entity pages; neither is silently re-labelled as an exclusion control. This keeps "exclude" in
one place with one grammar (`-field:"value"`).

## Step 1 — Search.svelte (exact edits)

1. Change the selected-filter type and initialiser:

```ts
let selectedFilters = $state<{ field: FilterField; value: string; excluded: boolean }[]>([]);
```

2. In `addFilter`, append `{ field: selectedField, value: selectedValue, excluded: false }`.
3. In `applyShortCompletePreset`, add `excluded: false` to the status filter it builds.
4. In `buildQuery`, map each filter as:

```ts
const term = f.field === 'status'
  ? `completion:"${COMPLETION_VALUES[f.value] ?? f.value}"`
  : `${f.field}:"${f.value}"`;
return f.excluded ? `-${term}` : term;
```

5. Add the flip action beside `removeFilter`:

```ts
function flipFilter(index: number) {
  selectedFilters = selectedFilters.map((filter, i) =>
    i === index ? { ...filter, excluded: !filter.excluded } : filter,
  );
  void runSearch();
}
```

6. In the active-filter `<li>`, add a flip button before the remove button and mark excluded chips:

```svelte
<span class="filter-tag" class:excluded={filter.excluded}>{filter.field}: {filter.value}</span>
<button
  type="button"
  class="flip"
  aria-label={`${filter.excluded ? 'Include' : 'Exclude'} ${filter.field}: ${filter.value}`}
  onclick={() => flipFilter(i)}
>
  {filter.excluded ? 'Include' : 'Exclude'}
</button>
```

7. Add CSS in the existing `<style>` block:

```css
.filter-tag.excluded {
  text-decoration: line-through;
  color: var(--color-muted);
}
.active-filters .flip {
  min-height: 2.75rem;
}
```

## Step 2 — targeted test (exact test)

In `src/routes/Search.test.ts`, add to the first describe block:

```ts
it('flips a filter chip from include to exclude and back', async () => {
  render(Search);
  await fireEvent.input(screen.getByLabelText('Search query'), { target: { value: 'winter' } });
  await fireEvent.change(screen.getByLabelText('Field'), { target: { value: 'mood' } });
  await fireEvent.input(screen.getByLabelText('Value'), { target: { value: 'Angst' } });
  await fireEvent.click(screen.getByText('Add'));
  expect(searchWorks).toHaveBeenLastCalledWith('winter AND mood:"Angst"', 20);
  await fireEvent.click(screen.getByLabelText('Exclude mood: Angst'));
  expect(searchWorks).toHaveBeenLastCalledWith('winter AND -mood:"Angst"', 20);
  await fireEvent.click(screen.getByLabelText('Include mood: Angst'));
  expect(searchWorks).toHaveBeenLastCalledWith('winter AND mood:"Angst"', 20);
});
```

## Step 3 — verification

```bash
cd /home/alvaro/code/rust/darkhold/frontend
node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
node node_modules/svelte-check/bin/svelte-check --tsconfig ./tsconfig.json
```

Expected: 27/27 tests pass; `svelte-check` 0 errors. No backend change, so no migration. The frontend bundle is
embedded in the binary, so deploy after the bundle changes and prove live with a browser chip flip on `/search`.

Execution note: direct `/search` is served by `crates/app/src/routes/search_page.rs`, not the SPA. The live DOM proof therefore loads `/library` and follows the in-app Search link before interacting. No server-rendered facet code was changed.

## Step 4 — state

Mark FS-03 `implemented-verified-e2e` in `docs/requirements.csv`, regenerate `docs/FEATURE-LEDGER.md` counts, move
the roadmap row to done, add a handoff section, and commit. Record in DECISIONS why QueryStatePanel and MetadataChip
were left alone.

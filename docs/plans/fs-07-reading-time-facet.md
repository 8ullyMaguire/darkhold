# FS-07 — reading-time band facet

Work from `/home/alvaro/code/rust/darkhold`. Spec: `docs/spec-amendments/faceted-search-brief-2026-11-26.md`,
`FS-07`. Acceptance: dual-engine test covers `<30m`, `30–90m`, `2–5h`, `5h+` and `unknown`; counts are under the
current filter set.

## Decision

Use 200 words/minute, a stated, conservative reading speed, and five measured bands:
`under_30m` (0–5,999 words), `30m_90m` (6,000–17,999), `90m_2h` (18,000–23,999), `2h_5h` (24,000–59,999),
`over_5h` (60,000+). The extra `90m_2h` band closes a gap the brief's four labels would otherwise leave uncounted.
`unknown` stays the facet's existing `unknown` count, not a fake bucket. Clicking a band emits the equivalent
`words:` range, so no new query grammar is introduced.

## Step 1 — `crates/db/src/facets.rs`

1. Add `ReadingTime` to `Facet`, `"reading_time"` to `parse`, `"reading_time"` to `name`.
2. Add `reading_time_facet(&rows)` to `all_facets` output (now five answers).
3. Implement:

```rust
fn reading_time_facet(rows: &[Scanned]) -> FacetAnswer {
    const BANDS: [(&str, &str, i64, i64); 5] = [
        ("under_30m", "Under 30 minutes", 0, 5_999),
        ("30m_90m", "30 – 90 minutes", 6_000, 17_999),
        ("90m_2h", "90 minutes – 2 hours", 18_000, 23_999),
        ("2h_5h", "2 – 5 hours", 24_000, 59_999),
        ("over_5h", "Over 5 hours", 60_000, i64::MAX),
    ];
    let mut buckets: BTreeMap<String, (String, i64)> = BTreeMap::new();
    let mut known = 0;
    for row in rows {
        if let Some(words) = row.words {
            known += 1;
            let (key, label) = BANDS
                .iter()
                .find(|(_, _, low, high)| words >= *low && words <= *high)
                .map(|(key, label, _, _)| (*key, *label))
                .unwrap_or(("over_5h", "Over 5 hours"));
            buckets.entry(key.to_owned()).and_modify(|e| e.1 += 1).or_insert((label.to_owned(), 1));
        }
    }
    answer(Facet::ReadingTime, tally(buckets), known, rows.len() as i64)
}
```

The scan already carries `words` and `measured`, so this is a pure re-bucketing of the same rows — the counts are
under the same filter set as length/status/rating/cadence.

## Step 2 — dual-engine test (`crates/db/tests/facets_two_engines.rs`)

Update the existing `answers.len()` assertion from 4 to 5. Add a second test using the file's `connect`/`exec`
helpers: five works with revisions of 3k, 12k, 20k, 30k and 90k words, plus one work with a unit but no revision.
Assert `reading_time` has `known=5`, `unknown=1`, and exactly one work in each of the five band keys.

Mutant guard: change the `30m_90m` high bound from `17_999` to `23_999`; the dual-engine test must fail because
12k and 20k collapse into one bucket. Restore and rerun green.

## Step 3 — SPA facet click (`frontend/src/routes/Search.svelte`)

In `facetTerm`, add:

```ts
case 'reading_time':
  if (key === 'under_30m') return 'words:<=5999';
  if (key === '30m_90m') return 'words:6000..17999';
  if (key === '90m_2h') return 'words:18000..23999';
  if (key === '2h_5h') return 'words:24000..59999';
  if (key === 'over_5h') return 'words:>=60000';
  return null;
```

Add `"reading_time"` to the `selectable` list passed to `FacetPanel` if that list is explicit.
Add a DOM test that a `reading_time` bucket click emits the matching `words:` clause and updates `?q=`.

Execution note: mutant changed `30m_90m` high bound from `17_999` to `23_999` and failed with `one work in 30m_90m: count 2`; restored green.

## Step 4 — verification

```bash
cargo test -p darkhold-db --test facets_two_engines
DARKHOLD_TEST_PG_URL=postgres://darkhold:darkhold@localhost:55433/postgres \
  cargo test -p darkhold-db --test facets_two_engines
cd frontend && node node_modules/vitest/vitest.mjs run src/routes/Search.test.ts
```

Build/deploy only after the bundle/binary changes. Live proof: `GET /api/v1/facets` includes `reading_time` with
five buckets and an `unknown` count; a real click from the SPA emits a `words:` clause.

## Step 5 — state

Mark `FS-07` implemented-verified-e2e, regenerate counts, move the roadmap row to Done, record that this was the
last unblocked roadmap row, update handoff/DECISIONS and commit. Review rows may start only after this lands.

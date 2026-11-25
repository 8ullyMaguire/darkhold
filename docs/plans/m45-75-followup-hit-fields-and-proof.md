# M45-75 follow-up — source facts on the hit, a genuine sort proof, planner evidence

Status: **ready to implement**, 2026-11-25. Spec:
`docs/spec-amendments/stale-popular-unrated-search.md` (follow-up section).
Work in the deploy checkout `~/code/rust/darkhold`; build and test on
thinkcentre with `CARGO_TARGET_DIR=$HOME/.cargo-target/darkhold`. Targeted
tests only; no full gate. One background build if the binary changes.

## Step 1 — the hit carries the source fact (M45-75.6)

Files: `crates/db/src/search.rs`, `crates/db/src/search/ast_search.rs`.

1. Add to `SearchResult`:
   `pub source_key: String,` and `pub source_updated_at: String,`.
2. In `render_statement`'s `select`, add two correlated columns next to
   `likes`, using the SAME source row the popularity figure came from:

   ```sql
   (SELECT li_hit.source_key FROM library_items li_hit
    WHERE li_hit.work_id = works.id
    ORDER BY COALESCE(li_hit.likes, 0) DESC, li_hit.source_key ASC LIMIT 1) AS source_key,
   (SELECT li_hit.source_updated_at FROM library_items li_hit
    WHERE li_hit.work_id = works.id
    ORDER BY COALESCE(li_hit.likes, 0) DESC, li_hit.source_key ASC LIMIT 1) AS source_updated_at,
   ```

   The order is spelled identically on SQLite and PostgreSQL; `COALESCE`
   avoids NULL ordering differences. No `NULLS LAST` (dialect drift).
3. `SearchRow` gains two `String` columns after `likes`; the destructuring
   gains `source_key` and `source_updated_at` and passes both into
   `SearchResult`.
4. The two non-AST constructors in `crates/db/src/search.rs` have no source
   row: set both fields to `String::new()` there.

Verify: `cargo test -p darkhold-db --test stale_popular_unrated_search` —
1 passed. Then assert on the live JSON that a hit carries `source_key`.

## Step 2 — CLI can print it (M45-75.6, second half)

Files: `crates/cli/src/output.rs`, `crates/cli/src/q.rs`.

- Extend `SEARCH_COLUMNS` with `Column::Source` and `Column::Updated`, in that
  order after `Author`. Replace the comment that says a search hit has no
  source key or dates: it now has both.
- `rec::rows` already reads `source_key` and `source_updated_at`, so no new
  reader is needed.
- Add a CLI unit test: `shape` accepts `--fields title,source,updated` and
  `jsonl` prints those three keys.

Verify: `cargo test -p darkhold-cli source_columns_are_printable` — 1 passed.

## Step 3 — make the ordering test able to fail (M45-75.7)

File: `crates/db/tests/stale_popular_unrated_search.rs`.

- Give work 01 `updated_at = 2026-11-19T00:00:00Z` and work 02
  `updated_at = 2026-11-21T00:00:00Z`; keep likes 5 and 90.
- Assert `popular.first()` is work 02 and `newest.first()` is work 01, and
  that the two differ. This is the property a removed popularity arm breaks.
- Mutant: remove the `Some("popularity" | …)` arm from `scope_sql`, run the
  test, confirm failure naming the first hit, restore the file from a copy.
  Never commit the mutant.

Verify on both engines: SQLite 1 passed; PG 1 passed; mutant 0 passed/1 failed;
restored 1 passed.

## Step 4 — planner evidence, no speculative index (M45-75.8)

Read-only on live: `sudo -u postgres psql -d darkhold -c '\d library_items'`
and `GET /api/v1/search?q=source:qq&explain=1`. Record which existing index
serves the correlated `EXISTS`. Create a migration only if the plan shows a
full scan of the 90k-row table; otherwise the record is the proof.

## Step 5 — closeout

Regenerate `docs/requirements.csv`, `docs/FEATURE-LEDGER.md`,
`docs/plans/roadmap-remaining.md` and `docs/handoff.md` in the same commit.
If the binary changed, one background release build with the estate build-mode
gate, then restart and prove `GET /api/v1/search?q=source:qq` carries
`source_key` on a hit. Never widen visibility to manufacture a hit.

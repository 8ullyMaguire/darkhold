# docs/plans/m45-75-followup-hit-fields-and-proof.md — plan card

Source: `docs/plans/m45-75-followup-hit-fields-and-proof.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/spec-amendments/stale-popular-unrated-search.md` (follow-up section).
Files: `crates/db/src/search.rs`, `crates/db/src/search/ast_search.rs`.
4. The two non-AST constructors in `crates/db/src/search.rs` have no source
Files: `crates/cli/src/output.rs`, `crates/cli/src/q.rs`.
File: `crates/db/tests/stale_popular_unrated_search.rs`.
Regenerate `docs/requirements.csv`, `docs/FEATURE-LEDGER.md`,
`docs/plans/roadmap-remaining.md` and `docs/handoff.md` in the same commit.

## Copy-pasteable code / commands

   (SELECT li_hit.source_key FROM library_items li_hit
    WHERE li_hit.work_id = works.id
    ORDER BY COALESCE(li_hit.likes, 0) DESC, li_hit.source_key ASC LIMIT 1) AS source_key,
   (SELECT li_hit.source_updated_at FROM library_items li_hit
    WHERE li_hit.work_id = works.id
    ORDER BY COALESCE(li_hit.likes, 0) DESC, li_hit.source_key ASC LIMIT 1) AS source_updated_at,
   

## Verification command and expected output

Work in the deploy checkout `~/code/rust/darkhold`; build and test on
tests only; no full gate. One background build if the binary changes.
Verify: `cargo test -p darkhold-db --test stale_popular_unrated_search` —
1 passed. Then assert on the live JSON that a hit carries `source_key`.
- Add a CLI unit test: `shape` accepts `--fields title,source,updated` and
Verify: `cargo test -p darkhold-cli source_columns_are_printable` — 1 passed.
## Step 3 — make the ordering test able to fail (M45-75.7)
- Assert `popular.first()` is work 02 and `newest.first()` is work 01, and
  test, confirm failure naming the first hit, restore the file from a copy.
Verify on both engines: SQLite 1 passed; PG 1 passed; mutant 0 passed/1 failed;
gate, then restart and prove `GET /api/v1/search?q=source:qq` carries

## RED/GREEN evidence

1 passed. Then assert on the live JSON that a hit carries `source_key`.
Verify: `cargo test -p darkhold-cli source_columns_are_printable` — 1 passed.
- Mutant: remove the `Some("popularity" | …)` arm from `scope_sql`, run the
  Never commit the mutant.
Verify on both engines: SQLite 1 passed; PG 1 passed; mutant 0 passed/1 failed;
restored 1 passed.

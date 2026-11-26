# docs/plans/m63-06-saved-queries.md — plan card

Source: `docs/plans/m63-06-saved-queries.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

> create/rename/delete are still session-only. See `docs/plans/m63-13-cron-install.md`.
`docs/ideas/2026-11-01-idea-sweep-power-users.md` (`--save`, `--digest-diff`).
Storage already exists and is complete: `saved_views` (migration `0009_library.sql`, both dialects) with
2. **There is no run state anywhere.** `grep -rn "digest" crates/` is empty; nothing records what a
### Step 1 — migration `0149_saved_query_runs.sql`, both dialects
`migrations/sqlite/0149_saved_query_runs.sql`:
`migrations/postgres/0149_saved_query_runs.sql`: identical, with
`crates/domain/src/library.rs`, beside `validate_query`:
`crates/db/src/library.rs`:
`crates/app/src/routes/library.rs`:
- `crates/app/src/routes/inventory.rs`: one `ROUTE_TABLE` entry for `POST /saved-views/{id}/runs`
### Step 5 — CLI (`crates/cli/src/rec.rs`)

## Copy-pasteable code / commands

-- M63-06: what a saved query answered last time, so a rerun can say what is new.
--
-- One row per view: the id set is the *answer*, not a history, and keeping it out
-- of `saved_views` keeps a run from bumping the row's optimistic version.
CREATE TABLE saved_query_runs (
    saved_view_id TEXT    PRIMARY KEY REFERENCES saved_views (id) ON DELETE CASCADE,
    ran_at        TEXT    NOT NULL,
    seen_ids_json TEXT    NOT NULL,
    seen_count    INTEGER NOT NULL
);

/// Which of `current` were not in `previous`, in the order they arrived.
///
/// The diff is over ids, not timestamps: a catalogue row can enter a saved
/// query's answer because its `source_updated_at` moved, because a tag backfill
/// gave it a fandom, or because a `-7d` window slid — none of which is a
/// "created since" fact. Only the previous answer can say what is new.
///
/// Duplicates in `current` are the caller's mistake and are preserved, not
/// collapsed: this function answers a question about a page, and a page that
/// repeats an id is a bug the caller should see.
#[must_use]
pub fn new_ids_since(previous: &[String], current: &[String]) -> Vec<String> {
    current
        .iter()
        .filter(|id| !previous.contains(id))
        .cloned()
        .collect()
}

    #[test]
    fn a_first_run_is_all_new_and_a_repeat_is_none() {
        let page = vec!["a".into(), "b".into()];
        assert_eq!(library::new_ids_since(&[], &page), page);
        assert_eq!(library::new_ids_since(&page, &page), Vec::<String>::new());
    }

    #[test]
    fn an_id_that_left_the_page_and_came_back_is_new_again() {
        // A window slides: `--updated 7d` run today drops last week's answer and
        // a row can walk back in. Comparing sets, not history, is the point.
        let previous = vec!["a".into(), "b".into()];
        let current = vec!["b".into(), "a".into(), "c".into()];
        assert_eq!(library::new_ids_since(&previous, &current), vec!["c".to_owned()]);
    }

## Verification command and expected output

    #[test]
    #[test]
cargo test -p darkhold-domain --lib library       # the diff, pure
cargo test -p darkhold-db --test saved_queries    # both dialects: v1 decode, catalogue view, runs
cargo test -p darkhold-app --test m63_06_saved_queries   # the four routes through the router
cargo test -p darkhold-cli --lib rec              # flags, refusals, rendering
bash scripts/gate.sh ledger && bash scripts/gate.sh repo
Mutation proof: `new_ids_since` returning `current` unchanged must fail the repeat-run test; summing
`previous` from the current page instead of the stored row must fail the db test.

## RED/GREEN evidence

Tests (RED first: `error[E0425]: cannot find function 'new_ids_since'`):

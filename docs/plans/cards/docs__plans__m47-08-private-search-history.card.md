# docs/plans/m47-08-private-search-history.md — plan card

Source: `docs/plans/m47-08-private-search-history.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

settings row kept the id. The test file is `crates/app/tests/m47_08b_search_history.rs`.
 **Milestone:** M47. **Status:** implemented 2026-11-02 (`6d14648d`); see `docs/verification.md` for the two engine bugs the gates caught.
**Source:** `docs/audit-100-reader-ideas-2026-10-04.md` item 31, and
`docs/plans/100-ideas-remaining.md` §2, which names item 31 as "next in queue".
that exist". Probed: `grep -rn "search_history\|SearchHistory" crates/` returns nothing, and no
| table | `migrations/{postgres,sqlite}/0139_search_history.sql` |
| store | `crates/db/src/search_history.rs` |
| setting | `search.record_history` in `crates/domain/src/settings.rs` |
| reader UI | chips under the search box in `frontend/src/routes/Search.svelte` |
// crates/db/src/search_history.rs
Both routes are added to `crates/app/src/routes/inventory.rs`, because that inventory is what the
cd frontend && npx vitest run src/routes/Search.test.ts

## Copy-pasteable code / commands

CREATE TABLE IF NOT EXISTS search_history (
    id           TEXT NOT NULL PRIMARY KEY,
    pseud_id     TEXT NOT NULL,
    query        TEXT NOT NULL,
    query_key    TEXT NOT NULL,
    result_count INTEGER NOT NULL,
    searched_at  TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS search_history_pseud_key
    ON search_history (pseud_id, query_key);
CREATE INDEX IF NOT EXISTS search_history_pseud_recent
    ON search_history (pseud_id, searched_at DESC);

// crates/db/src/search_history.rs
pub struct HistoryEntry { pub query: String, pub result_count: i64, pub searched_at: String }

/// Record one successful search, keeping only the newest `cap` per pseud.
pub async fn record(db: &Database, pseud_id: Uuid, query: &str, result_count: i64, cap: i64) -> Result<()>;
/// Newest-first history for one pseud.
pub async fn recent(db: &Database, pseud_id: Uuid, limit: i64) -> Result<Vec<HistoryEntry>>;
/// Remove every row for one pseud; returns how many were removed.
pub async fn clear(db: &Database, pseud_id: Uuid) -> Result<u64>;

DELETE FROM search_history
WHERE pseud_id = ?
  AND id NOT IN (
    SELECT id FROM search_history WHERE pseud_id = ?
    ORDER BY searched_at DESC, id LIMIT ?
  )

## Verification command and expected output

settings row kept the id. The test file is `crates/app/tests/m47_08b_search_history.rs`.
cargo test -p darkhold-db --test search_history            # both engines via DARKHOLD_TEST_PG_URL
cargo test -p darkhold-app --test m47_08b_search_history    # routes
cd frontend && npx vitest run src/routes/Search.test.ts

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.

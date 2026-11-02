# M47-08: private search history (audit item 31)

**Ledger row:** new, `M47-08b`.
**Renumbered 2026-11-02 (M47-15):** this row's id is `M47-08b`, not `M47-08` — the id named two rows
(this one, and the §46 settings export/import row that held it first), so the later row moved and the
settings row kept the id. The test file is `crates/app/tests/m47_08b_search_history.rs`.
 **Milestone:** M47. **Status:** implemented 2026-11-02 (`6d14648d`); see `docs/verification.md` for the two engine bugs the gates caught.
**Source:** `docs/audit-100-reader-ideas-2026-10-04.md` item 31, and
`docs/plans/100-ideas-remaining.md` §2, which names item 31 as "next in queue".

## What the audit got wrong about it

Item 31 is listed under "none needs a migration, all five are query-or-render work over columns
that exist". Probed: `grep -rn "search_history\|SearchHistory" crates/` returns nothing, and no
table holds a query string. The column it needs does not exist, so this one does need a migration
and the audit's own summary of the five is wrong for one of them. That is the fourth time this
repository's status documents have described a file tree from memory.

## What ships

| surface | file |
|---|---|
| table | `migrations/{postgres,sqlite}/0139_search_history.sql` |
| store | `crates/db/src/search_history.rs` |
| routes | `GET /api/v1/search/history`, `DELETE /api/v1/search/history`, recording inside `GET /api/v1/search` |
| setting | `search.record_history` in `crates/domain/src/settings.rs` |
| reader UI | chips under the search box in `frontend/src/routes/Search.svelte` |

## The table, and the four decisions in it

```sql
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
```

1. **`query_key` is a column, not `lower(query)` in an index.** SQLite's `lower()` folds ASCII
   only, so `Éowyn` and `éowyn` would be two rows on one engine and one row on the other. The
   normalisation (trim, collapse internal whitespace, `to_lowercase`) happens once in Rust, and
   both engines then compare the same bytes.
2. **`pseud_id` is `TEXT` with no foreign key**, matching `0137_wip_adoption`: pseud ids are UUID
   on PostgreSQL and TEXT everywhere in the recent tables, and §51 erases an account by rekeying
   the pseud rather than deleting it, so a cascade would never fire.
3. **No `account_id`.** The audit says "per pseud", and adding the account would make an
   erasure-rekeyed pseud's history reachable by the account that used to own it.
4. **`result_count` is stored, not recomputed.** The number of results a query returned is a fact
   about that search — re-running it later returns a different number, and a history that
   silently disagreed with what the reader saw would be worse than one showing no number at all.

## Store API

```rust
// crates/db/src/search_history.rs
pub struct HistoryEntry { pub query: String, pub result_count: i64, pub searched_at: String }

/// Record one successful search, keeping only the newest `cap` per pseud.
pub async fn record(db: &Database, pseud_id: Uuid, query: &str, result_count: i64, cap: i64) -> Result<()>;
/// Newest-first history for one pseud.
pub async fn recent(db: &Database, pseud_id: Uuid, limit: i64) -> Result<Vec<HistoryEntry>>;
/// Remove every row for one pseud; returns how many were removed.
pub async fn clear(db: &Database, pseud_id: Uuid) -> Result<u64>;
```

`record` is `INSERT … ON CONFLICT (pseud_id, query_key) DO UPDATE`, then a trim:

```sql
DELETE FROM search_history
WHERE pseud_id = ?
  AND id NOT IN (
    SELECT id FROM search_history WHERE pseud_id = ?
    ORDER BY searched_at DESC, id LIMIT ?
  )
```

The trim is not optional: without it the table grows one row per distinct query forever, and
"history" becomes a log — which is the one thing the reader did not agree to.

## Routes

- `GET /api/v1/search/history?limit=10` — `RequireSession`; 401 for an anonymous caller (there is
  no pseud to key on, and answering an empty list would hide the distinction between "no history"
  and "not signed in"). `limit` clamps to `1..=20`.
- `DELETE /api/v1/search/history` — `RequireSession`; returns `{"cleared": n}`.
- Recording lives at the end of the existing `search` handler, **after** the query succeeded:
  a 422 records nothing, a 200 records the trimmed `q` with the result count, and a failure of the
  history write is logged and discarded — a history write must never turn a working search into an
  500.

Both routes are added to `crates/app/src/routes/inventory.rs`, because that inventory is what the
route tests iterate and an unlisted route is invisible to them.

## Reader UI

Under the search box: the newest queries as chips, newest first, each one a button that fills the
box and runs. A "Clear" control next to them calls the delete route and empties the row without a
confirmation dialog — an undo-less delete of something the reader can recreate by searching again
does not need one. The row is hidden entirely when the history is empty, which covers both the
signed-out case and the `record_history = false` case without a second request.

## Gates

```bash
cargo test -p darkhold-db --test search_history            # both engines via DARKHOLD_TEST_PG_URL
cargo test -p darkhold-app --test m47_08b_search_history    # routes
cd frontend && npx vitest run src/routes/Search.test.ts
cd frontend && npx svelte-check --threshold error
```

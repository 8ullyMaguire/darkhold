# docs/plans/m45-22-concierge.md — plan card

Source: `docs/plans/m45-22-concierge.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Spec: `docs/spec.md` §54. Implementation plan. Tracker row: `docs/requirements.csv`
| Strategy registry + RRF blend | `crates/db/src/rec_strategy.rs`, `crates/app/src/rec_engine.rs` | §16.1a. `RecContext { account_id, seen, cap }` |
| Mood taxonomy | `crates/domain/src/taxonomy.rs` (`NodeKind::Mood`), `crates/domain/src/query.rs` (`QueryField::Mood`) | §15.8 / M10-06, `implemented-locally-tested` |
| Reading progress | `crates/db/src/reading.rs` | `save_progress`, `progress_for` |
| Notifications | `crates/db/src/notifications.rs` | `notify(db, account_id, kind, title, body, work_id)` |
| `Candidate` | `crates/domain/src/discovery.rs` | `work_id, score, reason, taste_signal, diversity_class` |
**File:** `crates/app/src/rec_engine.rs`, `crates/db/src/rec_strategy.rs`
Add to `crates/db/src/reading.rs`:
    let sql = db.sql(
> `migrations/sqlite/0004_reading.sql`. `subject_id` is TEXT on SQLite and UUID on
Then in `rec_engine.rs`, populate it:
In `rec_strategy.rs::generate_traced`, make the filter real:

## Copy-pasteable code / commands

/// Every work this account has any reading signal for.
///
/// Deliberately broad: a read, a started read and a rating all mean "this
/// reader has already been offered this and did not need it again". A narrow
/// definition would let the queue re-serve a work the reader DNF'd three weeks
/// ago.
///
/// **The shape is not three `work_id` columns.** `reading_history_entry` and
/// `reading_progress` are polymorphic — `subject_type` + `subject_id` — so a
/// work is `subject_type = 'work'`. Only `rating` has a real `work_id`. Querying
/// them as if they were all the same shape returns nothing for the two tables
/// that hold most of a reader's history.
pub async fn seen_work_ids(db: &Database, account_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let sql = db.sql(
        "SELECT subject_id FROM reading_history_entry
          WHERE account_id = ? AND subject_type = 'work'
         UNION
         SELECT subject_id FROM reading_progress
          WHERE account_id = ? AND subject_type = 'work'
         UNION
         SELECT work_id FROM rating
          WHERE account_id = ? AND deleted_at IS NULL",
        "SELECT subject_id::text FROM reading_history_entry
          WHERE account_id = $1::uuid AND subject_type = 'work'
         UNION
         SELECT subject_id::text FROM reading_progress
          WHERE account_id = $1::uuid AND subject_type = 'work'
         UNION
         SELECT work_id::text FROM rating
          WHERE account_id = $1::uuid AND deleted_at IS NULL",
    );
    match db.backend() {
        Backend::Sqlite => Ok(sqlx::query_scalar(&sql)
            .bind(account_id).fetch_all(db.sqlite_pool().ok_or(pool_err())?).await?),
        Backend::Postgres => Ok(sqlx::query_scalar(&sql)
            .bind(account_id).fetch_all(db.postgres_pool().ok_or(pool_err())?).await?),
    }
}

pub async fn generate_with_registry(
    db: &Database,
    registry: &RecRegistry,
    account_id: &str,
    limit: usize,
) -> Result<Vec<String>> {
    let seen = darkhold_db::reading::seen_work_ids(db, account_id).await.unwrap_or_default();
    let ctx = RecContext { account_id: account_id.to_string(), seen, cap: limit };
    registry.generate(db, ctx).await
}

let mut seen_set: HashSet<String> = ctx.seen.iter().cloned().collect();
// ...
for (rank, work_id) in ranked.iter().enumerate() {
    if seen_set.contains(work_id) {
        continue;   // already offered; §36.11's exclusion
    }
    let entry = scores.entry(work_id.clone()).or_insert(0.0);
    *entry += 1.0 / (self.k + (rank + 1) as f64);
}

## Verification command and expected output

**Verify**
cargo test -p darkhold-app --lib rec_engine 2>&1 | tail -5
cargo test -p darkhold-db --lib rec_strategy 2>&1 | grep 'test result'
Then prove the filter bites — add this test to `crates/db/src/rec_strategy.rs`:
#[tokio::test]
    assert!(ids.is_empty(), "a seen work must not be blended: {ids:?}");
lines and confirm it goes red. Restore, confirm green. A test that has not been
cargo test -p darkhold-app --test route_inventory 2>&1 | tail -3
cargo test -p darkhold-app --lib discovery 2>&1 | tail -3
**Verify**
cargo test -p darkhold-db --lib migration 2>&1 | grep -E '0114|test result'
**Verify**
cargo test -p darkhold-domain --lib concierge 2>&1 | grep 'test result'
| Test | Asserts |
**Verify**
cargo test -p darkhold-db --lib concierge_store 2>&1 | grep 'test result'
  cargo test -p darkhold-db --lib concierge_store 2>&1 | grep 'test result'
**Verify**
cargo test -p darkhold-app --test concierge_routes 2>&1 | grep 'test result'
  cargo test -p darkhold-app --test concierge_routes 2>&1 | grep 'test result'

## RED/GREEN evidence

the exact output that means it passed. Do not start a step before the previous
lines and confirm it goes red. Restore, confirm green. A test that has not been
seen red proves nothing.
Required tests, each of which must be proven red by breaking it:
  own. Break it by deleting `AND account_id = ?` and confirm red.
`notified_at` unconditionally and confirm `a_watcher_is_notified_once…` goes red.
HTTP 200 while the test suite stays green.
- [ ] All §54.7 acceptance criteria have a named test, green on **both** engines.
- [ ] Every new test has been seen red.

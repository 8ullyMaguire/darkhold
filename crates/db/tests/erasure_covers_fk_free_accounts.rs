//! M45-64 — every account-keyed column outside the foreign-key graph is classified, and handled.
//!
//! The erasure's deletion is `DELETE FROM accounts` and the schema's cascade. That is right for the
//! 71 tables whose FK chain reaches `accounts`, and **blind** to a table whose `account_id` is a
//! plain TEXT/UUID column with no `REFERENCES` clause — measured 2026-11-02: 18 such columns across
//! 16 tables, every one of which survived an erasure, under an id that no longer resolved to anybody.
//!
//! Two kinds of evidence here, because they fail differently:
//!
//! * **the schema invariant** (`every_account_column_outside_the_fk_graph_is_classified`) does not
//!   need a database — it parses the migrations and refuses an unclassified column, so the *next*
//!   table cannot be added without deciding what erasure does to it. It also refuses an entry that
//!   names a column the schema no longer has, so the list cannot rot in place.
//! * **the behavioural tests** run a real erasure and check the two treatments: a row that is about
//!   the person is gone, and a row that must survive survives with its account link cleared.
//!
//! The invariant cannot prove the *treatment* is right — that is a judgement recorded in
//! `FK_FREE_ACCOUNT_COLUMNS` with a reason per column — but it proves there is no silent fourth
//! option, which is what the leak was.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use darkhold_db::erasure::{erase_account, FK_FREE_ACCOUNT_COLUMNS};
use darkhold_db::Backend;
use test_support::TestDb;

fn migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("migrations")
        .join("sqlite")
}

/// `(table, column)` for every account-keyed column that no `REFERENCES accounts` clause covers.
fn unlinked_account_columns() -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(migrations_dir())
        .expect("migrations/sqlite")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    files.sort();

    for path in files {
        let text = std::fs::read_to_string(&path).expect("read a migration");
        // Walk `CREATE TABLE <name> ( … );` blocks. The migrations are plain SQL and this scanner
        // only needs the block's text, so a simple delimiter scan is enough and a full parser would
        // be a second thing to keep correct.
        let mut rest = text.as_str();
        while let Some(start) = rest.find("CREATE TABLE") {
            let after = &rest[start..];
            let Some(open) = after.find('(') else { break };
            let Some(close) = after[open..].find("\n);") else {
                break;
            };
            let block = &after[..open + close];
            // `CREATE TABLE [IF NOT EXISTS] <name> (` — the `IF NOT EXISTS` form is the common one
            // in this tree, and taking the third word blindly names every such table `IF`.
            let name = after
                .split('(')
                .next()
                .unwrap_or_default()
                .trim()
                .trim_start_matches("CREATE TABLE")
                .trim()
                .trim_start_matches("IF NOT EXISTS")
                .trim()
                .trim_matches('"')
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string();
            let table_covers_account = block.contains("REFERENCES accounts");
            for line in block.lines() {
                let mut words = line.trim().trim_end_matches(',').split_whitespace();
                let Some(column) = words.next() else { continue };
                // `account_id`, `*_account` (payment_events.author_account) and `*_account_id`
                // (trust_events.actor_account_id) are the three spellings the tree uses. Missing the
                // third is exactly how this test found its own list wrong on the first run.
                let account_keyed = column == "account_id"
                    || column.ends_with("_account")
                    || column.ends_with("_account_id");
                if !account_keyed || column.starts_with("--") {
                    continue;
                }
                // A per-column `REFERENCES accounts` covers this column; the table-level check above
                // covers the common shape where the clause sits on the column's own line.
                let column_covers = line.contains("REFERENCES accounts");
                if !column_covers && !table_covers_account {
                    out.insert((name.clone(), column.to_string()));
                }
            }
            rest = &after[open + close..];
        }
    }
    out
}

#[test]
fn every_account_column_outside_the_fk_graph_is_classified() {
    let measured = unlinked_account_columns();
    assert!(
        !measured.is_empty(),
        "the scan found no unlinked account columns at all, which means the scanner broke rather \
         than the schema getting safer — the leak this test exists for was 18 columns"
    );

    let classified: BTreeSet<(String, String)> = FK_FREE_ACCOUNT_COLUMNS
        .iter()
        .map(|c| (c.table.to_string(), c.column.to_string()))
        .collect();

    let unclassified: Vec<String> = measured
        .difference(&classified)
        .map(|(t, c)| format!("{t}.{c}"))
        .collect();
    assert!(
        unclassified.is_empty(),
        "these account columns are outside the foreign-key graph and no erasure treatment names \
         them, so `DELETE FROM accounts` leaves their rows behind:\n  {}",
        unclassified.join("\n  ")
    );

    let stale: Vec<String> = classified
        .difference(&measured)
        .map(|(t, c)| format!("{t}.{c}"))
        .collect();
    assert!(
        stale.is_empty(),
        "FK_FREE_ACCOUNT_COLUMNS names columns the schema does not have (a rename, or a typo in the \
         list — either way the treatment is not being applied):\n  {}",
        stale.join("\n  ")
    );
}

#[test]
fn no_treatment_is_deferred_without_an_owner() {
    // A deferred entry is a tracked gap, and a gap with no owning requirement is an omission with
    // extra steps. The owner is a row id, so it can be looked up.
    for column in FK_FREE_ACCOUNT_COLUMNS {
        if let darkhold_db::erasure::Treatment::DeferredTo(owner) = column.treatment {
            assert!(
                owner.starts_with("M") && owner.contains('-'),
                "{}.{} is deferred to `{owner}`, which is not a requirement id",
                column.table,
                column.column
            );
        }
    }
}

/// Insert a row of a table whose only interesting column is `account_id`, on either engine.
async fn exec(tdb: &TestDb, sqlite: &str, pg: &str, binds: &[&str]) -> Result<u64, String> {
    let sql = tdb.db().sql(sqlite, pg);
    // The query is built per arm rather than once: `sqlx::query::Query` is generic over the
    // database, so one value cannot be executed against both pools.
    match tdb.db().backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query(&sql);
            for value in binds {
                query = query.bind(*value);
            }
            query
                .execute(tdb.db().sqlite_pool().expect("sqlite handle"))
                .await
                .map(|result| result.rows_affected())
                .map_err(|error| error.to_string())
        }
        Backend::Postgres => {
            let mut query = sqlx::query(&sql);
            for value in binds {
                query = query.bind(*value);
            }
            query
                .execute(tdb.db().postgres_pool().expect("postgres handle"))
                .await
                .map(|result| result.rows_affected())
                .map_err(|error| error.to_string())
        }
    }
}

async fn count(tdb: &TestDb, table: &str, account_id: &str) -> i64 {
    let sqlite = format!("SELECT COUNT(*) FROM {table} WHERE account_id = ?");
    let postgres = format!("SELECT COUNT(*) FROM {table} WHERE account_id::text = $1");
    let sql = tdb.db().sql(&sqlite, &postgres);
    match tdb.db().backend() {
        Backend::Sqlite => {
            sqlx::query_scalar::<_, i64>(&sql)
                .bind(account_id)
                .fetch_one(tdb.db().sqlite_pool().expect("sqlite handle"))
                .await
        }
        Backend::Postgres => {
            sqlx::query_scalar::<_, i64>(&sql)
                .bind(account_id)
                .fetch_one(tdb.db().postgres_pool().expect("postgres handle"))
                .await
        }
    }
    .expect("count")
}

/// A real account row, because the two arms do not agree about `payment_events`.
///
/// On PostgreSQL that table carries `REFERENCES accounts (account_id) ON DELETE SET NULL`; on
/// SQLite it carries no foreign key at all (measured — `PRAGMA foreign_key_list` returns nothing
/// for it, and the PostgreSQL arm declares both columns). So an insert written for SQLite fails on
/// PostgreSQL, and the fix is a real account rather than a cast: the erasure is supposed to run
/// against an account that exists.
async fn insert_account(tdb: &TestDb, account_id: &str) {
    exec(
        tdb,
        "INSERT INTO accounts (id, email, created_at, updated_at) \
         VALUES (?, 'erased@example.test', '2026-11-02T00:00:00Z', '2026-11-02T00:00:00Z')",
        "INSERT INTO accounts (id, email, created_at, updated_at) \
         VALUES ($1::uuid, 'erased@example.test', '2026-11-02T00:00:00Z', '2026-11-02T00:00:00Z')",
        &[account_id],
    )
    .await
    .expect("insert an account");
}

#[tokio::test]
async fn a_row_about_the_person_is_deleted() {
    let tdb =
        TestDb::connect_with_dir("m45_64_delete", &test_support::scratch_dir("m45_64_delete"))
            .await;
    let account = "11111111-1111-1111-1111-111111111111";
    insert_account(&tdb, account).await;

    // Two tables with `Delete` treatments, one from each end of the tree: a taste judgement (the
    // most personal thing here) and a module preference (M65-10's own row).
    exec(
        &tdb,
        "INSERT INTO directory_votes (entry_id, account_id, vote_value, voted_at) \
         VALUES ('e1', ?, 1, '2026-11-02T00:00:00Z')",
        "INSERT INTO directory_votes (entry_id, account_id, vote_value, voted_at) \
         VALUES ('e1', $1, 1, '2026-11-02T00:00:00Z')",
        &[account],
    )
    .await
    .expect("insert a directory vote");

    exec(
        &tdb,
        "INSERT INTO module_user_state (account_id, module_id, enabled, updated_at) \
         VALUES (?, 'quiz', 0, '2026-11-02T00:00:00Z')",
        "INSERT INTO module_user_state (account_id, module_id, enabled, updated_at) \
         VALUES ($1, 'quiz', 0, '2026-11-02T00:00:00Z')",
        &[account],
    )
    .await
    .expect("insert a module preference");

    assert_eq!(count(&tdb, "directory_votes", account).await, 1);
    assert_eq!(count(&tdb, "module_user_state", account).await, 1);

    erase_account(tdb.db(), account).await.expect("erase");

    assert_eq!(
        count(&tdb, "directory_votes", account).await,
        0,
        "a vote is a personal act; there is no account id left to rekey it to, and keeping it \
         under a dead id is the leak this row is about"
    );
    assert_eq!(
        count(&tdb, "module_user_state", account).await,
        0,
        "a module preference restored against an erased reader's wish is exactly what the policy \
         entry feared — delete, not rekey"
    );

    tdb.cleanup().await;
}

#[tokio::test]
async fn a_row_that_must_survive_loses_its_account_link() {
    let tdb =
        TestDb::connect_with_dir("m45_64_sever", &test_support::scratch_dir("m45_64_sever")).await;
    let account = "22222222-2222-2222-2222-222222222222";
    insert_account(&tdb, account).await;

    exec(
        &tdb,
        "INSERT INTO payment_events \
         (id, kind, account_id, work_id, author_account, amount_minor, processor_fee_minor, \
          currency, net_minor, created_at) \
         VALUES ('33333333-3333-3333-3333-333333333333', 'tip', ?, NULL, NULL, 500, 25, 'EUR', 475, '2026-11-02T00:00:00Z')",
        "INSERT INTO payment_events \
         (id, kind, account_id, work_id, author_account, amount_minor, processor_fee_minor, \
          currency, net_minor, created_at) \
         VALUES ('33333333-3333-3333-3333-333333333333', 'tip', $1::uuid, NULL, NULL, 500, 25, 'EUR', 475, '2026-11-02T00:00:00Z')",
        &[account],
    )
    .await
    .expect("insert a payment event");

    erase_account(tdb.db(), account).await.expect("erase");

    assert_eq!(
        count(&tdb, "payment_events", account).await,
        0,
        "no row may still be addressed to the erased account"
    );

    // The record itself survives: handing money back is a accounting obligation, and the amount is
    // not personal data about the person who paid. This is the half that a blanket DELETE would get
    // wrong in the other direction.
    // The id is a UUID column on PostgreSQL and TEXT on SQLite, so it is compared as text.
    let sql = "SELECT COUNT(*) FROM payment_events \
               WHERE CAST(id AS TEXT) = '33333333-3333-3333-3333-333333333333' \
                 AND account_id IS NULL";
    let surviving: i64 = match tdb.db().backend() {
        Backend::Sqlite => {
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(tdb.db().sqlite_pool().expect("sqlite handle"))
                .await
        }
        Backend::Postgres => {
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(tdb.db().postgres_pool().expect("postgres handle"))
                .await
        }
    }
    .expect("count survivors");
    assert_eq!(
        surviving, 1,
        "the payment record must survive with its identity severed: a financial record that \
         vanishes on erasure breaks the ledger, and one that keeps the payer breaks the erasure"
    );

    tdb.cleanup().await;
}

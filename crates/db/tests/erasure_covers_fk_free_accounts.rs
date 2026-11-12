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

use darkhold_db::erasure::{erase_account, FK_FREE_ACCOUNT_COLUMNS};
use darkhold_db::Backend;
use test_support::TestDb;

/// `(table, column)` for every account-keyed column that no `REFERENCES accounts` clause covers.
/// What the migrated schema says, not what the migration text mentions.
///
/// The first version of this scan read every `CREATE TABLE` block it could find and unioned them. That is wrong twice
/// over: `0013_community.sql` begins `DROP TABLE IF EXISTS blocks; DROP TABLE IF EXISTS mutes;` and recreates both, so
/// the union kept five columns the database does not have; and a `REFERENCES accounts` clause written on a line the
/// pattern did not reach made a linked column look unlinked. A text parser can be made to agree with itself. It cannot
/// be made to agree with a database.
///
/// So the schema comes from SQLite: `sqlite_master` for the tables, `pragma_table_info` for the columns,
/// `pragma_foreign_key_list` for the edges. A column is *measured* when its name is one of the account spellings and its
/// table cannot reach `accounts` by following foreign keys — because a table that can reach `accounts` is deleted or
/// severed by the database itself, and a treatment there would be a second opinion about a job already done.
async fn measured_account_columns(
    tdb: &TestDb,
) -> (BTreeSet<(String, String)>, BTreeSet<(String, String)>) {
    let pool = tdb
        .db()
        .sqlite_pool()
        .expect("this scanner reads the SQLite schema; the PG run has its own migrations");
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .expect("list tables");

    let mut columns: BTreeSet<(String, String)> = BTreeSet::new();
    let mut edges: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();

    for table in &tables {
        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
            .bind(table)
            .fetch_all(pool)
            .await
            .expect("columns of a table");
        for name in names {
            columns.insert((table.clone(), name));
        }
        let targets: Vec<String> =
            sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_list(?)")
                .bind(table)
                .fetch_all(pool)
                .await
                .expect("foreign keys of a table");
        edges.insert(table.clone(), targets);
    }

    // Reachability: a table reaches `accounts` if it points at it, or at a table that does.
    let mut reaches: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    reaches.insert("accounts".to_string());
    loop {
        let before = reaches.len();
        for (table, targets) in &edges {
            if reaches.contains(table) {
                continue;
            }
            if targets.iter().any(|target| reaches.contains(target)) {
                reaches.insert(table.clone());
            }
        }
        if reaches.len() == before {
            break;
        }
    }

    let measured = columns
        .iter()
        .filter(|(table, column)| !reaches.contains(table) && is_account_spelling(column))
        .cloned()
        .collect();
    (measured, columns)
}

/// The spellings the schema actually uses. Missing one is how a leak survives a green gate:
/// `trust_levels.account` is a plain TEXT column with no `REFERENCES` clause, and it stayed green for as long as the
/// pattern only knew `account_id`. `created_by` and `*_pseud_id` are the same story two migrations later.
fn is_account_spelling(column: &str) -> bool {
    column == "account"
        || column == "account_id"
        || column == "created_by"
        || column.ends_with("_account")
        || column.ends_with("_account_id")
        || column.ends_with("pseud_id")
}

#[tokio::test]
async fn every_account_column_outside_the_fk_graph_is_classified() {
    let tdb = TestDb::connect_with_dir(
        "m45_65_identity_scan",
        &test_support::scratch_dir("m45_65_identity_scan"),
    )
    .await;
    if tdb.is_postgres() {
        // The scan reads `sqlite_master` and the `pragma_*` table functions, so it is a statement about the SQLite
        // schema only. The behaviour tests in this file run on both engines; this one announces why it does not.
        eprintln!("skipped on PostgreSQL: the identity scan reads the SQLite schema");
        return;
    }
    let (measured, schema_columns) = measured_account_columns(&tdb).await;

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

    // Stale means "the schema does not have this column" — a rename or a typo, where the treatment
    // silently stops being applied. It deliberately does NOT mean "the schema links it":
    // `payment_events.account_id` reaches `accounts` through its own foreign key, and the erasure
    // path still wants its treatment, so comparing classified against *measured* would have deleted
    // a decision that is doing work. Two different questions, two different sets.
    let stale: Vec<String> = classified
        .difference(&schema_columns)
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

    // M45-67: the list of works a reader wants more of. Added to this test rather than to a new one
    // because the question is the same one: does the generic treatment loop actually reach a table
    // that is not in the foreign-key graph? A classified column nobody exercises is a decision, not
    // a behaviour.
    exec(
        &tdb,
        "INSERT INTO external_seeds \
         (id, account_id, spec, title, source_key, source_url, provenance_json, created_at) \
         VALUES ('44444444-4444-4444-4444-444444444444', ?, 'A Gamer''s Guide', 'A Gamer''s Guide', \
                 'unknown', NULL, '{}', '2026-11-02T00:00:00Z')",
        "INSERT INTO external_seeds \
         (id, account_id, spec, title, source_key, source_url, provenance_json, created_at) \
         VALUES ('44444444-4444-4444-4444-444444444444', $1, 'A Gamer''s Guide', 'A Gamer''s Guide', \
                 'unknown', NULL, '{}', '2026-11-02T00:00:00Z')",
        &[account],
    )
    .await
    .expect("insert an external seed");

    assert_eq!(count(&tdb, "directory_votes", account).await, 1);
    assert_eq!(count(&tdb, "module_user_state", account).await, 1);
    assert_eq!(count(&tdb, "external_seeds", account).await, 1);

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

    assert_eq!(
        count(&tdb, "external_seeds", account).await,
        0,
        "a shelf is the most personal thing a reader pastes here: erasing the account must take it, \
         not leave the titles under an id that no longer resolves to anybody"
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

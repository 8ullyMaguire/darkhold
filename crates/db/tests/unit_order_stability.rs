//! Reading order is a promise, and `order_key` alone does not keep it.
//!
//! # The defect this file exists for
//!
//! `ORDER BY c.order_key ASC` was the whole ordering in `units_for_work`, and the
//! live catalogue says `order_key` is **not unique**: measured 2026-11-05 on the
//! work the estate bundle was rehearsed against
//! (`78eb8d57-e28f-49d8-b58e-ebe6181bac3b`),
//!
//! ```text
//! units 784 · distinct order_key 545
//! ```
//!
//! so 239 of its units share a key with a neighbour. Postgres promises no order it
//! was not given, which means the *reading* order of those ties was whatever the
//! planner happened to produce — the same defect that made two runs of the Python
//! bundle exporter emit different bytes (`cc8358…` vs `39646938…`) until it was
//! given `, u.id`.
//!
//! `units_for_work` feeds three surfaces — the reader page, narration, and every
//! export — so an unstable order there is three bugs wearing one coat. The export is
//! the one that fails loudly (two exports of one work must be byte-identical, gate
//! 24); the reader page is the one that fails quietly.
//!
//! # Why the assertion is a sort, not a golden list
//!
//! The fixture writes keys in a scattered order (`30, 10, 20, 10, 30, 20, 10, 20`)
//! and unit ids are UUIDs, so insertion order, key order and id order all disagree.
//! The expected sequence is computed by sorting on `(order_key, id)` — exactly the
//! clause under test — so the test fails if the clause is dropped and passes on both
//! engines when it is present.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` to exercise the PostgreSQL
//! arm, which is the one where the promise is weakest.

use darkhold_db::content::{create_unit, create_work, units_for_work};
use darkhold_db::identity::{create_account, create_pseud, AccountStatus};
use darkhold_db::Backend;
use darkhold_db::Database;
use darkhold_domain::policy::AgeState;
use darkhold_domain::WorkId;

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    let mut c = darkhold_db::DatabaseConfig::new(url);
    c.max_connections = 2;
    c.acquire_timeout = std::time::Duration::from_secs(60);
    c
}

/// A migrated database on the engine the environment names.
async fn connect(tag: &str) -> Database {
    let Ok(admin) = std::env::var("DARKHOLD_TEST_PG_URL") else {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "darkhold-unitorder-{tag}-{}-{}",
            uuid::Uuid::new_v4().simple(),
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let db = Database::connect(&make_config(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )))
        .await
        .expect("connect sqlite");
        db.migrate().await.expect("migrate");
        return db;
    };
    let name = format!("lh_unitorder_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&make_config(admin.clone()))
        .await
        .expect("connect admin");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(admin_db.postgres_pool().unwrap())
        .await
        .expect("create");
    let url = admin
        .rsplit_once('/')
        .map(|(p, _)| format!("{p}/{name}"))
        .unwrap();
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

/// A work to hang units on.
async fn work_fixture(db: &Database, tag: &str) -> WorkId {
    let account = create_account(
        db,
        &format!("{tag}@example.test"),
        AgeState::DeclaredAdult,
        AccountStatus::Active,
    )
    .await
    .expect("create the account");
    let pseud = create_pseud(db, account, &format!("{tag}-pseud"), "A Pseud")
        .await
        .expect("create the pseud");
    create_work(db, pseud, &format!("A {tag} work"), None)
        .await
        .expect("create the work")
        .id
}

/// Rewrite a unit's key. `create_unit` spaces keys by ten, so it never produces a
/// tie — which is why the live ties came from an importer rather than from the
/// editor, and why the fixture has to make them itself.
async fn set_key(db: &Database, unit: &str, key: i64) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("UPDATE units SET order_key = ? WHERE id = ?")
                .bind(key)
                .bind(unit)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("set the key");
        }
        Backend::Postgres => {
            sqlx::query("UPDATE units SET order_key = $1 WHERE id = $2::uuid")
                .bind(key)
                .bind(unit)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("set the key");
        }
    }
}

/// Eight units whose keys collide, described in the order the fixture writes them.
async fn tied_units(db: &Database, work: WorkId) -> Vec<(i64, String)> {
    let scatter = [30_i64, 10, 20, 10, 30, 20, 10, 20];
    let mut written = Vec::new();
    for (index, key) in scatter.iter().enumerate() {
        let unit = create_unit(db, work, &format!("chapter {}", index + 1))
            .await
            .expect("create the unit");
        set_key(db, &unit.id.to_string(), *key).await;
        written.push((*key, unit.id.to_string()));
    }
    written
}

#[tokio::test]
async fn tied_keys_are_ordered_by_key_then_id() {
    let db = connect("tied").await;
    let work = work_fixture(&db, "tied").await;
    let written = tied_units(&db, work).await;

    let returned: Vec<(i64, String)> = units_for_work(&db, work)
        .await
        .expect("read the units")
        .into_iter()
        .map(|unit| (unit.order_key, unit.id.to_string()))
        .collect();

    let mut expected = written.clone();
    expected.sort();

    assert_eq!(
        returned, expected,
        "units_for_work must order by (order_key, id); without the id tie-break the \
         order of tied keys is the planner's, not the work's"
    );
    assert_eq!(returned.len(), 8, "every unit comes back once");
}

#[tokio::test]
async fn the_same_work_reads_the_same_order_twice() {
    let db = connect("twice").await;
    let work = work_fixture(&db, "twice").await;
    tied_units(&db, work).await;

    let first: Vec<String> = units_for_work(&db, work)
        .await
        .expect("first read")
        .into_iter()
        .map(|unit| unit.id.to_string())
        .collect();
    let second: Vec<String> = units_for_work(&db, work)
        .await
        .expect("second read")
        .into_iter()
        .map(|unit| unit.id.to_string())
        .collect();

    assert_eq!(first, second, "two reads of one work are one order");
}

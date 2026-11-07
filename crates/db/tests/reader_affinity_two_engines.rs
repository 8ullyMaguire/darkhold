//! The reader-affinity sweep on whichever engine `DARKHOLD_TEST_PG_URL` names (M69-09).
//!
//! The sweep is one statement per signal plus an eight-column upsert, and every one of those is a place the two dialects
//! disagree: `CAST(… AS BIGINT)` for counts that are INT4 on PostgreSQL, an `ON CONFLICT` whose arms must have
//! compatible types, and a `LIMIT` bound as `?` on one engine and `$n` on the other. So it is run on both.
//!
//! Works are seeded as bare ids: `bookmarks.subject_id` is polymorphic text, `operator_affinities.work_id` carries no
//! foreign key and `work_tags` gained its columns by `ALTER` rather than a rebuild, so a work row is not needed to
//! exercise the joins. Arena ballots *do* reference works, so the arena signal is absent here — which is itself a useful
//! case: the score must come out of the two present signals reweighted, not diluted by a missing third.

use darkhold_db::{reader_affinity, Backend, Database};
use sqlx::Row as _;

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    let mut config = darkhold_db::DatabaseConfig::new(url);
    config.max_connections = 2;
    config.acquire_timeout = std::time::Duration::from_secs(60);
    config
}

async fn connect() -> Database {
    let Ok(admin) = std::env::var("DARKHOLD_TEST_PG_URL") else {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "darkhold-affinity-2e-{}-{}",
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
    let name = format!("lh_aff_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&make_config(admin.clone()))
        .await
        .expect("connect admin");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(admin_db.postgres_pool().expect("postgres handle"))
        .await
        .expect("create database");
    let url = admin
        .rsplit_once('/')
        .map(|(prefix, _)| format!("{prefix}/{name}"))
        .expect("a database url has a path");
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

const NOW: &str = "2026-11-07T00:00:00Z";
const READER: &str = "01955100-0000-4000-8000-000000a00001";
const OPERATOR: &str = "01955100-0000-4000-8000-000000a00002";

async fn exec(db: &Database, sqlite: &str, postgres: &str, binds: &[&str], ints: &[i64]) -> u64 {
    match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(sqlite);
            for bind in binds {
                q = q.bind(*bind);
            }
            for int in ints {
                q = q.bind(*int);
            }
            q.execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("execute")
                .rows_affected()
        }
        Backend::Postgres => {
            let mut q = sqlx::query(postgres);
            for bind in binds {
                q = q.bind(*bind);
            }
            for int in ints {
                q = q.bind(*int);
            }
            q.execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("execute")
                .rows_affected()
        }
    }
}

#[tokio::test]
async fn the_sweep_writes_one_row_per_reader_and_a_second_run_changes_nothing() {
    let db = connect().await;
    let db = &db;

    // Two accounts, one of which is the operator whose taste the score is measured against.
    for (id, email) in [(READER, "reader-2e@example.test"), (OPERATOR, "operator-2e@example.test")] {
        exec(
            db,
            "INSERT INTO accounts (id, email, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            "INSERT INTO accounts (id, email, created_at, updated_at) VALUES ($1::uuid, $2, $3, $3)",
            &[id, email, NOW],
            &[],
        )
        .await;
    }

    // A pseud and the two works it owns. PostgreSQL keeps the `work_tags → works` foreign key that the SQLite table
    // rebuild (0103) left behind, so the seed has to satisfy it on the engine that has it; the same rows are inserted on
    // both so the two runs are the same test.
    exec(
        db,
        "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
         VALUES ('01955100-0000-4000-8000-000000c00001', ?1, 'the-operator', 'The Operator', ?2, ?2)",
        "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
         VALUES ('01955100-0000-4000-8000-000000c00001'::uuid, $1::uuid, 'the-operator', 'The Operator', $2, $2)",
        &[OPERATOR, NOW],
        &[],
    )
    .await;
    for (work, title) in [
        ("01955100-0000-4000-8000-000000b00001", "The good one"),
        ("01955100-0000-4000-8000-000000b00002", "Another good one"),
    ] {
        exec(
            db,
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at)
             VALUES (?1, '01955100-0000-4000-8000-000000c00001', ?2, ?3, ?3)",
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at)
             VALUES ($1::uuid, '01955100-0000-4000-8000-000000c00001'::uuid, $2, $3, $3)",
            &[work, title, NOW],
            &[],
        )
        .await;
    }

    // The operator values two works that share a tag; the reader collected one of them.
    for work in ["01955100-0000-4000-8000-000000b00001", "01955100-0000-4000-8000-000000b00002"] {
        exec(
            db,
            // Bound text-first so the helper's (strings then integers) order matches the statement.
            "INSERT INTO operator_affinities (work_id, operator, set_at, affinity_bp, rationale)
             VALUES (?1, ?2, ?3, ?4, 'seed')",
            "INSERT INTO operator_affinities (work_id, operator, set_at, affinity_bp, rationale)
             VALUES ($1::uuid, $2::uuid, $3, $4, 'seed')",
            &[work, OPERATOR, NOW],
            &[9_000],
        )
        .await;
        exec(
            db,
            "INSERT INTO work_tags (work_id, node_id, added_at) VALUES (?1, 'n-angst', ?2)",
            "INSERT INTO work_tags (work_id, node_id, added_at) VALUES ($1::uuid, 'n-angst', $2)",
            &[work, NOW],
            &[],
        )
        .await;
    }
    exec(
        db,
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, note, is_public, created_at, updated_at)
         VALUES ('01955100-0000-4000-8000-000000d00001', ?1, 'work', '01955100-0000-4000-8000-000000b00001', '', 0, ?2, ?2)",
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, note, is_public, created_at, updated_at)
         VALUES ('01955100-0000-4000-8000-000000d00001'::uuid, $1::uuid, 'work', '01955100-0000-4000-8000-000000b00001', '', false, $2, $2)",
        &[READER, NOW],
        &[],
    )
    .await;

    // First sweep.
    let first = reader_affinity::recompute_all(db, Some(OPERATOR), NOW)
        .await
        .expect("sweep");
    assert_eq!(first.initialised, 1, "one candidate: {first:?}");
    assert_eq!(first.changed, 0);
    assert!(first.wrote());

    let score = reader_affinity::for_account(db, READER)
        .await
        .expect("read")
        .expect("a score");
    let bp = score["score_bp"].as_i64().expect("score_bp");
    assert!(bp > 0, "a reader who collected a valued work is not zero aligned: {score}");
    assert_eq!(
        score["signals"].as_i64(),
        Some(2),
        "favourite overlap and tag similarity, with the arena absent rather than zero: {score}"
    );
    assert_eq!(
        score["arena_agreement_bp"],
        serde_json::Value::Null,
        "an absent signal stores NULL, because 0 would be a measurement nobody made"
    );
    assert_eq!(score["evidence_bp"].as_i64(), Some(8_000), "0.5 + 0.3 of the total weight: {score}");

    // Second sweep over the same data: one row, no change.
    let second = reader_affinity::recompute_all(db, Some(OPERATOR), NOW)
        .await
        .expect("sweep");
    assert_eq!(second.initialised, 0);
    assert_eq!(
        second.changed, 0,
        "an upsert that reports a change every run is a sweep matching nothing: {second:?}"
    );
    assert_eq!(second.unchanged, 1);

    let ranked = reader_affinity::ranked(db, 10).await.expect("ranked");
    assert_eq!(ranked.len(), 1, "one reader is one row however often the sweep runs: {ranked:?}");
    assert_eq!(ranked[0]["account"], serde_json::json!(READER));

    // No operator means no score rather than a score of zero.
    let unscored = reader_affinity::recompute_all(db, None, NOW)
        .await
        .expect("sweep");
    assert_eq!(unscored.initialised, 0);
    assert_eq!(unscored.changed, 0);
    assert!(!unscored.wrote());
}

#[tokio::test]
async fn a_reader_never_swept_has_no_row_and_so_no_score() {
    let db = connect().await;
    let db = &db;

    assert_eq!(reader_affinity::for_account(db, READER).await.expect("read"), None);
    let ranked = reader_affinity::ranked(db, 10).await.expect("ranked");
    assert!(ranked.is_empty());

    // And the count of rows is a fact about the sweep, not about the readership: the table starts empty.
    let count: i64 = match db.backend() {
        Backend::Sqlite => sqlx::query("SELECT COUNT(*) FROM reader_affinity")
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count")
            .get::<i64, _>(0),
        Backend::Postgres => sqlx::query("SELECT CAST(COUNT(*) AS BIGINT) FROM reader_affinity")
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("count")
            .get::<i64, _>(0),
    };
    assert_eq!(count, 0);
}

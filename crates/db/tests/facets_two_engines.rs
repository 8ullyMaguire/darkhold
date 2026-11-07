//! `SUM()` widens on PostgreSQL and not on SQLite, so the facet scan decodes on one engine and 500s on the other.
//!
//! The live catalogue answered **every** request to `GET /api/v1/facets` with `500`:
//!
//! ```text
//! error occurred while decoding column 4: mismatched types;
//! Rust type `i64` (as SQL type `INT8`) is not compatible with SQL type `NUMERIC`
//! ```
//!
//! Column 4 is `COALESCE(SUM(r.word_count), 0) AS words`. On SQLite a sum of integers stays an INTEGER; on PostgreSQL summing a
//! `BIGINT` column yields `NUMERIC`, which sqlx refuses to decode into `i64` rather than widening silently. Every unit test passed
//! throughout, because every app-level facet test pins the SQLite backend — the failure existed only on the engine the product
//! actually runs.
//!
//! The statement now casts per backend (`::BIGINT` on PostgreSQL) and this file is what catches its removal. It runs on whichever
//! engine `DARKHOLD_TEST_PG_URL` names: SQLite when unset (where the cast is inert) and PostgreSQL when set (where it is the whole
//! point).
//!
//! ```text
//! DARKHOLD_TEST_PG_URL='postgres://darkhold:darkhold@thinkcentre:55433/postgres' \
//!   cargo test -p darkhold-db --test facets_two_engines
//! ```

use darkhold_db::facets::{all_facets, FacetFilter};
use darkhold_db::Database;

const ACCOUNT: &str = "01955100-0000-4000-8000-0000000a0001";
const PSEUD: &str = "01955100-0000-4000-8000-0000000b0001";
const MEASURED: &str = "01955100-0000-4000-8000-0000000c0001";
const UNMEASURED: &str = "01955100-0000-4000-8000-0000000c0002";

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    let mut config = darkhold_db::DatabaseConfig::new(url);
    config.max_connections = 2;
    config.acquire_timeout = std::time::Duration::from_secs(60);
    config
}

/// A migrated database on the engine the environment names.
///
/// PostgreSQL gets a database created for this call, because the shared `postgres` database accumulates applied migration
/// checksums and rejects a second run against it.
async fn connect() -> Database {
    let Ok(admin) = std::env::var("DARKHOLD_TEST_PG_URL") else {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "darkhold-facets-2e-{}-{}",
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
    let name = format!("lh_facets_{}", uuid::Uuid::new_v4().simple());
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

/// Insert one statement on whichever pool this database uses.
///
/// `?` on SQLite, `$n` on PostgreSQL. Written per call rather than through `Database::sql` because these are fixture statements, and
/// a fixture that goes through the production rewriting path would not prove the production statement parses.
async fn exec(db: &Database, sqlite: &str, postgres: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sqlite)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await
                .expect("fixture statement (sqlite)");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(postgres)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await
                .expect("fixture statement (postgres)");
        }
    }
}

async fn fixture(db: &Database) {
    // An account owns the pseud — `pseuds.account_id` is NOT NULL on both engines, and the fixture must be valid before it can
    // measure anything.
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at)
             VALUES ('{ACCOUNT}', 'facets-2e@example.invalid', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at)
             VALUES ('{ACCOUNT}', 'facets-2e@example.invalid', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    // A pseud and two works: one with a measured 12,000-word revision, one with a unit and no revision at all.
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}', '{ACCOUNT}', 'probe', 'probe', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{PSEUD}', '{ACCOUNT}', 'probe', 'probe', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;

    for (work, title) in [(MEASURED, "Measured"), (UNMEASURED, "Unmeasured")] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                    published_at, created_at, updated_at, version)
                 VALUES ('{work}','{PSEUD}','{title}','en','teen','public','published',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','2026-01-05T00:00:00Z',1)"
            ),
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, language, rating, visibility, lifecycle,
                                    published_at, created_at, updated_at, version)
                 VALUES ('{work}','{PSEUD}','{title}','en','teen','public','published',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','2026-01-05T00:00:00Z',1)"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO units (id, work_id, order_key, title, created_at, updated_at, version)
                 VALUES ('{work}', '{work}', 1, 'One', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
            ),
            &format!(
                "INSERT INTO units (id, work_id, order_key, title, created_at, updated_at, version)
                 VALUES ('{work}', '{work}', 1, 'One', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
            ),
        )
        .await;
    }

    // The measured work's revision. `word_count` is NOT NULL here, so "unmeasured" is a unit with no current revision.
    exec(
        db,
        &format!(
            "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html,
                                    plain_text, word_count, created_by_pseud_id, created_at)
             VALUES ('01955100-0000-4000-8000-0000000d0001','{MEASURED}',1,'{{}}','<p>x</p>','x',12000,'{PSEUD}',
                     '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html,
                                    plain_text, word_count, created_by_pseud_id, created_at)
             VALUES ('01955100-0000-4000-8000-0000000d0001','{MEASURED}',1,'{{}}','<p>x</p>','x',12000,'{PSEUD}',
                     '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    exec(
        db,
        &format!("UPDATE units SET current_revision_id = '01955100-0000-4000-8000-0000000d0001' WHERE id = '{MEASURED}'"),
        &format!("UPDATE units SET current_revision_id = '01955100-0000-4000-8000-0000000d0001' WHERE id = '{MEASURED}'"),
    )
    .await;
}

#[tokio::test]
async fn the_facet_scan_decodes_the_word_sum_on_this_engine() {
    let db = connect().await;
    fixture(&db).await;

    // No filters: this is the live request that returned 500, and it must answer four facets.
    let answers = all_facets(&db, &FacetFilter::default())
        .await
        .expect("the facet scan must decode on this engine");
    assert_eq!(answers.len(), 4, "length, status, rating, cadence");

    let length = answers
        .iter()
        .find(|answer| answer.facet == "length")
        .expect("a length facet");
    assert_eq!(
        length.known, 1,
        "one work has a measured length; the other is `unknown`, not zero: {length:?}"
    );
    assert_eq!(length.unknown, 1, "{length:?}");
    let measured: i64 = length.buckets.iter().map(|bucket| bucket.count).sum();
    assert_eq!(
        measured, 1,
        "the 12,000-word work lands in exactly one bucket"
    );
    assert!(
        length
            .buckets
            .iter()
            .any(|bucket| bucket.key == "5k_20k" && bucket.count == 1),
        "12,000 words is in 5k_20k: {:?}",
        length.buckets
    );

    // …and the same scan with a word window, whose bounds travel through the HAVING clause on the same sum expression.
    let windowed = all_facets(
        &db,
        &FacetFilter {
            min_words: Some(10_000),
            ..FacetFilter::default()
        },
    )
    .await
    .expect("a windowed facet scan must decode too");
    let length = windowed
        .iter()
        .find(|answer| answer.facet == "length")
        .expect("a length facet");
    assert_eq!(length.known, 1, "{length:?}");
}

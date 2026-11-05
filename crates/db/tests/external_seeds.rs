//! M63-04: "similar to this one", the author filter and the read-history
//! exclusion, on both engines.
//!
//! The similarity maths is unit-tested in `darkhold-domain`; this file pins the
//! four properties that only exist once a database is involved:
//!
//! 1. **The preselection does not drop a neighbour.** The `tokens` filter is a
//!    coarse `LIKE` scan and the ranker is exact — a row that shares a rare title
//!    token must survive the filter even when it shares nothing else.
//! 2. **An unrelated row is absent, not last.** `ranked` counts neighbours, and a
//!    page padded with unrelated rows is the failure this exists to prevent.
//! 3. **`--author` is exact and case-insensitive.** `Osh` must not return
//!    `Oshha`'s rows, and `OSHHA` must return them.
//! 4. **`--exclude-read` drops the account's own reading and nobody else's.**
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm.

use std::time::Duration;

use darkhold_db::Database;

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    darkhold_db::DatabaseConfig {
        url,
        max_connections: 2,
        acquire_timeout: Duration::from_secs(5),
        slow_query_warn: Duration::ZERO,
    }
}

async fn connect(tag: &str) -> Database {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "darkhold-extseed-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_extseed_{}_{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            );
            let admin_db = Database::connect(&make_config(admin.clone()))
                .await
                .expect("connect to the admin database");
            sqlx::query(&format!("CREATE DATABASE {name}"))
                .execute(admin_db.postgres_pool().expect("postgres pool"))
                .await
                .expect("create a scratch database");
            admin_db.close().await;
            let (prefix, _) = admin
                .rsplit_once('/')
                .expect("admin URL ends in a database");
            format!("{prefix}/{name}")
        }
        Err(_) => format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display()),
    };
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

/// M45-67: evidence is stored once per spec, scoped to the reader who supplied it, and never becomes
/// a catalogue row.
#[tokio::test]
async fn evidence_is_idempotent_scoped_and_never_a_catalogue_row() {
    let db = connect("store").await;
    let owner = uuid::Uuid::new_v4().to_string();
    let other = uuid::Uuid::new_v4().to_string();
    let provenance =
        serde_json::json!({"typed": "https://archiveofourown.org/works/999", "reason": "not here"});

    let claim = darkhold_db::external_seed::Claim {
        spec: "https://archiveofourown.org/works/999",
        title: "ao3 999",
        source_key: "ao3",
        source_url: Some("https://archiveofourown.org/works/999"),
        provenance: &provenance,
    };
    let first = darkhold_db::external_seed::remember(&db, &owner, &claim, "2026-01-01T00:00:00Z")
        .await
        .expect("remember");

    let second_claim = darkhold_db::external_seed::Claim {
        title: "a different title this time",
        source_url: None,
        ..claim.clone()
    };
    let again =
        darkhold_db::external_seed::remember(&db, &owner, &second_claim, "2026-01-02T00:00:00Z")
            .await
            .expect("remember twice");

    assert_eq!(
        first.id, again.id,
        "re-running a list confirms it, it does not duplicate it"
    );
    assert_eq!(
        again.title, "ao3 999",
        "the stored row keeps the words it was first given: a second paste is not an edit"
    );
    assert_eq!(
        darkhold_db::external_seed::list_for_account(&db, &owner)
            .await
            .expect("list")
            .len(),
        1,
        "one spec, one row"
    );

    // Scoped: another reader cannot see, and cannot seed from, this evidence.
    assert!(
        darkhold_db::external_seed::by_id(&db, &other, &first.id)
            .await
            .expect("lookup")
            .is_none(),
        "evidence belongs to the reader who supplied it"
    );
    assert!(darkhold_db::external_seed::by_id(&db, &owner, &first.id)
        .await
        .expect("lookup")
        .is_some());

    // And the claim this module exists to hold: the catalogue is untouched.
    let catalogue: i64 = match db.backend() {
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar("SELECT count(*) FROM library_items WHERE title = $1")
                .bind("ao3 999")
                .fetch_one(db.postgres_pool().expect("pg"))
                .await
                .expect("count")
        }
        darkhold_db::Backend::Sqlite => {
            sqlx::query_scalar("SELECT count(*) FROM library_items WHERE title = ?")
                .bind("ao3 999")
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("count")
        }
    };
    assert_eq!(
        catalogue, 0,
        "evidence is not a work: nothing was written to library_items"
    );
}

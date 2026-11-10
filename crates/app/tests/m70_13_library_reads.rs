//! The library reads that had no route (M70-13).
//!
//! Five store functions, five GETs: status counts, private tags, bookmark count, the shelves holding an item, and the
//! update checks since a time. Every one is account-scoped, and the test's job is to prove the scoping as much as the
//! data: a second account must see none of it.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{Backend, Database};
use test_support::{active_pseud_id, id, register, scratch_dir, TestClient, TestDb};

async fn exec(tdb: &TestDb, sqlite: &str, postgres: &str, binds: &[&str], ints: &[i64]) {
    match tdb.db().backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(sqlite);
            for b in binds {
                q = q.bind(*b);
            }
            for i in ints {
                q = q.bind(*i);
            }
            q.execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("seed row");
        }
        Backend::Postgres => {
            let mut q = sqlx::query(postgres);
            for b in binds {
                q = q.bind(*b);
            }
            for i in ints {
                q = q.bind(*i);
            }
            q.execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("seed row");
        }
    }
}

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

#[tokio::test]
async fn the_library_reads_answer_for_their_own_account_only() {
    let dir = scratch_dir("library-reads");
    let tdb = TestDb::connect_with_dir("m7013-library-reads", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    let mut reader = TestClient::new(build(&config, &db));
    let account = register(&mut reader, "library-reader@example.test", "libraryreader").await;
    let pseud = active_pseud_id(&mut reader, &account)
        .await
        .expect("active pseud");

    let work = id("library-reads-work");
    exec(
        &tdb,
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES (?, ?, 'Reads', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ($1::uuid, $2::uuid, 'Reads', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[&work, &pseud],
        &[],
    )
    .await;

    let item = id("library-reads-item");
    exec(
        &tdb,
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, source_url, created_at, updated_at, version)
         VALUES (?, ?, ?, 'manual', 'reads-1', 'An Item', 'https://example.invalid/i', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, source_url, created_at, updated_at, version)
         VALUES ($1::uuid, $2::uuid, $3::uuid, 'manual', 'reads-1', 'An Item', 'https://example.invalid/i', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
        &[&item, &account, &work],
        &[],
    )
    .await;

    // Two statuses, so the "zero statuses are still reported" rule has something to contrast with.
    for (n, status) in ["finished", "reading"].into_iter().enumerate() {
        let row = id(&format!("library-status-{n}"));
        let subject = id(&format!("library-subject-{n}"));
        exec(
            &tdb,
            "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, updated_at, version)
             VALUES (?, ?, 'work', ?, ?, '2026-01-02T00:00:00Z', 1)",
            "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, updated_at, version)
             VALUES ($1::uuid, $2::uuid, 'work', $3::uuid, $4, '2026-01-02T00:00:00Z'::timestamptz, 1)",
            &[&row, &account, &subject, status],
            &[],
        )
        .await;
    }

    // Private tags: two uses of one tag, one of another.
    for (n, tag) in ["comfort-read", "comfort-read", "wip"]
        .into_iter()
        .enumerate()
    {
        let row = id(&format!("library-tag-{n}"));
        let subject = id(&format!("library-tagged-{n}"));
        exec(
            &tdb,
            "INSERT INTO private_tags (id, account_id, subject_type, subject_id, tag, created_at)
             VALUES (?, ?, 'work', ?, ?, '2026-01-03T00:00:00Z')",
            "INSERT INTO private_tags (id, account_id, subject_type, subject_id, tag, created_at)
             VALUES ($1::uuid, $2::uuid, 'work', $3::uuid, $4, '2026-01-03T00:00:00Z')",
            &[&row, &account, &subject, tag],
            &[],
        )
        .await;
    }

    // A shelf holding the item, and one that does not.
    for (n, name) in ["Favourites", "To Reread"].into_iter().enumerate() {
        let shelf = id(&format!("library-shelf-{n}"));
        exec(
            &tdb,
            "INSERT INTO shelves (id, account_id, name, description, is_public, position, created_at, updated_at, version)
             VALUES (?, ?, ?, '', 0, ?, '2026-01-04T00:00:00Z', '2026-01-04T00:00:00Z', 1)",
            "INSERT INTO shelves (id, account_id, name, description, is_public, position, created_at, updated_at, version)
             VALUES ($1::uuid, $2::uuid, $3, '', FALSE, $4, '2026-01-04T00:00:00Z'::timestamptz, '2026-01-04T00:00:00Z'::timestamptz, 1)",
            &[&shelf, &account, name],
            &[n as i64],
        )
        .await;
        if n == 0 {
            let placement = id("library-placement");
            exec(
                &tdb,
                "INSERT INTO shelf_items (id, shelf_id, library_item_id, position, created_at)
                 VALUES (?, ?, ?, 0, '2026-01-04T00:00:00Z')",
                "INSERT INTO shelf_items (id, shelf_id, library_item_id, position, created_at)
                 VALUES ($1::uuid, $2::uuid, $3::uuid, 0, '2026-01-04T00:00:00Z'::timestamptz)",
                &[&placement, &shelf, &item],
                &[],
            )
            .await;
        }
    }

    // Two update checks, one before and one after the `since` the test will ask for.
    for (n, at, changes) in [
        ("old", "2026-01-05T00:00:00Z", 0i64),
        ("new", "2026-01-09T00:00:00Z", 3i64),
    ] {
        let check = id(&format!("library-check-{n}"));
        exec(
            &tdb,
            "INSERT INTO update_checks (id, account_id, library_item_id, checked_at, found_changes, report_json)
             VALUES (?, ?, ?, ?, ?, '{\"changed\":[\"chapter 3\"]}')",
            "INSERT INTO update_checks (id, account_id, library_item_id, checked_at, found_changes, report_json)
             VALUES ($1::uuid, $2::uuid, $3::uuid, $4, $5, '{\"changed\":[\"chapter 3\"]}')",
            &[&check, &account, &item, at],
            &[changes],
        )
        .await;
    }

    // 1. Status counts: both statuses, and every zero one reported too.
    let (status, body) = reader.get("/api/v1/library/status-counts").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["counts"]["finished"], serde_json::json!(1), "{body}");
    assert_eq!(body["counts"]["reading"], serde_json::json!(1), "{body}");
    assert_eq!(body["counts"]["dropped"], serde_json::json!(0), "{body}");
    assert_eq!(body["counts"]["on-hold"], serde_json::json!(0), "{body}");
    assert_eq!(body["total"], serde_json::json!(2), "{body}");

    // 2. Tags, with counts.
    let (status, body) = reader.get("/api/v1/library/tags").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let tags = body["tags"].as_array().expect("array");
    assert_eq!(tags.len(), 2, "{body}");
    assert_eq!(tags[0]["tag"], serde_json::json!("comfort-read"), "{body}");
    assert_eq!(tags[0]["count"], serde_json::json!(2), "{body}");
    assert_eq!(tags[1]["tag"], serde_json::json!("wip"), "{body}");

    // 3. Shelf membership is a set of names, not a boolean.
    let (status, body) = reader
        .get(format!("/api/v1/library/items/{item}/shelves"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["shelves"], serde_json::json!(["Favourites"]), "{body}");

    // 4. Update checks since a time: the later one only.
    let (status, body) = reader
        .get("/api/v1/library/updates?since=2026-01-07T00:00:00Z")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let checks = body["checks"].as_array().expect("array");
    assert_eq!(checks.len(), 1, "the older check is before `since`: {body}");
    assert_eq!(checks[0]["found_changes"], serde_json::json!(3), "{body}");
    assert_eq!(
        checks[0]["report"]["changed"],
        serde_json::json!(["chapter 3"]),
        "the stored report is parsed, not passed through as a string: {body}"
    );

    // 5. `since` is required: an unbounded history is a different question.
    let (status, body) = reader.get("/api/v1/library/updates").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["since"],
        serde_json::json!("an RFC 3339 timestamp"),
        "{body}"
    );

    // 6. Another account sees none of it, in every one of the five reads.
    let mut other = TestClient::new(build(&config, &db));
    register(&mut other, "library-other@example.test", "libraryother").await;
    let (_, body) = other.get("/api/v1/library/status-counts").await;
    assert_eq!(body["total"], serde_json::json!(0), "{body}");
    let (_, body) = other.get("/api/v1/library/tags").await;
    assert_eq!(body["tags"].as_array().map(Vec::len), Some(0), "{body}");
    let (_, body) = other
        .get(format!("/api/v1/library/items/{item}/shelves"))
        .await;
    assert_eq!(body["shelves"].as_array().map(Vec::len), Some(0), "{body}");
    let (_, body) = other
        .get("/api/v1/library/updates?since=2026-01-01T00:00:00Z")
        .await;
    assert_eq!(
        body["checks"].as_array().map(Vec::len),
        Some(0),
        "another account's update checks must not appear: {body}"
    );
    let (_, body) = other.get("/api/v1/bookmarks/count").await;
    assert_eq!(body["count"], serde_json::json!(0), "{body}");
}

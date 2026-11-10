//! The last two library reads (M70-13): a public bookmark list, and one item's latest change check.
//!
//! Both are questions the store already knew how to answer and no door asked:
//!
//! * `public_bookmarks_for` is the only query in `library` that takes no `account_id`, because it serves other readers.
//!   The store row carries the owner's account id and their **private note**; a bookmark being public does not publish
//!   either, so the door projects the fact of the bookmark and never the reader behind it.
//! * `latest_update_check` answers "what did the last check of this item find", which is a different question from
//!   `update_checks_since` ("everything that moved since T"). Never checked is `null`, not an empty object: "we have
//!   never looked" and "we looked and found nothing" are different facts.

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
                .unwrap_or_else(|error| panic!("seed row failed: {error}\nSQL: {sqlite}"));
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
                .unwrap_or_else(|error| panic!("seed row failed: {error}\nSQL: {postgres}"));
        }
    }
}

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

/// One bookmark, public or private. `is_public` is 1/0 on SQLite and a boolean on PostgreSQL — the column is a real
/// boolean there, which is why `BOOKMARK_COLUMNS_PG` casts it with `is_public::int::bigint`.
async fn seed_bookmark(
    tdb: &TestDb,
    bookmark: &str,
    account: &str,
    work: &str,
    note: &str,
    is_public: bool,
) {
    let sqlite = if is_public {
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, unit_id, position_permille, note, is_public, created_at, updated_at, version)
         VALUES (?, ?, 'work', ?, NULL, 412, ?, 1, '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', 1)"
    } else {
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, unit_id, position_permille, note, is_public, created_at, updated_at, version)
         VALUES (?, ?, 'work', ?, NULL, 412, ?, 0, '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', 1)"
    };
    let postgres = if is_public {
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, unit_id, position_permille, note, is_public, created_at, updated_at, version)
         VALUES ($1::uuid, $2::uuid, 'work', $3::uuid, NULL, 412, $4, TRUE, '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', 1)"
    } else {
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, unit_id, position_permille, note, is_public, created_at, updated_at, version)
         VALUES ($1::uuid, $2::uuid, 'work', $3::uuid, NULL, 412, $4, FALSE, '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', 1)"
    };
    exec(tdb, sqlite, postgres, &[bookmark, account, work, note], &[]).await;
}

async fn seed_library_item(tdb: &TestDb, item: &str, account: &str, work: &str) {
    exec(
        tdb,
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, source_url, created_at, updated_at, version)
         VALUES (?, ?, ?, 'manual', 'pub-1', 'An Item', 'https://example.invalid/p', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
        "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, source_url, created_at, updated_at, version)
         VALUES ($1::uuid, $2::uuid, $3::uuid, 'manual', 'pub-1', 'An Item', 'https://example.invalid/p', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
        &[item, account, work],
        &[],
    )
    .await;
}

#[tokio::test]
async fn a_public_bookmark_list_names_no_reader_and_the_latest_item_check_is_the_latest() {
    let dir = scratch_dir("library-public");
    let tdb = TestDb::connect_with_dir("m7013-library-public", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    let mut owner = TestClient::new(build(&config, &db));
    let account = register(&mut owner, "pub-owner@example.test", "pubowner").await;
    let pseud = active_pseud_id(&mut owner, &account)
        .await
        .expect("active pseud");
    let work = id("pub-work");
    exec(
        &tdb,
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES (?, ?, 'A Public Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ($1::uuid, $2::uuid, 'A Public Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        &[&work, &pseud],
        &[],
    )
    .await;

    // One public bookmark, one private, both by the same reader.
    seed_bookmark(
        &tdb,
        &id("pub-bm-open"),
        &account,
        &work,
        "a note nobody else may read",
        true,
    )
    .await;
    seed_bookmark(
        &tdb,
        &id("pub-bm-closed"),
        &account,
        &work,
        "another private note",
        false,
    )
    .await;

    // 1. An ANONYMOUS reader gets the public one, and no session is required.
    let mut stranger = TestClient::new(build(&config, &db));
    let (status, body) = stranger
        .get(format!("/api/v1/works/{work}/bookmarks/public"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], serde_json::json!(1), "{body}");
    let listed = body["bookmarks"].as_array().expect("bookmarks");
    assert_eq!(listed.len(), 1, "{body}");
    assert_eq!(
        listed[0]["position_permille"],
        serde_json::json!(412),
        "{body}"
    );
    assert_eq!(
        listed[0]["created_at"],
        serde_json::json!("2026-01-02T00:00:00Z"),
        "both engines answer RFC 3339 for a stored timestamp: {body}"
    );

    // 2. Neither the reader's account id, nor either note, appears anywhere in the answer. This is the assertion the
    //    store's doc comment is about: the row carries both, and the door must not pass them on.
    let text = body.to_string();
    assert!(
        !text.contains(&account),
        "the owner's account id leaked: {text}"
    );
    assert!(
        !text.contains("a note nobody else may read"),
        "a private note leaked: {text}"
    );
    assert!(
        !text.contains("another private note"),
        "a private note leaked from a bookmark excluded by is_public: {text}"
    );

    // 3. An item nobody has checked answers null, not an empty report.
    let item = id("pub-item");
    seed_library_item(&tdb, &item, &account, &work).await;
    let (status, body) = owner
        .get(format!("/api/v1/library/items/{item}/updates"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["last_check"].is_null(),
        "never checked is null: {body}"
    );

    // 4. Two checks, seeded out of order: the later one answers, and its report is parsed rather than passed through.
    let older = id("pub-check-old");
    let newer = id("pub-check-new");
    exec(
        &tdb,
        "INSERT INTO update_checks (id, account_id, library_item_id, checked_at, found_changes, report_json)
         VALUES (?, ?, ?, '2026-01-03T00:00:00Z', 2, '{\"added\": 2}')",
        "INSERT INTO update_checks (id, account_id, library_item_id, checked_at, found_changes, report_json)
         VALUES ($1::uuid, $2::uuid, $3::uuid, '2026-01-03T00:00:00Z', 2, '{\"added\": 2}')",
        &[&older, &account, &item],
        &[],
    )
    .await;
    exec(
        &tdb,
        "INSERT INTO update_checks (id, account_id, library_item_id, checked_at, found_changes, report_json)
         VALUES (?, ?, ?, '2026-01-09T00:00:00Z', 5, '{\"added\": 5}')",
        "INSERT INTO update_checks (id, account_id, library_item_id, checked_at, found_changes, report_json)
         VALUES ($1::uuid, $2::uuid, $3::uuid, '2026-01-09T00:00:00Z', 5, '{\"added\": 5}')",
        &[&newer, &account, &item],
        &[],
    )
    .await;

    let (status, body) = owner
        .get(format!("/api/v1/library/items/{item}/updates"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["last_check"]["id"], serde_json::json!(newer), "{body}");
    assert_eq!(
        body["last_check"]["found_changes"],
        serde_json::json!(5),
        "{body}"
    );
    assert_eq!(
        body["last_check"]["checked_at"],
        serde_json::json!("2026-01-09T00:00:00Z"),
        "{body}"
    );
    assert_eq!(
        body["last_check"]["report"],
        serde_json::json!({"added": 5}),
        "the stored report is JSON, so it comes back as JSON: {body}"
    );

    // 5. Another account asking about this item's checks gets null — the query is scoped by account, not by item.
    let mut other = TestClient::new(build(&config, &db));
    register(&mut other, "pub-other@example.test", "pubother").await;
    let (status, body) = other
        .get(format!("/api/v1/library/items/{item}/updates"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["last_check"].is_null(),
        "one account's checks are not another's: {body}"
    );
}

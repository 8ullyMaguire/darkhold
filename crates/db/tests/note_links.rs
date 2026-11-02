//! M45-38 — author-note links are recorded, checked and surfaced, on both engines.
//!
//! The gaps review (D3) says note links rot like any other link. What can go wrong quietly is what
//! these tests are about:
//!
//! 1. **A note with no link records nothing**, so most saves cost nothing.
//! 2. **Prose punctuation is not part of the URL**, so a working link is not stored as a broken one.
//! 3. **Recording is idempotent per (revision, url)** — a retried save must not reset a dead link's
//!    health, which is the failure that would make the whole monitor useless.
//! 4. **A superseded revision's links are still stored but no longer listed as live**, so a note the
//!    author already fixed stops being reported.
//! 5. **`mark_checked` keeps `last_healthy_at`** across a failure: "dead since Tuesday, last answered
//!    in March" is the sentence a reader needs, and clearing it loses the second half.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm.

use std::time::Duration;

use darkhold_db::note_links as store;
use darkhold_db::Database;
use darkhold_domain::media_resilience::LinkStatus;

const NOW_STAMP: &str = "2026-11-02T10:00:00Z";

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
        "darkhold-nl-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4().simple(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_nl_{}_{}",
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

async fn exec(db: &Database, query: &str) {
    let q = db.sql(query, query);
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&q)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("exec");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&q)
                .execute(db.postgres_pool().expect("pg"))
                .await
                .expect("exec");
        }
    }
}

/// A work with one unit and one revision; returns (work, unit, revision).
async fn a_work_with_a_note(db: &Database, revision_number: i64) -> (String, String, String) {
    let account = uuid::Uuid::new_v4().to_string();
    let pseud = uuid::Uuid::new_v4().to_string();
    let work = uuid::Uuid::new_v4().to_string();
    let unit = uuid::Uuid::new_v4().to_string();
    let revision = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
             VALUES ('{account}', '{account}@example.test', 'active', 'adult', '{NOW_STAMP}', '{NOW_STAMP}', 'minimal')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
             VALUES ('{pseud}', '{account}', 'nl-{}', 'NL', '{NOW_STAMP}', '{NOW_STAMP}')",
            &pseud[..8]
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, created_at, updated_at)
             VALUES ('{work}', '{pseud}', 'A Work', 'published', 'public', '{NOW_STAMP}', '{NOW_STAMP}')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO units (id, work_id, order_key, title, created_at, updated_at)
             VALUES ('{unit}', '{work}', 10, 'Chapter', '{NOW_STAMP}', '{NOW_STAMP}')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO revisions
                 (id, unit_id, revision_number, document_json, sanitized_html, plain_text,
                  word_count, note, created_by_pseud_id, created_at)
             VALUES ('{revision}', '{unit}', {revision_number}, '{{}}', '<p>x</p>', 'x', 1, NULL, '{pseud}', '{NOW_STAMP}')"
        ),
    )
    .await;
    (work, unit, revision)
}

/// Point the unit at a revision, the way a save does.
async fn publish(db: &Database, unit: &str, revision: &str) {
    exec(
        db,
        &format!("UPDATE units SET current_revision_id = '{revision}' WHERE id = '{unit}'"),
    )
    .await;
}

#[tokio::test]
async fn a_note_without_links_records_nothing() {
    let db = connect("empty").await;
    let (work, unit, revision) = a_work_with_a_note(&db, 1).await;
    let created =
        store::record_for_revision(&db, &work, &unit, &revision, Some("No links here at all."))
            .await
            .expect("record");
    assert_eq!(created, 0);
    assert!(store::list_for_work(&db, &work)
        .await
        .expect("list")
        .is_empty());
}

#[tokio::test]
async fn a_notes_links_are_stored_with_their_host_and_provider() {
    let db = connect("stored").await;
    let (work, unit, revision) = a_work_with_a_note(&db, 1).await;
    publish(&db, &unit, &revision).await;
    let created = store::record_for_revision(
        &db,
        &work,
        &unit,
        &revision,
        // Prose punctuation around the first URL, and a Tumblr link inside a sentence.
        Some("(see https://example.test/meta). Also https://blog.tumblr.com/post/1, thanks!"),
    )
    .await
    .expect("record");
    assert_eq!(created, 2, "two links, and the prose punctuation trimmed");

    let links = store::list_for_work(&db, &work).await.expect("list");
    assert_eq!(links.len(), 2);
    let example = links
        .iter()
        .find(|link| link.host == "example.test")
        .expect("the example host");
    assert_eq!(example.url, "https://example.test/meta");
    assert_eq!(example.provider, "other");
    assert_eq!(example.status, "pending_verification");
    let tumblr = links
        .iter()
        .find(|link| link.host == "blog.tumblr.com")
        .expect("the tumblr host");
    assert_eq!(tumblr.provider, "tumblr");
}

#[tokio::test]
async fn recording_the_same_revision_twice_does_not_reset_a_dead_link() {
    let db = connect("idempotent").await;
    let (work, unit, revision) = a_work_with_a_note(&db, 1).await;
    publish(&db, &unit, &revision).await;
    store::record_for_revision(&db, &work, &unit, &revision, Some("https://gone.test/x"))
        .await
        .expect("record");
    let link = store::list_for_work(&db, &work)
        .await
        .expect("list")
        .remove(0);
    store::mark_checked(&db, &link.id, LinkStatus::Dead, 1, Some("404"))
        .await
        .expect("check");

    let created =
        store::record_for_revision(&db, &work, &unit, &revision, Some("https://gone.test/x"))
            .await
            .expect("re-record");
    assert_eq!(created, 0, "the row already exists");

    let after = store::list_for_work(&db, &work).await.expect("list");
    assert_eq!(after[0].status, "dead", "a retried save did not freshen it");
    assert_eq!(after[0].consecutive_failures, 1);
}

#[tokio::test]
async fn a_superseded_revision_stops_being_live_but_is_still_recorded() {
    let db = connect("superseded").await;
    let (work, unit, first) = a_work_with_a_note(&db, 1).await;
    publish(&db, &unit, &first).await;
    store::record_for_revision(&db, &work, &unit, &first, Some("https://old.test/x"))
        .await
        .expect("record");
    let second = uuid::Uuid::new_v4().to_string();
    exec(
        &db,
        &format!(
            "INSERT INTO revisions
                 (id, unit_id, revision_number, document_json, sanitized_html, plain_text,
                  word_count, note, created_by_pseud_id, created_at)
             SELECT '{second}', unit_id, 2, document_json, sanitized_html, plain_text, word_count,
                    NULL, created_by_pseud_id, created_at
               FROM revisions WHERE id = '{first}'"
        ),
    )
    .await;
    publish(&db, &unit, &second).await;
    store::record_for_revision(&db, &work, &unit, &second, Some("https://new.test/x"))
        .await
        .expect("record");

    let all = store::list_for_work(&db, &work).await.expect("all");
    assert_eq!(all.len(), 2, "the old revision's link is still stored");

    let live = store::list_for_live_revisions(&db, &work)
        .await
        .expect("live");
    assert_eq!(
        live.len(),
        1,
        "but only the new revision's link is what the work says"
    );
    assert_eq!(live[0].url, "https://new.test/x");
}

#[tokio::test]
async fn a_failed_check_keeps_the_last_time_the_link_answered() {
    let db = connect("healthy").await;
    let (work, unit, revision) = a_work_with_a_note(&db, 1).await;
    publish(&db, &unit, &revision).await;
    store::record_for_revision(&db, &work, &unit, &revision, Some("https://flaky.test/x"))
        .await
        .expect("record");
    let link = store::list_for_work(&db, &work)
        .await
        .expect("list")
        .remove(0);
    store::mark_checked(&db, &link.id, LinkStatus::Healthy, 0, None)
        .await
        .expect("healthy");
    let healthy = store::list_for_work(&db, &work)
        .await
        .expect("list")
        .remove(0);
    assert!(
        healthy.last_healthy_at.is_some(),
        "the healthy check stamped it"
    );
    assert_eq!(healthy.consecutive_failures, 0);

    store::mark_checked(&db, &link.id, LinkStatus::Dead, 3, Some("timeout"))
        .await
        .expect("dead");
    let dead = store::list_for_work(&db, &work)
        .await
        .expect("list")
        .remove(0);
    assert_eq!(dead.status, "dead");
    assert_eq!(dead.consecutive_failures, 3);
    assert_eq!(
        dead.last_healthy_at, healthy.last_healthy_at,
        "the last time it answered survives the failure"
    );
    assert!(dead.is_dead());
    assert_eq!(
        store::count_dead_for_work(&db, &work).await.expect("count"),
        1
    );
}

#[tokio::test]
async fn the_checker_takes_never_checked_links_first() {
    let db = connect("due").await;
    let (work, unit, revision) = a_work_with_a_note(&db, 1).await;
    publish(&db, &unit, &revision).await;
    store::record_for_revision(
        &db,
        &work,
        &unit,
        &revision,
        Some("https://a.test/1 https://b.test/2 https://c.test/3"),
    )
    .await
    .expect("record");
    let mut links = store::list_for_work(&db, &work).await.expect("list");
    links.sort_by(|left, right| left.url.cmp(&right.url));
    // Check the middle one, so one row is known-checked and two are not.
    store::mark_checked(&db, &links[1].id, LinkStatus::Healthy, 0, None)
        .await
        .expect("check");

    let due = store::find_needing_check(&db, 10).await.expect("due");
    assert_eq!(due.len(), 3);
    assert_eq!(
        due.last().map(|link| link.url.as_str()),
        Some("https://b.test/2"),
        "the checked link sorts last, never-checked rows first"
    );
}

//! M69-05 — work mirrors through the store, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! Four claims:
//!
//! * one row per (work, peer), and re-recording **replaces** rather than adding a second — otherwise "is this work
//!   mirrored?" is a sum and the second row is the one nobody reads;
//! * the similarity stored is the number the decision was made on, so "why is this mirrored there?" has an answer that does
//!   not drift with the peer's taste;
//! * retiring keeps the row: a reader who saw the line yesterday is owed an explanation;
//! * recording again after a retirement **re-arms** it — a page still saying "withdrawn" would be wrong in the direction
//!   that looks like a bug.

use darkhold_db::{work_mirrors, Database};

const NOW: &str = "2026-11-07T00:00:00Z";

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
            "darkhold-mirror-{}-{}",
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
    let name = format!("lh_mirror_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&make_config(admin.clone()))
        .await
        .expect("admin");
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

#[tokio::test]
async fn recording_replaces_and_retiring_keeps_the_row() {
    let db = connect().await;
    let db = &db;

    let first = work_mirrors::record(
        db,
        "work-1",
        "https://peer.example",
        "https://peer.example/works/storm",
        7200,
        NOW,
    )
    .await
    .expect("record");
    assert_eq!(first.state, "active");
    assert_eq!(first.similarity_bp, 7200);
    assert_eq!(
        work_mirrors::active_count(db, "work-1")
            .await
            .expect("count"),
        1
    );

    // Re-recording replaces: same row, new address and number.
    let again = work_mirrors::record(
        db,
        "work-1",
        "https://peer.example",
        "https://peer.example/works/the-storm",
        8100,
        "2026-11-08T00:00:00Z",
    )
    .await
    .expect("record again");
    assert_eq!(again.id, first.id, "one identity is one row");
    assert_eq!(
        again.similarity_bp, 8100,
        "the number is the one the decision was made on"
    );
    assert_eq!(again.peer_url, "https://peer.example/works/the-storm");
    assert_eq!(
        work_mirrors::for_work(db, "work-1")
            .await
            .expect("list")
            .len(),
        1
    );

    // A second peer is a second row.
    work_mirrors::record(db, "work-1", "https://other.example", "", 6500, NOW)
        .await
        .expect("record other");
    assert_eq!(
        work_mirrors::active_count(db, "work-1")
            .await
            .expect("count"),
        2
    );

    // A retirement needs a reason and keeps the row.
    let error = work_mirrors::retire(db, &again.id, "lost", "  ", NOW)
        .await
        .expect_err("a retirement without a reason is not a retirement");
    assert!(error.to_string().contains("reason"), "{error}");
    assert!(own_retire(db, &again.id).await, "the mirror was retired");
    assert_eq!(
        work_mirrors::active_count(db, "work-1")
            .await
            .expect("count"),
        1
    );
    let rows = work_mirrors::for_work(db, "work-1").await.expect("list");
    assert_eq!(rows.len(), 2, "the retired row stays");
    let retired = rows
        .iter()
        .find(|row| row.id == again.id)
        .expect("the retired row");
    assert_eq!(retired.state, "lost");
    assert!(retired.retire_reason.is_some());
    assert_eq!(
        rows[0].state, "active",
        "the page shows the live one first: {rows:?}"
    );

    // Recording again re-arms it.
    let re_armed = work_mirrors::record(
        db,
        "work-1",
        "https://peer.example",
        "https://peer.example/works/the-storm",
        7000,
        "2026-11-09T00:00:00Z",
    )
    .await
    .expect("re-arm");
    assert_eq!(re_armed.state, "active");
    assert!(re_armed.retired_at.is_none(), "re-armed means not retired");
    assert!(re_armed.retire_reason.is_none());
    assert_eq!(
        work_mirrors::active_count(db, "work-1")
            .await
            .expect("count"),
        2
    );
}

async fn own_retire(db: &Database, id: &str) -> bool {
    work_mirrors::retire(db, id, "lost", "the peer stopped serving it", NOW)
        .await
        .expect("retire")
}

#[tokio::test]
async fn a_similarity_outside_basis_points_is_refused_and_an_unknown_state_is_not_a_retirement() {
    let db = connect().await;
    let db = &db;
    let error = work_mirrors::record(db, "work-2", "https://peer.example", "", 10_500, NOW)
        .await
        .expect_err("10500 is not a similarity");
    assert!(error.to_string().contains("10000"), "{error}");

    let mirror = work_mirrors::record(db, "work-2", "https://peer.example", "", 5000, NOW)
        .await
        .expect("record");
    let error = work_mirrors::retire(db, &mirror.id, "active", "nothing happened", NOW)
        .await
        .expect_err("'active' is not a retirement state");
    assert!(error.to_string().contains("lost"), "{error}");
    assert!(
        !work_mirrors::retire(db, "no-such-id", "withdrawn", "never existed", NOW)
            .await
            .expect("retire unknown"),
        "retiring something that is not there reports that nothing changed"
    );
}

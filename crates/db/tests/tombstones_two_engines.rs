//! M69-04 — tombstones through the store, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! Four claims:
//!
//! * a filed tombstone refuses the work, and the refusal **cites** the tombstone and its reason;
//! * a different author is a different work — an empty author is not a wildcard;
//! * the refusal is **counted**, because a tombstone that has stopped nothing is a mistyped title and an operator cannot
//!   tell the two apart otherwise;
//! * a withdrawal stops the refusal and keeps the row, so "why did my work vanish?" has an answer.

use darkhold_db::{tombstones, Database};

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
            "darkhold-tomb-{}-{}",
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
    let name = format!("lh_tomb_{}", uuid::Uuid::new_v4().simple());
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
async fn a_tombstone_refuses_the_work_cites_itself_and_counts_the_refusal() {
    let db = connect().await;
    let db = &db;

    let filed = tombstones::file(
        db,
        &tombstones::Filing {
            title: "Vampiric Concerto",
            author: "ashenthea",
            language: None,
            reason: "author request",
            origin_url: Some("https://archiveofourown.org/works/92356871"),
            dmca_id: None,
            filed_by: "operator",
            note: "asked by the author on 2026-11-06",
            now: NOW,
        },
    )
    .await
    .expect("file");
    assert_eq!(filed.refusals, 0);
    assert_eq!(filed.identity.len(), 32);

    // The same work, as another host would present it: different case, extra punctuation, a trailing space.
    let refusal = tombstones::refuse(db, "  vampiric CONCERTO ", "Ashenthea", None, NOW)
        .await
        .expect("refuse")
        .expect("a refusal, because identity is not the URL");
    assert_eq!(
        refusal.tombstone, filed.id,
        "the refusal cites the tombstone"
    );
    assert_eq!(refusal.reason, "author request");
    assert_eq!(refusal.filed_at, NOW);

    // Counted, so an operator can see the tombstone works.
    let listed = tombstones::list(db, 10).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].refusals, 1);
    assert_eq!(listed[0].last_refused_at.as_deref(), Some(NOW));

    // A different author is a different work.
    assert!(
        tombstones::refuse(db, "Vampiric Concerto", "someone-else", None, NOW)
            .await
            .expect("refuse")
            .is_none()
    );
    // An untitled work cannot be keyed, so it is not refused either.
    assert!(tombstones::refuse(db, "", "ashenthea", None, NOW)
        .await
        .expect("refuse")
        .is_none());
}

#[tokio::test]
async fn withdrawal_stops_the_refusal_and_keeps_the_row_and_re_filing_re_arms_it() {
    let db = connect().await;
    let db = &db;

    let filed = tombstones::file(
        db,
        &tombstones::Filing {
            title: "The Storm",
            author: "ada",
            language: Some("en"),
            reason: "dmca",
            origin_url: None,
            dmca_id: Some("dmca-42"),
            filed_by: "operator",
            note: "",
            now: NOW,
        },
    )
    .await
    .expect("file");

    // An empty reason is refused: the row is the answer to "why did my work vanish?".
    let error = tombstones::withdraw(db, &filed.id, "operator", "", NOW)
        .await
        .expect_err("a withdrawal without a reason is not a withdrawal");
    assert!(error.to_string().contains("reason"), "{error}");

    assert!(tombstones::withdraw(
        db,
        &filed.id,
        "operator",
        "filed against the wrong work",
        NOW
    )
    .await
    .expect("withdraw"));
    assert!(
        !tombstones::withdraw(db, &filed.id, "operator", "again", NOW)
            .await
            .expect("withdraw again"),
        "a second withdrawal is a no-op, not a claim that something was withdrawn"
    );

    // Withdrawn: no refusal, and the row still explains itself.
    assert!(tombstones::refuse(db, "The Storm", "ada", Some("en"), NOW)
        .await
        .expect("refuse")
        .is_none());
    let listed = tombstones::list(db, 10).await.expect("list");
    assert_eq!(
        listed.len(),
        1,
        "the row stays: deleting it would answer the author with silence"
    );
    assert_eq!(
        listed[0].withdraw_reason.as_deref(),
        Some("filed against the wrong work")
    );
    assert_eq!(listed[0].refusals, 0);

    // Re-filing after a withdrawal re-arms it, and keeps the paperwork reference.
    let re_armed = tombstones::file(
        db,
        &tombstones::Filing {
            title: "The Storm",
            author: "ada",
            language: Some("en"),
            reason: "dmca",
            origin_url: None,
            dmca_id: None,
            filed_by: "operator",
            note: "the author complained again",
            now: "2026-11-08T00:00:00Z",
        },
    )
    .await
    .expect("re-file");
    assert_eq!(re_armed.id, filed.id, "one identity is one row");
    assert!(
        re_armed.withdrawn_at.is_none(),
        "re-filed means active again"
    );
    assert_eq!(
        re_armed.dmca_id.as_deref(),
        Some("dmca-42"),
        "a re-file without paperwork keeps the paperwork it had"
    );
    assert!(tombstones::refuse(db, "The Storm", "ada", Some("en"), NOW)
        .await
        .expect("refuse")
        .is_some());
}

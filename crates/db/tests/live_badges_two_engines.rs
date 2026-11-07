//! M69-14 — the live badge's state machine through the store, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! Three claims, each with its own assertion, because each is a way a badge can lie:
//!
//! * a gain writes `held` and a `since`;
//! * a recompute that changes nothing writes **no event** (a row per sweep is a log of the scheduler);
//! * a loss clears `held`, clears `since`, writes an event with the number that caused it, and enqueues the outbox topic.

use darkhold_db::{live_badges, Backend, Database};

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
            "darkhold-livebadge-{}-{}",
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
    let name = format!("lh_badge_{}", uuid::Uuid::new_v4().simple());
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

async fn events(db: &Database, account: &str) -> Vec<serde_json::Value> {
    live_badges::events(db, account, 20).await.expect("events")
}

async fn outbox_topics(db: &Database) -> Vec<String> {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar("SELECT topic FROM outbox_events ORDER BY created_at")
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("topics")
        }
        Backend::Postgres => {
            sqlx::query_scalar("SELECT topic FROM outbox_events ORDER BY created_at")
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await
                .expect("topics")
        }
    }
}

#[tokio::test]
async fn a_badge_is_gained_held_and_lost_with_the_number_that_caused_each_change() {
    let db = connect().await;
    let db = &db;
    let account = "01955100-0000-4000-8000-000000f00001";

    // Gained.
    let outcome = live_badges::recompute(
        db,
        account,
        &darkhold_domain::live_badge::ATTUNED,
        Some(9_000),
        10_000,
        NOW,
    )
    .await
    .expect("recompute");
    assert!(outcome.held);
    assert_eq!(
        outcome.transition,
        darkhold_domain::live_badge::Transition::Gained
    );

    let states = live_badges::for_account(db, account).await.expect("states");
    let attuned = states
        .iter()
        .find(|state| state.badge == "attuned")
        .expect("the catalogue lists attuned even before a sweep");
    assert!(attuned.held);
    assert_eq!(
        attuned.since.as_deref(),
        Some(NOW),
        "a gained badge records since when"
    );
    assert_eq!(attuned.affinity_bp, Some(9_000));

    // Held, with no second event: a recompute that changes nothing is not news.
    let same = live_badges::recompute(
        db,
        account,
        &darkhold_domain::live_badge::ATTUNED,
        Some(9_100),
        10_000,
        NOW,
    )
    .await
    .expect("recompute");
    assert_eq!(
        same.transition,
        darkhold_domain::live_badge::Transition::Held
    );
    assert_eq!(events(db, account).await.len(), 1, "still one gain");

    // Lost: the reading fell, so the badge goes — and the event names the number.
    let lost = live_badges::recompute(
        db,
        account,
        &darkhold_domain::live_badge::ATTUNED,
        Some(4_200),
        10_000,
        NOW,
    )
    .await
    .expect("recompute");
    assert!(!lost.held);
    assert_eq!(
        lost.transition,
        darkhold_domain::live_badge::Transition::Lost
    );
    assert!(lost.reason.contains("4200"), "{}", lost.reason);

    let states = live_badges::for_account(db, account).await.expect("states");
    let attuned = states
        .iter()
        .find(|state| state.badge == "attuned")
        .expect("state");
    assert!(!attuned.held);
    assert_eq!(
        attuned.since, None,
        "'held since' is a claim about the present, so a loss clears it"
    );

    let history = events(db, account).await;
    assert_eq!(history.len(), 2, "{history:?}");
    assert_eq!(history[0]["kind"], "lost", "newest first");
    assert_eq!(history[0]["affinity_bp"], serde_json::json!(4_200));
    assert!(history[0]["reason"]
        .as_str()
        .expect("reason")
        .contains("4200"));
    assert_eq!(history[1]["kind"], "gained");

    let topics = outbox_topics(db).await;
    assert!(topics.contains(&"badge.gained".to_owned()), "{topics:?}");
    assert!(
        topics.contains(&"badge.lost".to_owned()),
        "a loss is a deliverable event, not a silence: {topics:?}"
    );
}

#[tokio::test]
async fn thin_evidence_never_awards_the_badge_however_high_the_score() {
    let db = connect().await;
    let db = &db;
    let account = "01955100-0000-4000-8000-000000f00002";

    // 10,000 of affinity on 5,000 of measured weight: one signal at its ceiling.
    let outcome = live_badges::recompute(
        db,
        account,
        &darkhold_domain::live_badge::ATTUNED,
        Some(10_000),
        5_000,
        NOW,
    )
    .await
    .expect("recompute");
    assert!(!outcome.held, "one signal is not a relationship");
    assert_eq!(
        outcome.transition,
        darkhold_domain::live_badge::Transition::Unearned
    );
    assert!(
        events(db, account).await.is_empty(),
        "nothing changed, so nothing is written"
    );

    // And the state row still records the reading, so a page can say why.
    let states = live_badges::for_account(db, account).await.expect("states");
    let attuned = states
        .iter()
        .find(|state| state.badge == "attuned")
        .expect("state");
    assert_eq!(attuned.affinity_bp, Some(10_000));
    assert_eq!(attuned.evidence_bp, 5_000);
}

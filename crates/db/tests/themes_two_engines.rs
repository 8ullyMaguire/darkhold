//! The theme store on whichever engine `DARKHOLD_TEST_PG_URL` names (M16-13).
//!
//! The store owns three shapes that differ per dialect: an upsert whose `state` demotion is part of the
//! statement, a partial unique index that allows exactly one published theme, and a preview joined by session
//! with an expiry compared in Rust. Each is checked on PostgreSQL rather than assumed.

use std::collections::BTreeMap;

use darkhold_db::theme_profiles::{self, INSTANCE_OWNER};
use darkhold_db::Database;
use darkhold_domain::theme_model::{Placement, SafeMode, Slot, Theme, Token};

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
            "darkhold-themes-2e-{}-{}",
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
    let name = format!("lh_theme_{}", uuid::Uuid::new_v4().simple());
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

fn theme(name: &str, accent: &str) -> Theme {
    Theme {
        name: name.to_owned(),
        palette: BTreeMap::from([(Token::Accent, accent.to_owned())]),
        layout: BTreeMap::from([(Slot::Nav, Placement::Aside)]),
    }
}

#[tokio::test]
async fn publishing_is_a_move_and_the_demotion_is_in_the_statement() {
    let db = connect().await;
    let db = &db;
    let owner = INSTANCE_OWNER;

    let first = theme_profiles::save(
        db,
        "instance",
        owner,
        &theme("one", "#c9a227"),
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("save first");
    let second = theme_profiles::save(
        db,
        "instance",
        owner,
        &theme("two", "#4fa3d1"),
        "2026-11-07T00:01:00Z",
    )
    .await
    .expect("save second");
    assert_ne!(first, second);

    assert!(
        theme_profiles::publish(db, "instance", owner, &first, "2026-11-07T00:02:00Z")
            .await
            .expect("publish first")
    );
    let published = theme_profiles::published(db, "instance", owner)
        .await
        .expect("published")
        .expect("one is published");
    assert_eq!(published.id, first);

    // The second publish must take the first one down: two published rows would make "the instance's
    // appearance" depend on row order.
    assert!(
        theme_profiles::publish(db, "instance", owner, &second, "2026-11-07T00:03:00Z")
            .await
            .expect("publish second")
    );
    let published = theme_profiles::published(db, "instance", owner)
        .await
        .expect("published")
        .expect("one is published");
    assert_eq!(published.id, second, "publishing is a move");
    assert_eq!(published.theme.palette[&Token::Accent], "#4fa3d1");

    // Saving over a published theme demotes it: the bytes that were approved are not the bytes just written.
    theme_profiles::save(
        db,
        "instance",
        owner,
        &theme("two", "#7fc27a"),
        "2026-11-07T00:04:00Z",
    )
    .await
    .expect("re-save");
    assert!(
        theme_profiles::published(db, "instance", owner)
            .await
            .expect("published")
            .is_none(),
        "an edited theme is a draft until somebody publishes it again"
    );

    // Reset deletes rather than writes the defaults.
    let rows = theme_profiles::list(db, "instance", owner)
        .await
        .expect("list");
    assert_eq!(
        rows.len(),
        2,
        "both drafts survive a reset of the published one: {rows:?}"
    );
}

#[tokio::test]
async fn a_preview_is_joined_by_session_and_expires_on_the_clock() {
    let db = connect().await;
    let db = &db;
    let account = "01955100-0000-4000-8000-000000e00001";

    let id = theme_profiles::save(
        db,
        "reader",
        account,
        &theme("mine", "#c9a227"),
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("save");

    theme_profiles::start_preview(db, &id, "session-a", "2026-11-07T00:15:00Z")
        .await
        .expect("preview");

    // A session that did not ask sees nothing.
    assert!(theme_profiles::preview_for(db, "session-b", 0)
        .await
        .expect("preview")
        .is_none());

    // The asking session sees it, and the expiry is compared in Rust against the clock it is given.
    let live = theme_profiles::preview_for(db, "session-a", 1_000)
        .await
        .expect("preview")
        .expect("live preview");
    assert_eq!(live.id, id);

    let far_future = 4_102_444_800; // 2100-01-01
    assert!(
        theme_profiles::preview_for(db, "session-a", far_future)
            .await
            .expect("preview")
            .is_none(),
        "an expired preview is not a preview"
    );

    // Asking twice extends one row rather than stacking two.
    theme_profiles::start_preview(db, &id, "session-a", "2026-11-07T00:30:00Z")
        .await
        .expect("extend");
    let count: i64 = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar("SELECT COUNT(*) FROM theme_previews")
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar("SELECT CAST(COUNT(*) AS BIGINT) FROM theme_previews")
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("count")
        }
    };
    assert_eq!(count, 1);
}

#[tokio::test]
async fn safe_mode_is_one_row_and_its_reason_survives_the_round_trip() {
    let db = connect().await;
    let db = &db;

    assert!(!theme_profiles::safe_mode(db).await.expect("read").active);

    theme_profiles::set_safe_mode(
        db,
        &SafeMode::on("the reader crashed on load"),
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("on");
    let mode = theme_profiles::safe_mode(db).await.expect("read");
    assert!(mode.active);
    assert_eq!(mode.reason.as_deref(), Some("the reader crashed on load"));

    // Turning it on twice is turning it on, not adding a row.
    theme_profiles::set_safe_mode(db, &SafeMode::on("again"), "2026-11-07T00:01:00Z")
        .await
        .expect("on again");
    let rows: i64 = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar("SELECT COUNT(*) FROM theme_safe_mode")
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar("SELECT CAST(COUNT(*) AS BIGINT) FROM theme_safe_mode")
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("count")
        }
    };
    assert_eq!(rows, 1);

    theme_profiles::set_safe_mode(db, &SafeMode::off(), "2026-11-07T00:02:00Z")
        .await
        .expect("off");
    assert!(!theme_profiles::safe_mode(db).await.expect("read").active);
}

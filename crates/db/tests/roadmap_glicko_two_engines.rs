//! M69-10 — the Glicko-2 store, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! What belongs to the store rather than the arithmetic:
//!
//! * a card with no row has **no** deviation, and `position_for` adopts it at the Elo rating with maximum uncertainty — an
//!   Elo number is a position without an uncertainty, and inventing a small one is how a two-ballot card claims to be settled;
//! * saving replaces rather than accumulates, and `diverged` describes the **last** ballot that moved the card, not "ever";
//! * the values read back are the values written, on both engines (the `diverged` column is `INTEGER` on SQLite and `BOOLEAN`
//!   on PostgreSQL, which is the classic place for a `false`/`0` mismatch).

use darkhold_db::{roadmap_glicko, Database};
use darkhold_domain::glicko::Rating;

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
            "darkhold-glicko-{}-{}",
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
    let name = format!("lh_glicko_{}", uuid::Uuid::new_v4().simple());
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
async fn a_card_with_no_row_enters_at_its_elo_and_maximum_uncertainty() {
    let db = connect().await;
    let db = &db;
    assert!(
        roadmap_glicko::load(db, "card-none")
            .await
            .expect("load")
            .is_none(),
        "no row means no numbers, not zeros"
    );

    let adopted = roadmap_glicko::position_for(1720.0, None);
    assert_eq!(adopted.rating, 1720.0);
    assert_eq!(
        adopted.deviation,
        darkhold_domain::glicko::MAX_DEVIATION,
        "an Elo rating carries no uncertainty, so the uncertainty starts at the ceiling"
    );

    roadmap_glicko::save(
        db,
        "card-none",
        Rating {
            rating: 1720.0,
            deviation: 120.0,
            volatility: 0.07,
        },
        true,
        "2026-11-07T00:00:00Z",
    )
    .await
    .expect("save");

    let stored = roadmap_glicko::load(db, "card-none")
        .await
        .expect("load")
        .expect("row");
    assert_eq!(stored.deviation, 120.0);
    assert_eq!(stored.volatility, 0.07);
    assert!(
        stored.diverged,
        "true survives the round trip on both engines"
    );

    // A card is entered with its stored numbers, not with fresh ones.
    let position = roadmap_glicko::position_for(1720.0, Some(stored));
    assert_eq!(position.deviation, 120.0);
    assert_eq!(position.volatility, 0.07);
    assert_eq!(
        position.rating, 1720.0,
        "the rating column is the board's, not the store's"
    );
}

#[tokio::test]
async fn saving_replaces_the_row_and_the_flag_describes_the_last_ballot() {
    let db = connect().await;
    let db = &db;
    let first = Rating {
        rating: 1500.0,
        deviation: 200.0,
        volatility: 0.06,
    };
    roadmap_glicko::save(db, "card-a", first, true, "2026-11-07T00:00:00Z")
        .await
        .expect("save");
    let second = Rating {
        rating: 1600.0,
        deviation: 90.0,
        volatility: 0.05,
    };
    roadmap_glicko::save(db, "card-a", second, false, "2026-11-08T00:00:00Z")
        .await
        .expect("save again");

    let stored = roadmap_glicko::load(db, "card-a")
        .await
        .expect("load")
        .expect("row");
    assert_eq!(
        stored.deviation, 90.0,
        "the second write replaced the first"
    );
    assert_eq!(stored.volatility, 0.05);
    assert!(
        !stored.diverged,
        "a card that diverged once and then settled stops being flagged: the flag is about the last ballot, not the history"
    );

    let all = roadmap_glicko::load_all(db).await.expect("load all");
    assert_eq!(all.len(), 1, "one row per card: {all:?}");
    assert_eq!(all["card-a"].deviation, 90.0);
}

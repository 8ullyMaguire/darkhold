//! M69-13 — trust velocity through the store, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! Two things a multiplier like this gets wrong, and this file is built around both:
//!
//! * **It takes a level away.** A reader whose affinity dropped would be demoted by a product decision nobody can see.
//!   The level written is floored at the level already held, and the test drives that with a real drop in the multiplier.
//! * **It reads absence as punishment.** A reader with no affinity row has not been measured; treating that as a zero
//!   would slow every new reader identically. The test removes the row and asserts the rate returns to neutral.
//!
//! Tenure is the only axis used, because it is the one that needs no second table to seed: an account created 250 days
//! ago scores 250, which is level 4 on the default ladder, and a halved multiplier lands on 125, which is level 3.

use darkhold_db::governance::{self, TrustSettings};
use darkhold_db::{Backend, Database};

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
            "darkhold-velocity-{}-{}",
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
    let name = format!("lh_vel_{}", uuid::Uuid::new_v4().simple());
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

/// An active account created `days` ago, so the tenure axis has a value.
async fn seed_account(db: &Database, id: &str, days: i64) {
    let created = (time::OffsetDateTime::now_utc() - time::Duration::days(days))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format");
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
                 VALUES (?, ?, 'active', 'adult', ?, ?, 'minimal')",
            )
            .bind(id)
            .bind(format!("velocity-{id}@example.test"))
            .bind(&created)
            .bind(&created)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("seed account");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, permission_statement)
                 VALUES ($1::uuid, $2, 'active', 'adult', $3::timestamptz, $3::timestamptz, 'minimal')",
            )
            .bind(id)
            .bind(format!("velocity-{id}@example.test"))
            .bind(&created)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("seed account");
        }
    }
}

async fn settings<'a>(
    ladder: &'a darkhold_domain::trust::TrustLadder,
    axes: &'a darkhold_domain::trust::TrustAxes,
    requirements: &'a darkhold_domain::trust::TrustRequirements,
    velocity: bool,
) -> TrustSettings<'a> {
    let _ = &requirements;
    TrustSettings {
        ladder,
        axes,
        requirements,
        policy: darkhold_domain::trust::ElasticPolicy::default(),
        guest_level: 1,
        accepted_since: None,
        velocity,
    }
}

async fn stored_basis(db: &Database, id: &str) -> serde_json::Value {
    let basis: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar("SELECT basis FROM trust_levels WHERE account = ?")
                .bind(id)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("read basis")
        }
        Backend::Postgres => {
            sqlx::query_scalar("SELECT basis FROM trust_levels WHERE account = $1")
                .bind(id)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await
                .expect("read basis")
        }
    };
    basis
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::Value::Null)
}

#[tokio::test]
async fn a_dropped_multiplier_slows_the_next_level_and_never_takes_the_held_one() {
    let db = connect().await;
    let db = &db;
    let id = "01955100-0000-4000-8000-000000e00001";
    seed_account(db, id, 250).await;

    let ladder = darkhold_domain::trust::TrustLadder::default_ladder();
    let axes = darkhold_domain::trust::TrustAxes::default();
    let requirements = darkhold_domain::trust::TrustRequirements::default();

    // Velocity off: the reader's level is whatever 250 days of tenure earns.
    let plain = governance::recompute_trust(
        db,
        id,
        &settings(&ladder, &axes, &requirements, false).await,
    )
    .await
    .expect("recompute");
    assert_eq!(plain.level, 4, "250 days is level 4 on the default ladder");
    assert!(
        stored_basis(db, id).await.get("velocity").is_none(),
        "an instance that has not adopted velocity stores no multiplier"
    );

    // Now the instance adopts velocity and this reader's affinity is measured at zero: alignment factor 0.5.
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO reader_affinity
                    (account_id, score_bp, evidence_bp, signals, computed_at)
                 VALUES (?, 0, 5000, 1, '2026-11-07T00:00:00Z')",
            )
            .bind(id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("seed affinity");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO reader_affinity
                    (account_id, score_bp, evidence_bp, signals, computed_at)
                 VALUES ($1, 0, 5000, 1, '2026-11-07T00:00:00Z')",
            )
            .bind(id)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("seed affinity");
        }
    }

    let slowed = governance::recompute_trust(
        db,
        id,
        &settings(&ladder, &axes, &requirements, true).await,
    )
    .await
    .expect("recompute");
    assert_eq!(
        slowed.level, 4,
        "the multiplier halves the rate, so the earner reached level 3 — and the held level 4 stands"
    );
    let basis = stored_basis(db, id).await;
    let velocity = &basis["velocity"];
    assert!(
        (velocity["velocity"].as_f64().expect("velocity") - 0.5).abs() < 1e-9,
        "alignment 0 with no reports is 0.5 x 1.0: {basis}"
    );
    assert_eq!(velocity["earned_level"], serde_json::json!(3), "{basis}");
    assert_eq!(velocity["held_level"], serde_json::json!(4), "{basis}");
    assert!(
        velocity["reason"].as_str().expect("reason").contains("never takes a level away"),
        "the stored basis explains the floor: {basis}"
    );

    // And an unmeasured reader is neutral: with the affinity row gone, the multiplier is 1.0 again.
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("DELETE FROM reader_affinity WHERE account_id = ?")
                .bind(id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("clear affinity");
        }
        Backend::Postgres => {
            sqlx::query("DELETE FROM reader_affinity WHERE account_id = $1")
                .bind(id)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("clear affinity");
        }
    }
    let signals = governance::velocity_signals(db, id).await.expect("signals");
    assert_eq!(
        signals.alignment_bp, None,
        "no row is 'not measured', which is different from a score of zero"
    );
    assert!((signals.velocity() - 1.0).abs() < 1e-9);
}

#[tokio::test]
async fn report_counts_are_read_from_the_same_join_the_trust_axis_uses() {
    let db = connect().await;
    let db = &db;
    let id = "01955100-0000-4000-8000-000000e00002";
    seed_account(db, id, 5).await;

    let signals = governance::velocity_signals(db, id).await.expect("signals");
    assert_eq!(signals.confirmed_reports, 0);
    assert_eq!(signals.dismissed_reports, 0);
    assert_eq!(
        signals.behaviour_factor(),
        1.0,
        "a reader who never reported anything is neutral, not suspicious"
    );
}

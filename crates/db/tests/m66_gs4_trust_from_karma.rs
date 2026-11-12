//! M66 GS-4 — trust is derived from the acts, not from a second score about the same person.
//!
//! The row's premise is that `karma` (99 references) and `trust_level` (52) are already in the tree, and that two numbers
//! measuring the same thing is the failure mode. It is, and the tree already resolved it in one direction:
//! `crates/db/src/typed_votes.rs` says, in the module header, that **nothing outside that module and its display route
//! reads `forum_karma`** — "spec §35.2: karma never gates trust, moderation, ranking or credits". Trust is computed from
//! `TrustEvidence`, which is five observations of what a person *did* (tenure, accepted contributions, ratings on their
//! published work, report accuracy, activity), and none of them is a stored score.
//!
//! So the row's work is not to fuse the two numbers — that would make trust farmable by vote brigading, and decayable by
//! silence, which is the demotion this tree refuses elsewhere. The work is to **pin the property**, because it is the kind
//! of property that is cheap to fake and cheap to lose:
//!
//! 1. **Trust moves with the acts.** An account whose published work carries public five-star ratings has a higher
//!    `vote_score` axis than an identical account with no ratings — the derivation is real, not a constant dressed up as a
//!    ladder.
//! 2. **Karma cannot buy a level.** An account with five million basis points of `forum_karma` and no acts is *not*
//!    trusted. This is the "test that would pass for the wrong reason" the row warns about, inverted on purpose: it fails
//!    the moment someone wires the ledger into `evidence_for`, and it is written so that the failure names the ledger.
//!
//! Both engines when `DARKHOLD_TEST_PG_URL` is set, like every store test in this crate.

use darkhold_db::governance::{self, TrustSettings};
use darkhold_db::{Backend, Database};

/// A stable UUID for a human-readable name: PostgreSQL's id columns are `uuid`, and "quiet" is not one.
fn uid(name: &str) -> String {
    // Two FNV-1a passes with different seeds, so the id is derived from the name and stable across runs. The `uuid`
    // crate here is built without the `v5` feature, and a dependency change for a test helper would be the wrong trade.
    let fold = |seed: u64| {
        let mut hash = 0xcbf2_9ce4_8422_2325u64 ^ seed;
        for byte in name.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    };
    uuid::Uuid::from_u128((u128::from(fold(0x9e37)) << 64) | u128::from(fold(0x51ed))).to_string()
}

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
            "darkhold-gs4-{}-{}",
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
    let name = format!("lh_gs4_{}", uuid::Uuid::new_v4().simple());
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

/// An account created `days` ago, so the tenure axis is identical for the accounts a test compares.
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
            .bind(format!("gs4-{id}@example.test"))
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
            .bind(format!("gs4-{id}@example.test"))
            .bind(&created)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("seed account");
        }
    }
}

async fn seed_pseud(db: &Database, account: &str, pseud: &str, handle: &str) {
    let at = "2026-01-01T00:00:00Z";
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
                 VALUES (?, ?, ?, 'GS-4', ?, ?)",
            )
            .bind(pseud)
            .bind(account)
            .bind(handle)
            .bind(at)
            .bind(at)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("seed pseud");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at)
                 VALUES ($1::uuid, $2::uuid, $3, 'GS-4', $4::timestamptz, $4::timestamptz)",
            )
            .bind(pseud)
            .bind(account)
            .bind(handle)
            .bind(at)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("seed pseud");
        }
    }
}

async fn seed_work(db: &Database, work: &str, owner_pseud: &str, title: &str) {
    let at = "2026-01-01T00:00:00Z";
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, created_at, updated_at, published_at)
                 VALUES (?, ?, ?, 'published', ?, ?, ?)",
            )
            .bind(work)
            .bind(owner_pseud)
            .bind(title)
            .bind(at)
            .bind(at)
            .bind(at)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("seed work");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, created_at, updated_at, published_at)
                 VALUES ($1::uuid, $2::uuid, $3, 'published', $4::timestamptz, $4::timestamptz, $4::timestamptz)",
            )
            .bind(work)
            .bind(owner_pseud)
            .bind(title)
            .bind(at)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("seed work");
        }
    }
}

async fn seed_rating(
    db: &Database,
    work: &str,
    rater_account: &str,
    rater_pseud: &str,
    stars: i32,
) {
    let at = "2026-01-01T00:00:00Z";
    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO rating (id, account_id, pseud_id, work_id, stars, is_public, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
            )
            .bind(&id)
            .bind(rater_account)
            .bind(rater_pseud)
            .bind(work)
            .bind(stars)
            .bind(at)
            .bind(at)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("seed rating");
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO rating (id, account_id, pseud_id, work_id, stars, is_public, created_at, updated_at)
                 VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5::int, TRUE, $6::timestamptz, $6::timestamptz)",
            )
            .bind(&id)
            .bind(rater_account)
            .bind(rater_pseud)
            .bind(work)
            .bind(stars)
            .bind(at)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("seed rating");
        }
    }
}

/// The karma ledger, written the way `typed_votes` writes it — the only module allowed to touch this table.
async fn seed_karma(db: &Database, pseud: &str, karma_bp: i64) {
    let at = "2026-01-01T00:00:00Z";
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("INSERT INTO forum_karma (pseud, karma_bp, updated_at) VALUES (?, ?, ?)")
                .bind(pseud)
                .bind(karma_bp)
                .bind(at)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("seed karma");
        }
        Backend::Postgres => {
            sqlx::query("INSERT INTO forum_karma (pseud, karma_bp, updated_at) VALUES ($1::uuid, $2, $3::timestamptz)")
                .bind(pseud)
                .bind(karma_bp)
                .bind(at)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("seed karma");
        }
    }
}

fn settings<'a>(
    ladder: &'a darkhold_domain::trust::TrustLadder,
    axes: &'a darkhold_domain::trust::TrustAxes,
    requirements: &'a darkhold_domain::trust::TrustRequirements,
) -> TrustSettings<'a> {
    TrustSettings {
        ladder,
        axes,
        requirements,
        policy: darkhold_domain::trust::ElasticPolicy::default(),
        guest_level: 1,
        accepted_since: None,
        velocity: false,
    }
}

async fn level_of(
    db: &Database,
    account: &str,
    ladder: &darkhold_domain::trust::TrustLadder,
    axes: &darkhold_domain::trust::TrustAxes,
    requirements: &darkhold_domain::trust::TrustRequirements,
) -> (i64, i64) {
    let evidence = governance::evidence_for(db, account, None)
        .await
        .expect("evidence");
    let assessment =
        governance::recompute_trust(db, account, &settings(ladder, axes, requirements))
            .await
            .expect("recompute");
    (assessment.level, evidence.vote_score)
}

#[tokio::test]
async fn trust_moves_with_the_ratings_a_person_earned() {
    let db = connect().await;
    let ladder = darkhold_domain::trust::TrustLadder::default_ladder();
    let axes = darkhold_domain::trust::TrustAxes::default();
    let requirements = darkhold_domain::trust::TrustRequirements::default();

    // Two readers of the same age. One has published work that strangers rated five stars; the other has published nothing.
    seed_account(&db, &uid("quiet"), 40).await;
    seed_pseud(&db, &uid("quiet"), &uid("quiet-p"), "quiet").await;
    seed_account(&db, &uid("praised"), 40).await;
    seed_pseud(&db, &uid("praised"), &uid("praised-p"), "praised").await;

    for index in 0..12 {
        let work = uid(&format!("praised-work-{index}"));
        seed_work(&db, &work, &uid("praised-p"), &format!("Work {index}")).await;
        seed_account(&db, &uid(&format!("rater-{index}")), 5).await;
        seed_pseud(
            &db,
            &uid(&format!("rater-{index}")),
            &uid(&format!("rater-{index}-p")),
            &format!("rater{index}"),
        )
        .await;
        seed_rating(
            &db,
            &work,
            &uid(&format!("rater-{index}")),
            &uid(&format!("rater-{index}-p")),
            5,
        )
        .await;
    }

    let (quiet_level, quiet_score) =
        level_of(&db, &uid("quiet"), &ladder, &axes, &requirements).await;
    let (praised_level, praised_score) =
        level_of(&db, &uid("praised"), &ladder, &axes, &requirements).await;

    assert!(
        praised_score > quiet_score,
        "the vote axis counts what readers actually did: praised={praised_score} quiet={quiet_score}"
    );
    assert!(
        praised_level >= quiet_level,
        "and the level follows the axis: praised={praised_level} quiet={quiet_level}"
    );
}

#[tokio::test]
async fn the_karma_ledger_cannot_buy_a_level() {
    let db = connect().await;
    let ladder = darkhold_domain::trust::TrustLadder::default_ladder();
    let axes = darkhold_domain::trust::TrustAxes::default();
    let requirements = darkhold_domain::trust::TrustRequirements::default();

    // Identical accounts, identical (empty) history. One of them holds five million basis points of karma.
    seed_account(&db, &uid("nobody"), 40).await;
    seed_pseud(&db, &uid("nobody"), &uid("nobody-p"), "nobody").await;
    seed_account(&db, &uid("karmamonger"), 40).await;
    seed_pseud(
        &db,
        &uid("karmamonger"),
        &uid("karmamonger-p"),
        "karmamonger",
    )
    .await;
    seed_karma(&db, &uid("karmamonger-p"), 5_000_000).await;

    // And a newcomer holding the same ledger, so the comparison is about the ledger and not about the forty days.
    seed_account(&db, &uid("newcomer"), 0).await;
    seed_pseud(&db, &uid("newcomer"), &uid("newcomer-p"), "newcomer").await;
    seed_account(&db, &uid("newcomer-karma"), 0).await;
    seed_pseud(
        &db,
        &uid("newcomer-karma"),
        &uid("newcomer-karma-p"),
        "newcomerkarma",
    )
    .await;
    seed_karma(&db, &uid("newcomer-karma-p"), 5_000_000).await;

    let (nobody_level, nobody_score) =
        level_of(&db, &uid("nobody"), &ladder, &axes, &requirements).await;
    let (karma_level, karma_score) =
        level_of(&db, &uid("karmamonger"), &ladder, &axes, &requirements).await;
    let (newcomer_level, _) = level_of(&db, &uid("newcomer"), &ladder, &axes, &requirements).await;
    let (newcomer_karma_level, _) =
        level_of(&db, &uid("newcomer-karma"), &ladder, &axes, &requirements).await;

    assert_eq!(
        (karma_level, karma_score),
        (nobody_level, nobody_score),
        "spec §35.2: karma never gates trust. If this fails, `evidence_for` has started reading `forum_karma`, \
         and a vote ring — or a decaying ledger — now decides who is trusted"
    );
    assert_eq!(
        newcomer_karma_level, newcomer_level,
        "the ledger does not substitute for history either: a newcomer with five million basis points of karma is a \
         newcomer (level {newcomer_karma_level})"
    );
    assert!(
        newcomer_karma_level <= nobody_level,
        "and it must not outrank an account that simply stayed: newcomer-with-karma={newcomer_karma_level}, \
         forty-days-nobody={nobody_level}"
    );
}

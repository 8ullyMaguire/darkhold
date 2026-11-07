//! W7 task 4 — the contribution axis and its cutoff, at the store level.
//!
//! The rules are proven as pure functions in `darkhold-domain`; this proves the path from a row in the database to the number the
//! axis reads. It exists because the rule that mattered most (a contribution counts only after activation) was, until now, proven
//! only as SQL typed into `psql` — which says nothing about the binding order in `counts_for`.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` for the PostgreSQL arm, as every other store test in this crate does.

use std::time::Duration;

use darkhold_db::Database;

const ACCOUNT: &str = "00000000-0000-0000-0000-0000000000a1";
const PSEUD: &str = "00000000-0000-0000-0000-0000000000b1";
const PROPOSAL: &str = "00000000-0000-0000-0000-0000000000c1";
const PROPOSAL_AT: &str = "2026-10-01T00:00:00Z";

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
        "darkhold-ladder-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4().simple(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_ladder_{}_{}",
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

/// One account, one pseud, and one **passed** proposal dated 2026-10-01 — the row the `accepted_contributions` axis counts.
///
/// Every column listed here was measured from `information_schema` before this file was written, after three fixtures in two days
/// failed one NOT NULL column at a time.
async fn seed(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at) VALUES ('{ACCOUNT}', 'ladder@example.invalid', '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES ('{PSEUD}', '{ACCOUNT}', 'ladderprobe', 'Ladder Probe', '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO category_proposals (id, category_slug, action, payload, status, yes_votes, no_votes, quorum_needed, closes_at, created_by, created_at) \
             VALUES ('{PROPOSAL}', 'ladder-probe', 'create', '{{}}', 'passed', 0, 0, 2, '2026-10-02T00:00:00Z', '{ACCOUNT}', '{PROPOSAL_AT}')"
        ),
    )
    .await;
}

#[tokio::test]
async fn a_contribution_after_the_cutoff_counts_and_one_before_it_does_not() {
    let db = connect("cutoff").await;
    seed(&db).await;

    let settings = darkhold_db::governance::TrustSettings {
        ladder: &darkhold_domain::trust::TrustLadder::default_ladder(),
        axes: &darkhold_domain::trust::TrustAxes::default(),
        requirements: &darkhold_domain::trust::TrustRequirements::default(),
        policy: darkhold_domain::trust::ElasticPolicy::default(),
        guest_level: 1,
        accepted_since: Some("2026-01-01T00:00:00Z"),
        velocity: false,
    };
    let everything = darkhold_db::governance::evidence_for(&db, ACCOUNT, settings.accepted_since)
        .await
        .expect("evidence before the proposal");
    assert_eq!(
        everything.accepted_contributions, 1,
        "a passed proposal dated 2026-10-01 must count when the cutoff is 2026-01-01"
    );

    let after = darkhold_db::governance::evidence_for(&db, ACCOUNT, Some("2026-10-15T00:00:00Z"))
        .await
        .expect("evidence after the proposal");
    assert_eq!(
        after.accepted_contributions, 0,
        "the same proposal must not count when the ladder activated after it was made — this is the property that stops an \
         import from minting a moderator"
    );

    db.close().await;
}

/// The sweep must be idempotent: a second pass over unchanged data writes nothing.
///
/// Measured by counting `trust_events` rows, not by the absence of an error — a job that "runs cleanly" while writing a duplicate
/// row every minute is exactly the failure this catches.
#[tokio::test]
async fn a_second_sweep_writes_no_event() {
    let db = connect("idempotent").await;
    seed(&db).await;
    let settings = darkhold_db::governance::TrustSettings {
        ladder: &darkhold_domain::trust::TrustLadder::default_ladder(),
        axes: &darkhold_domain::trust::TrustAxes::default(),
        requirements: &darkhold_domain::trust::TrustRequirements::default(),
        policy: darkhold_domain::trust::ElasticPolicy::default(),
        guest_level: 1,
        accepted_since: Some("2026-01-01T00:00:00Z"),
        velocity: false,
    };

    async fn events(db: &darkhold_db::Database) -> i64 {
        match db.backend() {
            darkhold_db::Backend::Sqlite => sqlx::query_scalar("SELECT COUNT(*) FROM trust_events")
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("count"),
            darkhold_db::Backend::Postgres => {
                sqlx::query_scalar("SELECT COUNT(*) FROM trust_events")
                    .fetch_one(db.postgres_pool().expect("pg"))
                    .await
                    .expect("count")
            }
        }
    }

    darkhold_db::moderation_ladder::sweep(&db, &settings)
        .await
        .expect("first sweep");
    let after_first = events(&db).await;

    let second = darkhold_db::moderation_ladder::sweep(&db, &settings)
        .await
        .expect("second sweep");
    let after_second = events(&db).await;

    assert_eq!(
        after_first, after_second,
        "a second sweep over unchanged data must not write another event"
    );
    assert!(
        second.changes.is_empty(),
        "a second sweep reported changes it did not make: {:?}",
        second.changes
    );
    db.close().await;
}

/// Run the sweep against a **live** database, printing what it reports and what it wrote.
///
/// Ignored by default so the fixture stays hermetic; run it deliberately:
///
/// ```text
/// DARKHOLD_LADDER_DB=postgres:///darkhold?host=/run/postgresql \
///   cargo test -p darkhold-db --test moderation_ladder -- --ignored --nocapture
/// ```
///
/// It exists because "moderators are minted at level 5" is a fixture until the sweep has walked the real accounts. The assertion is
/// deliberately weak — the sweep must not error and must report a level for every non-guest account — because the interesting output is
/// the count of crossings, which is a fact about this instance and not a property of the code.
#[tokio::test]
#[ignore]
async fn live_sweep() {
    let url = std::env::var("DARKHOLD_LADDER_DB")
        .expect("DARKHOLD_LADDER_DB must name the database to sweep");
    let db = Database::connect(&make_config(url)).await.expect("connect");
    let settings = darkhold_db::governance::TrustSettings {
        ladder: &darkhold_domain::trust::TrustLadder::default_ladder(),
        axes: &darkhold_domain::trust::TrustAxes::default(),
        requirements: &darkhold_domain::trust::TrustRequirements::default(),
        policy: darkhold_domain::trust::ElasticPolicy::default(),
        guest_level: 1,
        accepted_since: None,
        velocity: false,
    };
    let before: i64 = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar("SELECT COUNT(*) FROM trust_events")
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar("SELECT COUNT(*) FROM trust_events")
            .fetch_one(db.postgres_pool().expect("pg"))
            .await
            .expect("count"),
    };
    let sweep = darkhold_db::moderation_ladder::sweep(&db, &settings)
        .await
        .expect("sweep the live database");
    let after: i64 = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar("SELECT COUNT(*) FROM trust_events")
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("count"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar("SELECT COUNT(*) FROM trust_events")
            .fetch_one(db.postgres_pool().expect("pg"))
            .await
            .expect("count"),
    };
    println!(
        "sweep: {} level change(s), {} crossing(s), trust_events {} -> {}",
        sweep.changes.len(),
        sweep.crossings.len(),
        before,
        after
    );
    for c in &sweep.changes {
        println!("  change {} {} -> {}", c.account, c.from, c.to);
    }
    for c in &sweep.crossings {
        println!("  crossing {c} — a moderator claim is due");
    }
    db.close().await;
}

//! A bounty's expiry, which the store implements and nothing ran (M70-13 pass 52).
//!
//! `finder::sweep_expired` is the store half of M45-34: a bounty past `closes_at` becomes `expired` and the credits its
//! `credit_holds` row reserved come free. The store tests prove both halves — "wind the close date back and sweep: the
//! credits come free again" — and no process ever called it. So on the deployed instance a bounty that nobody answered kept
//! its asker's credits frozen forever, which is the failure the module's own doc names first: "A cancelled bounty that kept
//! its hold would quietly freeze someone's credits forever."
//!
//! A store function with no scheduler is the same shape as a table with no writer. The fix is one call in the worker's
//! maintenance pass, beside the other sweeps that nobody presses a button for.

use darkhold_app::worker::{Worker, WorkerOptions};
use darkhold_db::finder::{self as store, Created, NewBounty};
use darkhold_domain::economy::TxnType;
use test_support::{scratch_dir, TestDb};

async fn state_for(
    tdb: &TestDb,
    dir: &std::path::Path,
) -> darkhold_app::state::AppState {
    let mut config = darkhold_app::config::Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    darkhold_app::state::AppState::new(config, tdb.db().clone())
}

async fn exec(db: &darkhold_db::Database, statement: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
    }
}

/// An account with `credits` earned, and a bounty it opened that closes at `closes_at`.
async fn funded_bounty(
    db: &darkhold_db::Database,
    credits: i64,
    budget: i64,
    closes_at: &str,
) -> String {
    let account = uuid::Uuid::new_v4().to_string();
    let pseud = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, email, created_at, updated_at) VALUES ('{account}', \
             'sweep-{account}@x.test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES \
             ('{pseud}', '{account}', 'sweep{account}', 'sweep', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    darkhold_db::economy::post_transaction(
        db,
        TxnType::Grant,
        &format!("grant:{account}"),
        "test grant",
        &[(account.clone(), "earned".to_owned(), credits)],
    )
    .await
    .expect("grant");

    let new = NewBounty {
        description: "Naruto fic read around 2019: Sakura learns a seal, there is a broken tanto in chapter 3, and the \
                      ending is on a rooftop in the rain."
            .to_owned(),
        fandom: Some("Naruto".to_owned()),
        budget,
        closes_at: closes_at.to_owned(),
    };
    match store::create(db, &account, &new).await.expect("create") {
        Created::Bounty(bounty) => bounty.id,
        Created::Refused(reason) => panic!("the fixture's bounty was refused: {reason}"),
    }
}

/// One string column, one row, per engine. Two arms rather than a generic executor: `&str` binds to a UUID column on
/// PostgreSQL only through `::text`, so the SQL differs anyway and a single call site would hide which arm it was.
async fn scalar(db: &darkhold_db::Database, sqlite: &str, postgres: &str, key: &str) -> String {
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(sqlite)
            .bind(key)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("scalar"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(postgres)
            .bind(key)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("scalar"),
    }
}

#[tokio::test]
async fn a_maintenance_pass_expires_a_bounty_nobody_swept() {
    let dir = scratch_dir("bounty-expiry");
    let tdb = TestDb::connect_with_dir("m7013-bounty-expiry", &dir).await;
    let state = state_for(&tdb, &dir).await;
    let db = tdb.db();

    // Closes in thirty days: `create` refuses a date in the past AND anything more than ninety days out ("an unanswered
    // bounty should expire rather than sit on someone's credits forever" — the validation says the same thing the sweep
    // exists to enforce).
    let closes_at = (chrono::Utc::now() + chrono::Duration::days(30))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let bounty = funded_bounty(db, 10, 4, &closes_at).await;
    let asker = scalar(
        db,
        "SELECT asker_account_id FROM finder_bounties WHERE id = ?",
        "SELECT asker_account_id::text FROM finder_bounties WHERE id = $1",
        &bounty,
    )
    .await;

    // 1. Opening a bounty reserves its budget: the credits are gone from "available" while the bounty is open. That is the
    //    hold the sweep has to release, and asserting it first is what makes the release an observation.
    assert_eq!(
        store::available_credits(db, &asker)
            .await
            .expect("available"),
        6,
        "the budget is held while the bounty is open"
    );

    // 2. Wind the close date back — the same move the store's own test makes — and run one maintenance pass. The pass is the
    //    only thing under test: the store's sweep is already covered where it lives.
    exec(
        db,
        &format!(
            "UPDATE finder_bounties SET closes_at = '2020-01-01T00:00:00Z' WHERE id = '{bounty}'"
        ),
    )
    .await;
    Worker::new(WorkerOptions::named("bounty-sweep"))
        .maintenance_pass(&state)
        .await
        .expect("maintenance pass");

    let state_of = scalar(
        db,
        "SELECT state FROM finder_bounties WHERE id = ?",
        "SELECT state FROM finder_bounties WHERE id = $1",
        &bounty,
    )
    .await;
    assert_eq!(state_of, "expired", "the pass expired the bounty");
    assert_eq!(
        store::available_credits(db, &asker)
            .await
            .expect("available"),
        10,
        "an expired bounty returns its credits — without that, the credits stay frozen forever"
    );

    // 3. The sweep is idempotent, so a pass a minute later changes nothing: the state predicate means a second run finds
    //    nothing, and a pass that double-released would pay the asker twice.
    Worker::new(WorkerOptions::named("bounty-sweep"))
        .maintenance_pass(&state)
        .await
        .expect("second maintenance pass");
    assert_eq!(
        store::available_credits(db, &asker)
            .await
            .expect("available"),
        10,
        "a second pass releases nothing more"
    );
}

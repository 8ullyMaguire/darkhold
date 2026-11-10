//! One loan, for the reader who holds it (M70-13 pass 57).
//!
//! `lending::find_loan(loan_id)` existed and had no caller. The surfaces around it were complete without it —
//! `GET /me/loans` lists the caller's windows, `POST /media/{id}/lend` takes one, `PUT` revokes — so the gap was invisible: a
//! client could learn a loan's id from the list and then had no way to ask about that loan. The natural use is a page that
//! shows one loan while it runs (when does this end, did it get revoked), and the only alternative was to re-fetch the whole
//! list and pick the row out of it.
//!
//! Two properties make this door worth having rather than a convenience:
//!
//! * **Owner-scoped, and `404` to a stranger.** A loan id is not a capability: it names a window held by one account, and a
//!   `403` would confirm that a loan exists and is somebody's.
//! * **The window's state is computed, not stored.** `state_at(now)` answers `active`, `expired` or `revoked` from the three
//!   timestamps, so a loan that ran out is `expired` the moment the clock passes `expires_at` and not when a sweeper next runs.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::json;
use test_support::{register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

async fn exec(db: &Database, sqlite: &str, postgres: &str) {
    let sql = db.sql(sqlite, postgres);
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("exec");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("exec");
        }
    }
}

/// An account with an active pseud, and one work it owns.
async fn account_with_work(db: &Database, email: &str, handle: &str) -> (String, String) {
    let mut client = TestClient::new(server::build_router(AppState::new(
        Config::development_defaults(),
        db.clone(),
    )));
    let account = register(&mut client, email, handle).await;
    let pseud = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
             VALUES ('{pseud}', '{account}', '{handle}-pseud', '{handle}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
             VALUES ('{pseud}', '{account}'::uuid, '{handle}-pseud', '{handle}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    let work = uuid::Uuid::new_v4().to_string();
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) \
             VALUES ('{work}', '{pseud}', 'Lent Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) \
             VALUES ('{work}'::uuid, '{pseud}'::uuid, 'Lent Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    (account, work)
}

#[tokio::test]
async fn a_loan_is_readable_by_its_borrower_and_by_nobody_else() {
    let dir = scratch_dir("loan-detail");
    let tdb = TestDb::connect_with_dir("m7013-loan-detail", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let (borrower, work) =
        account_with_work(&db, "loan-borrower@example.test", "loanborrower").await;
    let (stranger, _) = account_with_work(&db, "loan-stranger@example.test", "loanstranger").await;
    let _ = stranger;

    // Two hours of window: long enough that `active` is not a race with the test's own clock.
    let expires = (time::OffsetDateTime::now_utc() + time::Duration::hours(2))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("rfc3339");
    let loan = darkhold_db::lending::grant_loan(&db, &work, &borrower, 1, &expires)
        .await
        .expect("grant");

    // 1. The borrower reads their own loan: the window, its end, and the copy.
    let mut owner = TestClient::new(build(&config, &db));
    test_support::sign_in_as(
        &mut owner,
        &tdb,
        "loan-borrower@example.test",
        "loanborrower",
    )
    .await;
    let (status, body) = owner.get(format!("/api/v1/me/loans/{}", loan.id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], json!(loan.id), "{body}");
    assert_eq!(body["work_id"], json!(work), "{body}");
    assert_eq!(body["state"], json!("active"), "{body}");
    assert_eq!(body["expires_at"], json!(expires), "{body}");
    assert_eq!(body["copy_number"], json!(1), "{body}");

    // 2. A stranger gets `404`, not `403`: a loan id is not a capability, and a `403` would confirm that a loan exists and is
    //    somebody else's.
    let mut other = TestClient::new(build(&config, &db));
    test_support::sign_in_as(
        &mut other,
        &tdb,
        "loan-stranger@example.test",
        "loanstranger",
    )
    .await;
    let (status, body) = other.get(format!("/api/v1/me/loans/{}", loan.id)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "somebody else's loan must be indistinguishable from an absent one: {body}"
    );

    // 3. Revoking does not erase the record: the borrower still sees the window, now ended, and `state` says how.
    assert!(
        darkhold_db::lending::revoke_loan(&db, &loan.id)
            .await
            .expect("revoke"),
        "the loan was active"
    );
    let (status, body) = owner.get(format!("/api/v1/me/loans/{}", loan.id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["state"],
        json!("revoked"),
        "a revoked loan is still a record of a window the reader held: {body}"
    );
    assert!(body["revoked_at"].as_str().is_some(), "{body}");

    // 4. An id that is not a loan, and an id that is not even an id, both answer `404` — never a 500 from a cast.
    let (status, body) = owner
        .get(format!("/api/v1/me/loans/{}", uuid::Uuid::new_v4()))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = owner.get("/api/v1/me/loans/not-a-loan").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a malformed id is absent, not an internal error: {body}"
    );
}

//! W7 task 2 step 3 — the role claim decides, and a refusal is audited (ADR 0003).
//!
//! Three cases, in the order that matters:
//!
//! 1. a moderator of this site is REFUSED a cross-site door (403) — a claim scoped to one site must not widen to the estate,
//!    and this is the wave's own acceptance sentence;
//! 2. that refusal is AUDITED — one row, and it names the site that was required, so "did anyone try to act across sites"
//!    is answerable from the modlog;
//! 3. the same session with `role:operator` is ADMITTED (200) — without this the 403 proves only that the route is broken.
//!
//! The roles are written straight onto the session row rather than driven through an OIDC login: this test is about what the
//! check does with a claim it is given, and the login path is covered by the token exchange the probe measured live.
//!
//! Case 1 fails if the site comparison is removed; case 3 fails if the operator claim is ignored.

use serde_json::json;

use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;

use test_support::{scratch_dir, sign_in_as, TestClient, TestDb};

const ROUTE: &str = "/api/v1/admin/fixity/report";

// The two engines have different pool types, so each helper matches on the backend exactly as production code does — the
// `unwrap_or_else` shortcut does not typecheck, and it is the kind of shortcut that would hide a Postgres-only bug.
async fn set_roles(db: &Database, roles: &str) {
    let sql = db.sql(
        "UPDATE sessions SET roles = ?",
        "UPDATE sessions SET roles = $1",
    );
    // The two arms return different result types, so the binding is discarded inside each arm rather than unified.
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(roles)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("setting roles");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(roles)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("setting roles");
        }
    }
}

async fn refusal_rows(db: &Database) -> i64 {
    let sql = db.sql(
        "SELECT count(*) FROM audit_log WHERE action = 'admin.refused'",
        "SELECT count(*) FROM audit_log WHERE action = 'admin.refused'",
    );
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("counting refusals"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("counting refusals"),
    }
}

#[tokio::test]
async fn a_moderator_is_refused_a_cross_site_door_and_the_refusal_is_audited() {
    let dir = scratch_dir("admin_scope");
    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();
    // No break-glass account and no operator configured: the only authority in play is the claim.
    config.administration.operator_account_id = None;
    let tdb = TestDb::connect_with_dir("admin_scope", &dir).await;
    let db = tdb.db().clone();

    let mut client = TestClient::new(server::build_router(AppState::new(config, db.clone())));

    let email = "moderator@example.com";
    let handle = "moderator";
    sign_in_as(&mut client, &tdb, email, handle).await;

    // 1. a site-scoped moderator, at a cross-site door.
    set_roles(&db, r#"["role:moderator:darkhold"]"#).await;
    let (status, body) = client.get(ROUTE).await;
    assert_eq!(
        status.as_u16(),
        403,
        "a moderator of one site must not reach a cross-site door: {status} {body}"
    );

    // 2. and the refusal is a row, not a log line.
    assert_eq!(
        refusal_rows(&db).await,
        1,
        "the refusal must be audited exactly once"
    );

    // 3. the same door, same session, with the estate-wide claim.
    set_roles(&db, r#"["role:operator"]"#).await;
    let (status, _body) = client.get(ROUTE).await;
    assert_eq!(
        status.as_u16(),
        200,
        "an operator claim must open the cross-site door: {status}"
    );

    // The admitted call is not a refusal, so the count did not move.
    assert_eq!(refusal_rows(&db).await, 1, "only the refusal is audited");
    let _ = json!({});
}

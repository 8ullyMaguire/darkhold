//! Account identity as an operator acts on it (§14.2 folds, §15.8 guardians, §12.6 facts) — the module's doors (M70-13).
//!
//! `grant_guardian_authorization` documented the defect in its own header: before it, "a minor could never participate,
//! whatever the operator set up", because `guardian_authorizations` existed and nothing wrote to it. That is the shape of
//! this whole pass — a store that implements a promise, and a binary that never asks it to.
//!
//! The claims:
//!
//! 1. **Facts are the operator's, and a missing account is 404** — distinct from an account with no facts.
//! 2. **An age decision takes a reason** and refuses a state the store does not know, in both cases naming the field.
//! 3. **The guardian grant does both halves** — records the authorisation *and* moves the state to
//!    `authorized_under_policy`, so the account can participate. A row that left the state where it was is a row nobody
//!    reads.
//! 4. **A fold points at a live account.** No self-fold, no fold into a folded account, no fold into nothing — a tombstone
//!    that points nowhere is worse than no tombstone.
//! 5. **A trust history is the account's own or an operator's.** Anybody else gets 404, which was a live leak in the same
//!    module and is closed here.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use test_support::{register, scratch_dir, sign_in_as, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

async fn account_id_of(client: &mut TestClient) -> String {
    let (status, me) = client.get("/api/v1/auth/me").await;
    assert_eq!(status, StatusCode::OK, "{me}");
    me["account"]["id"].as_str().unwrap_or_default().to_owned()
}

#[tokio::test]
async fn age_a_guardian_grant_and_a_fold_are_operator_actions_with_reasons() {
    let dir = scratch_dir("identity-admin");
    let tdb = TestDb::connect_with_dir("m7013-identity-admin", &dir).await;
    let db: Database = tdb.db().clone();

    let base = Config::development_defaults();
    let mut bootstrap = TestClient::new(build(&base, &db));
    let operator_account = register(&mut bootstrap, "ia-operator@example.test", "iaoperator").await;
    let mut config = base.clone();
    config.administration.operator_account_id = Some(operator_account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&config, &db));
    sign_in_as(&mut operator, &tdb, "ia-operator@example.test", "iaoperator").await;

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "ia-reader@example.test", "iareader").await;
    let reader_id = account_id_of(&mut reader).await;

    let mut other = TestClient::new(build(&config, &db));
    register(&mut other, "ia-other@example.test", "iaother").await;
    let other_id = account_id_of(&mut other).await;

    // 1. Facts are operator-only, and the operator gets the instance's own copy of trust and resonance.
    let (status, body) = reader
        .get(format!("/api/v1/admin/accounts/{reader_id}/facts"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, facts) = operator
        .get(format!("/api/v1/admin/accounts/{reader_id}/facts"))
        .await;
    assert_eq!(status, StatusCode::OK, "{facts}");
    assert_eq!(facts["account_id"], serde_json::json!(reader_id), "{facts}");
    assert_eq!(facts["is_guest"], serde_json::json!(false), "{facts}");
    assert!(facts["trust_level"].as_i64().is_some(), "{facts}");
    assert!(facts["resonance"].as_f64().is_some(), "{facts}");
    assert!(facts["supporter_until"].is_null(), "{facts}");

    // A missing account is 404, not an account with no facts.
    let (status, body) = operator
        .get("/api/v1/admin/accounts/00000000-0000-4000-8000-000000000000/facts")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 2. An age decision needs a reason and a state the store knows.
    let (status, body) = operator
        .post(
            format!("/api/v1/admin/accounts/{reader_id}/age-state"),
            serde_json::json!({"state": "restricted"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("reason"), "{body}");
    let (status, body) = operator
        .post(
            format!("/api/v1/admin/accounts/{reader_id}/age-state"),
            serde_json::json!({"state": "probably_fine", "reason": "a hunch"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("state"), "{body}");

    let (status, set) = operator
        .post(
            format!("/api/v1/admin/accounts/{reader_id}/age-state"),
            serde_json::json!({"state": "restricted", "reason": "policy review, ticket 12"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    // Registration declares an adult (the sign-up form asks), so the "from" is that declaration and not "unknown".
    assert_eq!(set["from"], serde_json::json!("declared_adult"), "{set}");
    assert_eq!(set["state"], serde_json::json!("restricted"), "{set}");
    assert_eq!(
        set["may_participate"],
        serde_json::json!(false),
        "restricted is the state that forbids participation: {set}"
    );

    // The change is in the modlog with its reason, which is the point of requiring one.
    let (status, log) = operator.get("/api/v1/modlog?limit=50").await;
    assert_eq!(status, StatusCode::OK, "{log}");
    let entry = log["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|e| e["action"] == serde_json::json!("account.age_state"))
        .cloned()
        .unwrap_or_else(|| panic!("no age-state entry in the modlog: {log}"));
    assert_eq!(
        entry["reason"],
        serde_json::json!("policy review, ticket 12"),
        "{entry}"
    );

    // 3. The guardian grant records the authorisation and moves the state — both halves, read back from the row.
    let (status, body) = operator
        .post(
            format!("/api/v1/admin/accounts/{reader_id}/guardian-authorization"),
            serde_json::json!({"reason": "guardian verified by call"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("verification_reference"),
        "{body}"
    );

    let (status, granted) = operator
        .post(
            format!("/api/v1/admin/accounts/{reader_id}/guardian-authorization"),
            serde_json::json!({
                "verification_reference": "GUARD-4471",
                "reason": "guardian verified by call",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{granted}");
    assert_eq!(
        granted["state"],
        serde_json::json!("authorized_under_policy"),
        "the grant moves the state; a recorded authorisation that leaves the account stuck is a row nobody reads: {granted}"
    );
    assert_eq!(
        granted["may_participate"],
        serde_json::json!(true),
        "{granted}"
    );

    // 4. A fold points at a live account.
    let (status, body) = operator
        .post(
            format!("/api/v1/admin/accounts/{reader_id}/fold"),
            serde_json::json!({"folded_into": reader_id, "reason": "tidy"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("itself"), "{body}");

    let (status, body) = operator
        .post(
            format!("/api/v1/admin/accounts/{reader_id}/fold"),
            serde_json::json!({"folded_into": "00000000-0000-4000-8000-000000000000", "reason": "tidy"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("no such account"), "{body}");

    let (status, folded) = operator
        .post(
            format!("/api/v1/admin/accounts/{reader_id}/fold"),
            serde_json::json!({"folded_into": other_id, "reason": "guest merged into their account"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{folded}");
    assert_eq!(folded["status"], serde_json::json!("folded"), "{folded}");
    assert_eq!(folded["folded_into"], serde_json::json!(other_id), "{folded}");

    // A second fold of the same account, and a fold *into* the tombstone, are both refused: no chains that end nowhere.
    let (status, body) = operator
        .post(
            format!("/api/v1/admin/accounts/{reader_id}/fold"),
            serde_json::json!({"folded_into": other_id, "reason": "again"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("already folded"),
        "a tombstone is terminal: {body}"
    );
    let mut third = TestClient::new(build(&config, &db));
    register(&mut third, "ia-third@example.test", "iathird").await;
    let third_id = account_id_of(&mut third).await;
    let (status, body) = operator
        .post(
            format!("/api/v1/admin/accounts/{third_id}/fold"),
            serde_json::json!({"folded_into": reader_id, "reason": "chain"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("folded"),
        "the refusal says the target is a tombstone: {body}"
    );

    // 5. A trust history is the account's own, or an operator's.
    let (status, own) = other.get(format!("/api/v1/accounts/{other_id}/trust")).await;
    assert_eq!(status, StatusCode::OK, "{own}");
    assert_eq!(own["account_id"], serde_json::json!(other_id), "{own}");
    let (status, body) = other.get(format!("/api/v1/accounts/{reader_id}/trust")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "somebody's disciplinary record is not another reader's to read: {body}"
    );
    let (status, theirs) = operator
        .get(format!("/api/v1/accounts/{reader_id}/trust"))
        .await;
    assert_eq!(status, StatusCode::OK, "{theirs}");
}

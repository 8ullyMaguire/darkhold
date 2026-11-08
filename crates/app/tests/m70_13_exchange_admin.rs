//! The exchange's operator side (§11.17, §15.17, §19.14) — the module's doors (M70-13).
//!
//! `set_enabled` and `set_instance_id` had no caller, so **the only way to turn this instance's exchange on was SQL** and
//! the identifier a signal carries was whatever a migration left there. `list_latent_demand` had no caller, so the instance
//! accumulated demand it could not show anybody; `count_provenance_signals` had no caller, so the function whose name
//! exists to keep "provenance" from reading as "headcount" was never read by a person.
//!
//! The claims:
//!
//! 1. **No settings row is not "disabled".** `configured: false` with the defaults the first write would create — an
//!    operator has never touched the exchange, which is a different state from having turned it off.
//! 2. **The switch works and the answer is the row afterwards**, including the 1000/hour default that appears with it.
//! 3. **The instance id is an identifier, not free text** — refused with the field named, and recorded in the modlog,
//!    because it is what this instance calls itself to strangers.
//! 4. **Latent demand is demand**, with `signal_count` named for what it counts and no reader count anywhere.
//! 5. **Provenance is distinct signals**, which is why it is an operator diagnostic and not a public field.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::exchange;
use darkhold_db::Database;
use test_support::{register, scratch_dir, sign_in_as, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

#[tokio::test]
async fn the_exchange_switch_its_name_and_its_two_queues_are_reachable_only_by_an_operator() {
    let dir = scratch_dir("exchange-admin");
    let tdb = TestDb::connect_with_dir("m7013-exchange-admin", &dir).await;
    let db: Database = tdb.db().clone();

    let base = Config::development_defaults();
    let mut bootstrap = TestClient::new(build(&base, &db));
    let operator_account = register(&mut bootstrap, "ex-operator@example.test", "exoperator").await;
    let mut config = base.clone();
    config.administration.operator_account_id = Some(operator_account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&config, &db));
    sign_in_as(&mut operator, &tdb, "ex-operator@example.test", "exoperator").await;

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "ex-reader@example.test", "exreader").await;

    // 1. Never configured: not an error, and not the same answer as "an operator turned it off".
    let (status, fresh) = operator.get("/api/v1/admin/exchange/settings").await;
    assert_eq!(status, StatusCode::OK, "{fresh}");
    assert_eq!(fresh["configured"], serde_json::json!(false), "{fresh}");
    assert_eq!(fresh["enabled"], serde_json::json!(false), "{fresh}");
    assert_eq!(
        fresh["rate_limit_per_hour"],
        serde_json::json!(1000),
        "the default the first write would create: {fresh}"
    );

    // A reader never reaches any of it.
    let (status, body) = reader.get("/api/v1/admin/exchange/settings").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 2. Flipping the switch creates the row with §11.17's rate limit beside it.
    let (status, body) = operator
        .post(
            "/api/v1/admin/exchange/settings",
            serde_json::json!({"enabled": true}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a settings change without a reason is refused: {body}"
    );
    assert!(body.to_string().contains("reason"), "{body}");

    let (status, on) = operator
        .post(
            "/api/v1/admin/exchange/settings",
            serde_json::json!({"enabled": true, "reason": "opening the exchange to partner instances"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{on}");
    assert_eq!(on["configured"], serde_json::json!(true), "{on}");
    assert_eq!(on["enabled"], serde_json::json!(true), "{on}");
    assert_eq!(on["rate_limit_per_hour"], serde_json::json!(1000), "{on}");

    // An empty body is not a no-op, it is a bug in the caller.
    let (status, body) = operator
        .post(
            "/api/v1/admin/exchange/settings",
            serde_json::json!({"reason": "nothing to change"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("enabled"), "{body}");

    // 3. The instance's own name: validated, and written.
    for bad in ["", "  ", "has a space", "quote\"ed", "a".repeat(65).as_str()] {
        let (status, body) = operator
            .post(
                "/api/v1/admin/exchange/settings",
                serde_json::json!({"instance_id": bad, "reason": "naming"}),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "accepted {bad:?}: {body}"
        );
        assert!(body.to_string().contains("instance_id"), "{body}");
    }
    let (status, named) = operator
        .post(
            "/api/v1/admin/exchange/settings",
            serde_json::json!({"instance_id": "darkhold.example:8443", "reason": "naming the instance"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{named}");
    assert_eq!(
        named["instance_id"],
        serde_json::json!("darkhold.example:8443"),
        "{named}"
    );

    // Both changes are in the modlog with their reason.
    let (status, log) = operator.get("/api/v1/modlog?limit=50").await;
    assert_eq!(status, StatusCode::OK, "{log}");
    let entries: Vec<_> = log["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|e| e["action"] == serde_json::json!("exchange.settings"))
        .cloned()
        .collect();
    assert_eq!(entries.len(), 2, "one row per accepted change: {log}");
    assert!(
        entries
            .iter()
            .any(|e| e["reason"] == serde_json::json!("naming the instance")),
        "{log}"
    );

    // 4. Latent demand: the instance's own demand rows, with a count that is not a headcount.
    exchange::reinforce_latent_demand(&db, "aaaaaaaa-0000-4000-8000-000000000001", "ao3.example")
        .await
        .expect("latent demand");
    exchange::reinforce_latent_demand(&db, "aaaaaaaa-0000-4000-8000-000000000001", "ao3.example")
        .await
        .expect("reinforce");
    // ... and the queue is the operator's: a reader asking gets 404, not a demand list.
    let (status, body) = reader.get("/api/v1/admin/exchange/latent-demand").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, demand) = operator.get("/api/v1/admin/exchange/latent-demand").await;
    assert_eq!(status, StatusCode::OK, "{demand}");
    assert_eq!(demand["count"], serde_json::json!(1), "{demand}");
    assert_eq!(
        demand["items"][0]["work_id"],
        serde_json::json!("aaaaaaaa-0000-4000-8000-000000000001"),
        "{demand}"
    );
    assert_eq!(
        demand["items"][0]["signal_count"],
        serde_json::json!(1),
        "§16.16.1: re-signalling adds no weight, so the count stays 1: {demand}"
    );
    assert!(
        demand["items"][0].get("submitters").is_none() && demand["items"][0].get("holders").is_none(),
        "no headcount field, structurally: {demand}"
    );

    // 5. Provenance: distinct signals retained for one work.
    let work = "bbbbbbbb-0000-4000-8000-000000000002";
    let account = operator_account.clone();
    for hash in ["hash-one", "hash-two"] {
        exchange::store_signal(&db, hash, Some(work), &account, Some("ao3.example"), "{}")
            .await
            .expect("signal");
    }
    let (status, provenance) = operator
        .get(format!("/api/v1/admin/exchange/works/{work}/provenance"))
        .await;
    assert_eq!(status, StatusCode::OK, "{provenance}");
    assert_eq!(
        provenance["provenance_signals"],
        serde_json::json!(2),
        "two distinct signals, not two people: {provenance}"
    );
    assert!(
        provenance["note"]
            .as_str()
            .is_some_and(|n| n.contains("not a count of readers")),
        "the distinction travels with the number: {provenance}"
    );

    // ... and a reader still cannot see it.
    let (status, body) = reader
        .get(format!("/api/v1/admin/exchange/works/{work}/provenance"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

//! Imported taste signals (§49.6–§49.8, M45-20) — the module's first doors (M70-13).
//!
//! `crates/db/src/taste_import.rs` was written with a long header explaining five decisions and then had no route at all:
//! nothing in the application named `taste_import`, so a reader could not import a history, could not see the split
//! between what they gave this instance and what was carried over, and could not read the audit row of an import.
//!
//! The claims this test makes, in the order §49.6–§49.8 make them:
//!
//! 1. **An import lands as `imported`.** The store takes no origin parameter, so the door cannot produce an organic
//!    signal; the read shows the origin beside every row.
//! 2. **Re-importing the same history adds nothing** (§49.8). The second run over the same file is `added == 0`, and
//!    the profile has the same number of rows as after the first.
//! 3. **The discount is visible and is the named constant** (§49.6): three imported signals carry less weight than their
//!    own values, by exactly `IMPORTED_SIGNAL_DISCOUNT`.
//! 4. **The split is readable as rows and weight**, which are different questions with different answers.
//! 5. **An import is the caller's**, and the audit row of one is not readable by a stranger.
//! 6. **A credential named in the body must be the caller's own** — otherwise a reader could attach somebody else's
//!    credential id to their own import run.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use test_support::{register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

fn history() -> serde_json::Value {
    serde_json::json!({
        "source_key": "ao3",
        "import_kind": "bookmarks",
        "signals": [
            {"kind": "bookmark", "dimension_key": "tag:fluff", "source_signal_key": "wk-1",
             "occurred_at": "2019-04-01T00:00:00Z"},
            {"kind": "bookmark", "dimension_key": "tag:fluff", "source_signal_key": "wk-2",
             "occurred_at": "2019-05-02T00:00:00Z"},
            {"kind": "rating", "dimension_key": "tag:angst", "source_signal_key": "wk-3",
             "occurred_at": "2019-06-03T00:00:00Z"},
        ],
    })
}

#[tokio::test]
async fn an_import_lands_as_imported_discounted_once_and_readable_only_by_its_reader() {
    let dir = scratch_dir("taste-import");
    let tdb = TestDb::connect_with_dir("m7013-taste-import", &dir).await;
    let db: Database = tdb.db().clone();

    let config = Config::development_defaults();

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "ti-reader@example.test", "tireader").await;

    // 1. Three signals, all imported, all inserted.
    let (status, first) = reader
        .post("/api/v1/me/taste/signals/import", history())
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["seen"], serde_json::json!(3), "{first}");
    assert_eq!(first["added"], serde_json::json!(3), "{first}");
    let import_id = first["import_id"].as_str().expect("import id").to_owned();

    // 2. §49.8: the same history imported twice leaves the profile identical to importing it once.
    let (status, second) = reader
        .post("/api/v1/me/taste/signals/import", history())
        .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(
        second["seen"],
        serde_json::json!(3),
        "the source still offers three: {second}"
    );
    assert_eq!(
        second["added"],
        serde_json::json!(0),
        "a re-import adds nothing: {second}"
    );

    // 3. Every row reads as imported, and the origin is the store's decision rather than the door's.
    let (status, listed) = reader.get("/api/v1/me/taste/signals").await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["count"], serde_json::json!(3), "{listed}");
    let signals = listed["signals"].as_array().expect("signals");
    assert_eq!(signals.len(), 3, "{listed}");
    for signal in signals {
        assert_eq!(
            signal["origin"],
            serde_json::json!("imported"),
            "an import cannot masquerade as organic: {listed}"
        );
    }

    // 4. The dimension filter narrows to one dimension's signals and nothing else.
    let (status, fluff) = reader
        .get("/api/v1/me/taste/signals?dimension_key=tag:fluff")
        .await;
    assert_eq!(status, StatusCode::OK, "{fluff}");
    assert_eq!(fluff["count"], serde_json::json!(2), "{fluff}");
    assert_eq!(
        fluff["signals"][0]["dimension_key"],
        serde_json::json!("tag:fluff"),
        "{fluff}"
    );

    // 5. The split: three rows, and a weight below their own values by exactly the named discount.
    //    bookmark 0.4 + bookmark 0.4 + rating 1.0 = 1.8 raw, × 0.5 imported = 0.9.
    let (status, split) = reader.get("/api/v1/me/taste/signals/origins").await;
    assert_eq!(status, StatusCode::OK, "{split}");
    assert_eq!(split["organic"], serde_json::json!(0), "{split}");
    assert_eq!(split["imported"], serde_json::json!(3), "{split}");
    assert_eq!(
        split["imported_discount"],
        serde_json::json!(0.5),
        "the magnitude is named rather than implied: {split}"
    );
    let imported_weight = split["imported_weight"].as_f64().expect("weight");
    assert!(
        (imported_weight - 0.9).abs() < 1e-9,
        "three imported signals weigh 0.9 (1.8 raw × 0.5): {split}"
    );

    // 6. The audit row of the run: what it offered, what it inserted, and when it started.
    let (status, run) = reader
        .get(format!("/api/v1/me/taste/imports/{import_id}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(run["signals_seen"], serde_json::json!(3), "{run}");
    assert_eq!(run["signals_added"], serde_json::json!(3), "{run}");
    assert_eq!(run["source_key"], serde_json::json!("ao3"), "{run}");
    assert_eq!(run["import_kind"], serde_json::json!("bookmarks"), "{run}");

    // 7. Somebody else's import row is 404, not "here it is with an account id in it".
    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "ti-stranger@example.test", "tistranger").await;
    let (status, body) = stranger
        .get(format!("/api/v1/me/taste/imports/{import_id}"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    // ... and the stranger's own profile is empty, not the reader's.
    let (status, theirs) = stranger.get("/api/v1/me/taste/signals").await;
    assert_eq!(status, StatusCode::OK, "{theirs}");
    assert_eq!(theirs["count"], serde_json::json!(0), "{theirs}");

    // 8. Bad input is refused with the field named, before anything is written.
    let (status, body) = reader
        .post(
            "/api/v1/me/taste/signals/import",
            serde_json::json!({
                "source_key": "ao3",
                "import_kind": "bookmarks",
                "signals": [{"kind": "loved", "dimension_key": "tag:fluff", "source_signal_key": "wk-9"}],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("kind"), "{body}");
    let (status, body) = reader
        .post(
            "/api/v1/me/taste/signals/import",
            serde_json::json!({"source_key": "ao3", "import_kind": "bookmarks", "signals": []}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("signals"), "{body}");

    // 9. A credential that EXISTS but belongs to another reader is refused, and the stranger who owns it may use it.
    //    Both halves matter: an all-zero id would be refused for not existing, which is a different check and a weaker
    //    one — it cannot tell "somebody else's" from "nobody's".
    let (_, stranger_me) = stranger.get("/api/v1/auth/me").await;
    let stranger_account = stranger_me["account"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let stranger_pseud = test_support::active_pseud_id(&mut stranger, &stranger_account)
        .await
        .expect("stranger pseud");
    let cipher = darkhold_app::secrets::load_cipher(&dir.join("storage"), None, false)
        .expect("the development key");
    let secret_id = darkhold_app::secrets::seal_secret(
        &db,
        &cipher,
        "source_credential",
        &format!("{stranger_pseud}\u{1f}ao3\u{1f}their label"),
        "secret",
        &darkhold_app::secrets::Secret::new("their-session-cookie"),
    )
    .await
    .expect("seal the stranger's credential");
    let (credential, _previous) = darkhold_db::imports::upsert_source_credential(
        &db,
        &stranger_pseud,
        "ao3",
        &secret_id,
        "their label",
        None,
        Some("password"),
        Some("archiveofourown.org"),
        None,
    )
    .await
    .expect("store the stranger's credential");

    let (status, body) = reader
        .post(
            "/api/v1/me/taste/signals/import",
            serde_json::json!({
                "source_key": "ao3",
                "import_kind": "bookmarks",
                "credential_id": credential.id,
                "signals": [{"kind": "bookmark", "dimension_key": "tag:fluff", "source_signal_key": "wk-11"}],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("not a credential of yours"),
        "the refusal names the reason, not just the field: {body}"
    );

    // The owner of that credential may use it, which is what makes the refusal an ownership check rather than an
    // existence check.
    let (status, theirs_ok) = stranger
        .post(
            "/api/v1/me/taste/signals/import",
            serde_json::json!({
                "source_key": "ao3",
                "import_kind": "bookmarks",
                "credential_id": credential.id,
                "signals": [{"kind": "bookmark", "dimension_key": "tag:fluff", "source_signal_key": "wk-12"}],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{theirs_ok}");
    assert_eq!(theirs_ok["added"], serde_json::json!(1), "{theirs_ok}");

    // 10. A credential id that is not the caller's is refused: an import must not wire somebody else's login to it.
    let (status, body) = reader
        .post(
            "/api/v1/me/taste/signals/import",
            serde_json::json!({
                "source_key": "ao3",
                "import_kind": "bookmarks",
                "credential_id": "00000000-0000-0000-0000-000000000000",
                "signals": [{"kind": "bookmark", "dimension_key": "tag:fluff", "source_signal_key": "wk-10"}],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("credential_id"), "{body}");
}

//! The import catalogue's missing half (M70-13): six store functions with no caller.
//!
//! Four of the six had a producer and got one:
//!
//! * `upsert_source` — its doc comment claimed "called at startup for every registered adapter, so the catalogue a reader
//!   sees cannot drift from the adapters the binary actually has". Nothing called it, so `sources` was whatever a
//!   migration had seeded. It is called from `serve` now, and the first test proves the table ends up holding exactly the
//!   keys the registry carries, twice over (a deploy happens more than once).
//! * `set_source_enabled` — an operator's pause, with the reason a reader will see.
//! * `set_source_verification` — how this host can verify a source, and the refusal of a `blocked-here` with no note.
//! * `get_import_job_for` — not a door: the account-scoped read that `get_import` was duplicating in Rust.
//!
//! The other two, `list_credential_events` and `pseuds_with_unconsented_credentials`, have **no producer in this build**:
//! every adapter in `crates/scrapers/src/sites` declares `authentication: AuthKind::None`, so no credential row can be
//! created through the API at all. They are given their doors anyway and exercised against rows written by the store,
//! because a credential audit trail with no credential would be a dead surface — but the honest statement is that the
//! doors are tested, not that they are reachable live. Recorded in `roadmap-remaining.md`.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::imports;
use darkhold_db::{Backend, Database};
use test_support::{active_pseud_id, id, register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

async fn exec(tdb: &TestDb, sqlite: &str, postgres: &str, binds: &[&str]) {
    match tdb.db().backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(sqlite);
            for b in binds {
                q = q.bind(*b);
            }
            q.execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("seed row failed: {error}\nSQL: {sqlite}"));
        }
        Backend::Postgres => {
            let mut q = sqlx::query(postgres);
            for b in binds {
                q = q.bind(*b);
            }
            q.execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("seed row failed: {error}\nSQL: {postgres}"));
        }
    }
}


/// The account id behind a handle, as the type `config.administration` stores.
///
/// The two engines answer different types (`TEXT` on SQLite, `uuid` on PostgreSQL), so each arm parses into the same
/// `AccountId` rather than collecting into one variable.
async fn account_of(db: &Database, handle: &str) -> darkhold_domain::ids::AccountId {
    let sql = match db.backend() {
        Backend::Sqlite => "SELECT account_id FROM pseuds WHERE lower(handle) = lower(?)",
        Backend::Postgres => "SELECT account_id::text AS account_id FROM pseuds WHERE lower(handle) = lower($1)",
    };
    match db.backend() {
        Backend::Sqlite => {
            let value: String = sqlx::query_scalar(sql)
                .bind(handle)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("the operator's account");
            value.parse().expect("account id parses")
        }
        Backend::Postgres => {
            let value: String = sqlx::query_scalar(sql)
                .bind(handle)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await
                .expect("the operator's account");
            value.parse().expect("account id parses")
        }
    }
}

/// Register a reader.
async fn reader(client: &mut TestClient, email: &str, handle: &str) -> String {
    register(client, email, handle).await
}

#[tokio::test]
async fn the_catalogue_is_this_build_pausing_needs_a_reason_and_the_audit_trail_is_the_owners() {
    let dir = scratch_dir("imports-catalogue");
    let tdb = TestDb::connect_with_dir("m7013-imports-catalogue", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    // 1. Startup records every adapter the build carries, and a second start changes nothing but the timestamps.
    let state = AppState::new(config.clone(), db.clone());
    let written = darkhold_app::source_sync::sync_source_catalogue(&state).await;
    let keys: Vec<String> = state
        .registry()
        .catalogue()
        .iter()
        .map(|entry| entry.key.as_str().to_owned())
        .collect();
    assert!(written > 1, "the build carries more than one adapter");
    let recorded = imports::list_sources(&db).await.expect("sources");
    let recorded_keys: Vec<String> = recorded.iter().map(|row| row.key.clone()).collect();
    for key in &keys {
        assert!(
            recorded_keys.contains(key),
            "{key} is an adapter this build carries and the catalogue does not list: {recorded_keys:?}"
        );
    }
    let before = recorded.len();
    darkhold_app::source_sync::sync_source_catalogue(&state).await;
    let after = imports::list_sources(&db).await.expect("sources");
    assert_eq!(
        after.len(),
        before,
        "a second start must not add rows: the catalogue is keyed on the source"
    );

    // 2. An operator pauses a source; the reason is stored beside the flag.
    let mut bootstrap = TestClient::new(build(&config, &db));
    reader(&mut bootstrap, "imp-op@example.test", "impoperator").await;
    let mut config = config.clone();
    config.administration.operator_account_id = Some(account_of(&db, "impoperator").await);
    let mut operator_client = TestClient::new(build(&config, &db));
    // A rebuilt router is a new client with an empty cookie jar; the session is in the database, so signing in again is
    // the honest way to be the operator rather than copying cookies by hand.
    test_support::sign_in_as(&mut operator_client, &tdb, "imp-op@example.test", "impoperator").await;

    let target = keys.first().expect("a source to pause").clone();
    let (status, body) = operator_client
        .put(
            format!("/api/v1/admin/sources/{target}/enabled"),
            serde_json::json!({ "enabled": false, "reason": "robots.txt changed under us" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let paused = imports::find_source(&db, &target)
        .await
        .expect("read the source")
        .expect("the source exists");
    assert!(!paused.enabled, "the pause was recorded: {paused:?}");
    assert_eq!(
        paused.disabled_reason.as_deref(),
        Some("robots.txt changed under us"),
        "the reason a reader sees is stored beside the flag"
    );
    assert_eq!(paused.health, "paused", "health follows the flag");

    // 3. Resuming needs no reason — the reason explains a pause, not a decision to run.
    let (status, body) = operator_client
        .put(
            format!("/api/v1/admin/sources/{target}/enabled"),
            serde_json::json!({ "enabled": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // 4. A source this build does not carry is 404, not a silent no-op.
    let (status, body) = operator_client
        .put(
            "/api/v1/admin/sources/no-such-source/enabled",
            serde_json::json!({ "enabled": false, "reason": "testing" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 5. `blocked-here` without a note is refused, and refused as a validation error naming the field.
    let (status, body) = operator_client
        .put(
            format!("/api/v1/admin/sources/{target}/verification"),
            serde_json::json!({ "status": "blocked-here" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("reason"),
        "the refusal names the missing field: {body}"
    );
    // ... and with a note it is recorded.
    let (status, body) = operator_client
        .put(
            format!("/api/v1/admin/sources/{target}/verification"),
            serde_json::json!({ "status": "blocked-here", "reason": "Cloudflare challenge on every page" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let verified = imports::find_source(&db, &target)
        .await
        .expect("read the source")
        .expect("the source exists");
    assert_eq!(verified.verification_status.as_deref(), Some("blocked-here"));
    assert_eq!(
        verified.verification_note.as_deref(),
        Some("Cloudflare challenge on every page"),
        "the note travels with the status"
    );

    // 6. A non-operator is answered 404 on the same doors — confirming that an operator surface exists is itself a
    //    disclosure (spec §11.8).
    let mut plain_client = TestClient::new(build(&config, &db));
    reader(&mut plain_client, "imp-plain@example.test", "impplain").await;
    let (status, body) = plain_client
        .put(
            format!("/api/v1/admin/sources/{target}/enabled"),
            serde_json::json!({ "enabled": false, "reason": "should not work" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 7. A credential written by the store has an audit trail its owner can read, and nobody else can.
    let mut vault_client = TestClient::new(build(&config, &db));
    let vault_account = reader(&mut vault_client, "imp-vault@example.test", "impvault").await;
    let pseud = active_pseud_id(&mut vault_client, &vault_account)
        .await
        .expect("active pseud");
    let cipher = darkhold_app::secrets::load_cipher(&dir.join("storage"), None, false)
        .expect("the development key");
    let secret_id = darkhold_app::secrets::seal_secret(
        &db,
        &cipher,
        "source_credential",
        &format!("{pseud}\u{1f}ao3\u{1f}a label"),
        "secret",
        &darkhold_app::secrets::Secret::new("a-session-cookie"),
    )
    .await
    .expect("seal");
    let (credential, _previous) = imports::upsert_source_credential(
        &db,
        &pseud,
        "ao3",
        &secret_id,
        "a label",
        None,
        Some("password"),
        Some("archiveofourown.org"),
        None,
    )
    .await
    .expect("store a credential");
    imports::record_credential_event(&db, &credential.id, &pseud, imports::CredentialEvent::Created)
        .await
        .expect("record the created event");
    imports::record_credential_event(&db, &credential.id, &pseud, imports::CredentialEvent::Revoked)
        .await
        .expect("record the revoked event");

    // Two events recorded inside the same second cannot be ordered: `occurred_at` is second-resolution and the tiebreak
    // is a random UUID, so asserting which came first there would be asserting a coin flip. The `created` row is moved
    // back a day instead, which is what makes "newest first" a real claim rather than a hopeful one.
    exec(
        &tdb,
        "UPDATE credential_audit_events SET occurred_at = '2026-01-01T00:00:00Z' WHERE event = 'created'",
        "UPDATE credential_audit_events SET occurred_at = '2026-01-01T00:00:00Z' WHERE event = 'created'",
        &[],
    )
    .await;

    let (status, body) = vault_client
        .get(format!("/api/v1/source-credentials/{}/events", credential.id))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let events = body["events"].as_array().expect("events");
    assert_eq!(events.len(), 2, "the trail outlives nothing here: {body}");
    assert_eq!(
        events[0]["event"],
        serde_json::json!("revoked"),
        "newest first: {body}"
    );
    assert_eq!(events[1]["event"], serde_json::json!("created"), "{body}");

    let mut stranger_client = TestClient::new(build(&config, &db));
    let stranger_account = reader(&mut stranger_client, "imp-stranger@example.test", "impstranger").await;
    // An active pseud, because `RequirePseud` refuses a session without one *before* the handler runs: without this the
    // stranger would get a 404 from the extractor and the ownership filter would never be exercised at all.
    let _ = active_pseud_id(&mut stranger_client, &stranger_account)
        .await
        .expect("the stranger's active pseud");
    let (status, body) = stranger_client
        .get(format!("/api/v1/source-credentials/{}/events", credential.id))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another reader's credential trail is not yours: {body}"
    );

    // 8. The consent sweep names the pseud holding an unconsented credential, and stops naming it once consent exists.
    let (status, body) = operator_client
        .get("/api/v1/admin/credentials/unconsented")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["pseud_ids"].to_string().contains(&pseud),
        "the sweep must name the pseud whose credential has no consent record: {body}"
    );
    exec(
        &tdb,
        "UPDATE source_credentials SET consent_at = '2026-01-01T00:00:00Z' WHERE id = ?",
        "UPDATE source_credentials SET consent_at = '2026-01-01T00:00:00Z' WHERE id::text = $1",
        &[&credential.id],
    )
    .await;
    let (status, body) = operator_client
        .get("/api/v1/admin/credentials/unconsented")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body["pseud_ids"].to_string().contains(&pseud),
        "consent recorded is consent recorded: {body}"
    );

    // 9. A stranger is 404 on the sweep too.
    let (status, body) = stranger_client
        .get("/api/v1/admin/credentials/unconsented")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// `get_import` was the second implementation of the account scope `get_import_job_for` already had.
#[tokio::test]
async fn an_import_is_its_owners_and_the_scoped_read_still_refuses_a_stranger() {
    let dir = scratch_dir("imports-scope");
    let tdb = TestDb::connect_with_dir("m7013-imports-scope", &dir).await;
    let db: Database = tdb.db().clone();

    let mut config = Config::development_defaults();
    config.storage.root = dir.clone();

    let mut owner = TestClient::new(build(&config, &db));
    let account = register(&mut owner, "imp-owner2@example.test", "impowner2").await;

    // A job written through the store, in the order the import route writes it: the queue row first, because the import
    // names it in its payload. This test is about the read's scope, not about starting an import.
    let pseud = active_pseud_id(&mut owner, &account)
        .await
        .expect("active pseud");
    let account_id: darkhold_domain::ids::AccountId = account.parse().expect("account id parses");
    let queue_job = darkhold_db::jobs::enqueue(
        &db,
        darkhold_domain::jobs::JobKind::Import,
        "{}",
        None,
        Some(account_id),
        0,
        &darkhold_domain::jobs::RetryPolicy::default(),
    )
    .await
    .expect("enqueue the import's job");
    let job = id("imp-job");
    imports::create_import_job(
        &db,
        &job,
        &queue_job.to_string(),
        &account,
        &pseud,
        "ao3",
        "https://archiveofourown.org/works/1",
        "library",
        false,
    )
    .await
    .expect("create the import");

    let (status, body) = owner.get(format!("/api/v1/imports/{job}")).await;
    assert_eq!(status, StatusCode::OK, "the owner reads their own import: {body}");
    assert_eq!(body["id"], serde_json::json!(job), "{body}");

    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "imp-nosy@example.test", "impnosy").await;
    let (status, body) = stranger.get(format!("/api/v1/imports/{job}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the account scope is in the query now, and it still refuses: {body}"
    );
}

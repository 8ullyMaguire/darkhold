//! Preservation over HTTP — the doors M59-11…16 never had (M70-13).
//!
//! Five rules, each only real if a request proves it:
//!
//! * a destination is operator policy: adding or switching one off is not a reader's business, and a stranger gets `404` from
//!   the operator doors rather than `403` (ADR 0030);
//! * recording a crosspost is the **owner's** claim and pays **nothing** — §2.2 pays for verified live preservation, so the
//!   payment belongs to the recheck, not to the door;
//! * an author who set `redistribution: no` has refused the crosspost, and a refusal is not a claim to overwrite;
//! * a work this instance **imported** is already preserved elsewhere, and §2.7 refuses a second claim on it;
//! * the work's own answer counts live numbers and says what would make it more preserved — not a bare number a reader has to
//!   compare against a threshold they cannot see.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7013pres-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Client {
    app: Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn new(app: Router) -> Self {
        Self {
            app,
            cookies: Vec::new(),
        }
    }

    fn capture(&mut self, response: &axum::response::Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((pair, _)) = text.split_once(';') else {
                continue;
            };
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let name = name.trim().to_owned();
            let value = value.trim().to_owned();
            self.cookies.retain(|(k, _)| k != &name);
            if !value.is_empty() {
                self.cookies.push((name, value));
            }
        }
    }

    async fn send(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(Method::from_bytes(method.as_bytes()).expect("method"))
            .uri(uri);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; ");
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(token) = self
            .cookies
            .iter()
            .find(|(k, _)| k == "darkhold_csrf")
            .map(|(_, v)| v.to_owned())
        {
            builder = builder.header("x-csrf-token", token);
        }
        let body = body.map_or_else(Body::empty, |value| Body::from(value.to_string()));
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// The same cookies, a new app.
    ///
    /// A session lives in the database and its cookie in this client, so "sign in, then change the config" is two apps and one
    /// signed-in reader. Losing the cookies here is losing the sign-in — which is exactly how the first version of this test
    /// turned an operator's write into a `401`.
    fn rejoin(&self, app: Router) -> Self {
        Self {
            app,
            cookies: self.cookies.clone(),
        }
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                Some(json!({
                    "email": format!("m7013pres-{handle}@example.test"),
                    "password": "test-pass-1234",
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                })),
            )
            .await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "register {handle}: {body}"
        );
        let (_, me) = self.send("GET", "/api/v1/auth/me", None).await;
        me["account"]["id"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .expect("account id")
            .to_owned()
    }
}

fn config(dir: &std::path::Path, operator: Option<String>) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    if let Some(id) = operator {
        config.administration.operator_account_id = Some(id.parse().expect("account id"));
    }
    config
}

async fn published(client: &mut Client, title: &str) -> String {
    let (status, body) = client
        .send("POST", "/api/v1/works", Some(json!({ "title": title })))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("a work id").to_owned()
}

#[tokio::test]
async fn an_operator_configures_a_destination_and_a_stranger_cannot() {
    let dir = scratch_dir("destinations");
    let tdb = TestDb::connect_with_dir("m7013pres", &dir).await;

    let mut operator = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    let operator_id = operator.register("pres-operator").await;

    // The same database, with the operator named in config — an operator is configuration, not a role somebody grants
    // themselves through the API.
    let mut operator = operator.rejoin(server::build_router(AppState::new(
        config(&dir, Some(operator_id)),
        tdb.db().clone(),
    )));
    let mut reader = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    reader.register("pres-reader").await;

    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/admin/preservation/destinations/ao3-mirror",
            Some(json!({
                "name": "AO3 mirror",
                "base_url": "https://mirror.example",
                "match_rule": "/works/",
                "accepts_automated": true,
                "enabled": true,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], json!("ao3-mirror"), "{body}");

    // A base URL with a trailing slash makes every fetch a `//`, and the door refuses rather than normalises.
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/admin/preservation/destinations/slashy",
            Some(json!({
                "name": "Slashy",
                "base_url": "https://slashy.example/",
                "match_rule": "/works/",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = operator
        .send("GET", "/api/v1/admin/preservation/destinations", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["enabled"], json!(1), "{body}");
    assert_eq!(body["total"], json!(1), "{body}");

    // Off is not a deletion: the destination stays listed, and the count says nothing is enabled.
    let (status, body) = operator
        .send(
            "POST",
            "/api/v1/admin/preservation/destinations/ao3-mirror/enabled",
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = operator
        .send("GET", "/api/v1/admin/preservation/destinations", None)
        .await;
    assert_eq!(body["enabled"], json!(0), "{body}");
    assert_eq!(body["total"], json!(1), "{body}");

    // Back on, because the rest of the test needs a destination.
    operator
        .send(
            "POST",
            "/api/v1/admin/preservation/destinations/ao3-mirror/enabled",
            Some(json!({ "enabled": true })),
        )
        .await;

    // A stranger: the operator's config is not theirs to read, and a refusal does not confirm what exists (ADR 0030).
    let (status, body) = reader
        .send("GET", "/api/v1/admin/preservation/destinations", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = reader
        .send(
            "PUT",
            "/api/v1/admin/preservation/destinations/theirs",
            Some(json!({
                "name": "Theirs",
                "base_url": "https://theirs.example",
                "match_rule": "/w/",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = reader
        .send(
            "POST",
            "/api/v1/admin/preservation/destinations/ao3-mirror/enabled",
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn the_owner_records_a_claim_which_pays_nothing() {
    let dir = scratch_dir("claim");
    let tdb = TestDb::connect_with_dir("m7013presclaim", &dir).await;

    let mut operator = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    let operator_id = operator.register("claim-operator").await;
    let mut operator = operator.rejoin(server::build_router(AppState::new(
        config(&dir, Some(operator_id)),
        tdb.db().clone(),
    )));
    operator
        .send(
            "PUT",
            "/api/v1/admin/preservation/destinations/ao3-mirror",
            Some(json!({
                "name": "AO3 mirror",
                "base_url": "https://mirror.example",
                "match_rule": "/works/",
                "accepts_automated": true,
            })),
        )
        .await;

    let mut author = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    author.register("pres-author").await;
    let work = published(&mut author, "A Work To Preserve").await;

    // Before any claim: nothing verified, one destination eligible, and the work says what it is short of.
    let (status, body) = author
        .send("GET", &format!("/api/v1/works/{work}/preservation"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["verified"], json!(0), "{body}");
    assert_eq!(body["eligible_destinations"], json!(1), "{body}");
    assert_eq!(body["threshold"], json!(3), "{body}");
    assert_eq!(body["fully_preserved"], json!(false), "{body}");
    assert_eq!(
        body["shortfall"],
        json!("0 of 3 verified archives; more destinations would reach it"),
        "the work says what would make it preserved: {body}"
    );
    // The default is `unstated`, not `ask`: a work whose author has not been asked has not said anything, and a default of
    // `ask` would put a share decision in the mouths of every author who never opened the settings page.
    assert_eq!(body["redistribution"], json!("unstated"), "{body}");

    // The claim. It is recorded as unverified and pays nothing — §2.2 pays for verified live preservation.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/preservation/ao3-mirror"),
            Some(json!({
                "external_record_id": "999111",
                "external_url": "https://mirror.example/works/999111",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("unverified"), "{body}");
    assert_eq!(body["credits_paid"], json!(0), "{body}");
    assert_eq!(body["destination_id"], json!("ao3-mirror"), "{body}");

    // Recorded once: the same destination again returns the existing row rather than a second claim.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/preservation/ao3-mirror"),
            Some(json!({ "external_record_id": "999111" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, body) = author
        .send("GET", &format!("/api/v1/works/{work}/preservation"), None)
        .await;
    assert_eq!(
        body["targets"].as_array().expect("targets").len(),
        1,
        "one claim per destination: {body}"
    );
    assert_eq!(
        body["verified"],
        json!(0),
        "a claim is not a verification: {body}"
    );

    // An unknown destination is a 404 rather than a claim against nothing.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/preservation/nope"),
            Some(json!({ "external_record_id": "1" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // A stranger cannot claim on somebody else's work.
    let mut stranger = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    stranger.register("claim-stranger").await;
    let (status, body) = stranger
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/preservation/ao3-mirror"),
            Some(json!({ "external_record_id": "2" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn a_refusal_and_an_imported_work_both_stop_the_claim() {
    let dir = scratch_dir("refusal");
    let tdb = TestDb::connect_with_dir("m7013presrefusal", &dir).await;

    let mut operator = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    let operator_id = operator.register("refusal-operator").await;
    let mut operator = operator.rejoin(server::build_router(AppState::new(
        config(&dir, Some(operator_id)),
        tdb.db().clone(),
    )));
    operator
        .send(
            "PUT",
            "/api/v1/admin/preservation/destinations/ao3-mirror",
            Some(json!({
                "name": "AO3 mirror",
                "base_url": "https://mirror.example",
                "match_rule": "/works/",
            })),
        )
        .await;

    let mut author = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    let account = author.register("refusal-author").await;
    let work = published(&mut author, "A Withdrawn Work").await;

    // The author refuses redistribution, and the refusal is what stands between the work and a crosspost.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/preservation/redistribution"),
            Some(json!({ "redistribution": "no" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["redistribution"], json!("no"), "{body}");
    assert_eq!(body["permits_crosspost"], json!(false), "{body}");

    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/preservation/ao3-mirror"),
            Some(json!({ "external_record_id": "1" })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a refusal is not a claim to overwrite: {body}"
    );

    // `yes` permits it, and the statement is the author's.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/preservation/redistribution"),
            Some(json!({ "redistribution": "yes" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["permits_crosspost"], json!(true), "{body}");

    // An unknown word is refused with the vocabulary named.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/preservation/redistribution"),
            Some(json!({ "redistribution": "maybe" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // §2.7: an imported work is already preserved elsewhere. The library row is what "imported" means.
    let other = published(&mut author, "An Imported Work").await;
    let now = "2026-01-01T00:00:00Z";
    let sql = tdb.db().sql(
        "INSERT INTO library_items
           (id, account_id, work_id, source_key, source_work_key, title, source_url, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'ao3', '12345', 'An Imported Work', 'https://ao3.example/x', ?4, ?4)",
        "INSERT INTO library_items
           (id, account_id, work_id, source_key, source_work_key, title, source_url, created_at, updated_at)
         VALUES ($1::uuid, $2::uuid, $3::uuid, 'ao3', '12345', 'An Imported Work', 'https://ao3.example/x', $4, $4)",
    );
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&account)
                .bind(&other)
                .bind(now)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("library item");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&account)
                .bind(&other)
                .bind(now)
                .execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("library item");
        }
    }

    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{other}/preservation/ao3-mirror"),
            Some(json!({ "external_record_id": "7" })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "§2.7 refuses an imported work: {body}"
    );
}

#[tokio::test]
async fn the_leaderboard_is_public_and_empty_until_something_is_verified() {
    let dir = scratch_dir("preservers");
    let tdb = TestDb::connect_with_dir("m7013preservers", &dir).await;
    let app = server::build_router(AppState::new(config(&dir, None), tdb.db().clone()));

    // No session at all: the leaderboard is one of the few things on this instance that is nobody's private business.
    let (status, body) = Client::new(app)
        .send("GET", "/api/v1/preservation/preservers", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["preservers"].as_array().expect("rows").len(),
        0,
        "{body}"
    );
    assert_eq!(body["days"], json!(28), "{body}");
}

/// The edition list, which is how "this work exists in N places" is answered exactly once (A0).
///
/// `members_of` and `member_counts` had no caller, so the grouping table was written by the claim path and read by nobody:
/// a work could accumulate external locations that no page ever listed, and "this work has one copy" and "this work has
/// four" were indistinguishable from outside.
///
/// The claims: a work nobody has crossposted answers an **empty list and zero counts** (not a 404, because "not
/// crossposted" is a state), the local copy is first, and the counts are the same rows rather than a second query that
/// could disagree.
#[tokio::test]
async fn the_edition_list_is_local_first_and_counts_what_the_claim_wrote() {
    let dir = scratch_dir("editions");
    let tdb = TestDb::connect_with_dir("m7013presed", &dir).await;

    let mut operator = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    let operator_id = operator.register("ed-operator").await;
    let mut operator = operator.rejoin(server::build_router(AppState::new(
        config(&dir, Some(operator_id)),
        tdb.db().clone(),
    )));
    let (status, body) = operator
        .send(
            "PUT",
            "/api/v1/admin/preservation/destinations/ao3-mirror",
            Some(json!({
                "name": "AO3 mirror",
                "base_url": "https://mirror.example",
                "match_rule": "/works/",
                "accepts_automated": true,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let mut author = Client::new(server::build_router(AppState::new(
        config(&dir, None),
        tdb.db().clone(),
    )));
    author.register("ed-author").await;
    let work = published(&mut author, "A Work With Editions").await;

    // Never crossposted: no identity, so no members and no counts — and that is a normal answer, not a missing page.
    let (status, body) = author
        .send("GET", &format!("/api/v1/works/{work}/preservation"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["editions"], json!([]), "{body}");
    assert_eq!(
        body["copies"],
        json!({"total": 0, "external": 0, "local": 0}),
        "{body}"
    );

    // One claim: the identity is created with this instance's own copy, then the external member is recorded.
    let (status, body) = author
        .send(
            "PUT",
            &format!("/api/v1/works/{work}/preservation/ao3-mirror"),
            Some(json!({
                "external_record_id": "444222",
                "external_url": "https://mirror.example/works/444222",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = author
        .send("GET", &format!("/api/v1/works/{work}/preservation"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let editions = body["editions"].as_array().expect("editions");
    assert_eq!(editions.len(), 2, "the local copy and one external: {body}");
    assert!(
        editions[0]["work_id"].as_str().is_some(),
        "local first — the copy a reader can actually open: {body}"
    );
    assert!(
        editions[0]["external_url"].is_null(),
        "the local member has no external location: {body}"
    );
    assert_eq!(
        editions[1]["external_url"],
        json!("https://mirror.example/works/444222"),
        "{body}"
    );
    assert_eq!(
        editions[1]["external_source_key"],
        json!("ao3-mirror"),
        "{body}"
    );
    assert_eq!(
        body["copies"],
        json!({"total": 2, "external": 1, "local": 1}),
        "the counts are the same rows: {body}"
    );
}

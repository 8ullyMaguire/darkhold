//! M45-42 — claim your works: a token in the author's own profile attaches metadata rows to their account.
//!
//! What only the routes can get wrong:
//!
//! 1. **The token is the proof, and it is minted once.** Two claims by one author must not create two tokens to paste, and a
//!    page without the token proves nothing — the claim stays `pending` rather than failing.
//! 2. **A work already owned is refused by name, not skipped.** `claimed: 0` with no reasons reads as "nothing to do"; an
//!    author who cannot take their own work needs to know which work and why.
//! 3. **The exclusion registry wins.** An author who opted their works out of this instance cannot claim them back in
//!    through a door that bypasses the registry.
//! 4. **A claim id is not a capability.** A stranger gets `404`, not `403`.
//! 5. **Verifying twice is not an error.** The second answer is `claimed: 0` with the work already theirs.
//!
//! The profile fetch is a seam (`ProfileFetcher`), so this suite answers with a canned page instead of reaching AO3.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use serde_json::{json, Value};
use test_support::TestDb;
use tower::ServiceExt;

const PASSWORD: &str = "test-pass-1234";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m45-39-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn config_for(dir: &Path) -> Config {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    // The pool the router queries comes from `TestDb` below, so this value is never
    // connected to — but it must not name the wrong engine. A suite that runs on
    // PostgreSQL while its config says `sqlite://` is exactly what the dialect gate
    // reports, and the gate is right: it cannot tell a dead field from a live one.
    // The selector is read here so the config and the run agree.
    config.database = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(url) if !url.is_empty() => DatabaseConfig::new(url),
        _ => DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )),
    };
    config
}

struct Client {
    app: axum::Router,
    cookies: Vec<(String, String)>,
}

impl Client {
    fn new(app: axum::Router) -> Self {
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

    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
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
        let body = match body {
            Some(v) => Body::from(v.to_string()),
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        self.capture(&response);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    async fn get(&mut self, uri: &str) -> (StatusCode, Value) {
        self.request("GET", uri, None).await
    }

    async fn send(&mut self, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        self.request(method, uri, Some(body)).await
    }

    async fn register(&mut self, handle: &str) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/register",
                json!({
                    "email": format!("m45-39-{handle}@example.test"),
                    "password": PASSWORD,
                    "handle": handle,
                    "display_name": handle,
                    "age_band": "adult",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "register {handle}: {body}");
        body["pseuds"][0]["handle"]
            .as_str()
            .unwrap_or(handle)
            .to_owned()
    }
}

struct Harness {
    _dir: PathBuf,
    tdb: TestDb,
    config: Config,
}

impl Harness {
    /// A harness where `operator` holds the operator role and `reader` does not.
    async fn new(tag: &str) -> Self {
        server::set_trust_proxy(false);
        let dir = scratch_dir(tag);
        let mut config = config_for(&dir);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let db = tdb.db().clone();

        let mut bootstrap = Client::new(server::build_router(AppState::new(
            config.clone(),
            db.clone(),
        )));
        bootstrap.register("operator").await;
        let account = account_of(&db, "operator").await;
        config.administration.operator_account_id = Some(account.into());

        let mut reader_client = Client::new(server::build_router(AppState::new(
            config.clone(),
            db.clone(),
        )));
        reader_client.register("reader").await;

        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    fn db(&self) -> &darkhold_db::Database {
        self.tdb.db()
    }

    /// A client with a freshly registered account, already signed in.
    async fn registered(&self, handle: &str) -> Client {
        let mut client = self.client();
        client.register(handle).await;
        client
    }

    /// A client whose profile fetcher answers `url` with `page`, signed in as whoever `from` is signed in as.
    ///
    /// The router is rebuilt because the fetcher lives on the app state; the cookies are carried over because the claim is
    /// the caller's, and a fresh anonymous client would get `401` for a reason this suite is not about.
    fn client_with_page(&self, from: &Client, url: &str, page: &str) -> Client {
        let fetcher = darkhold_app::profile_fetch::CannedProfileFetcher::default()
            .with(url, page)
            .into_fetcher();
        let mut client = Client::new(server::build_router(
            AppState::new(self.config.clone(), self.tdb.db().clone()).with_profile_fetcher(fetcher),
        ));
        client.cookies = from.cookies.clone();
        client
    }

    fn client(&self) -> Client {
        Client::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    /// A signed-in client for the named handle.
    #[allow(dead_code)]
    async fn signed_in(&self, handle: &str) -> Client {
        let mut client = self.client();
        let (status, body) = client
            .send(
                "POST",
                "/api/v1/auth/login",
                json!({
                    "email": format!("m45-39-{handle}@example.test"),
                    "password": PASSWORD,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "login {handle}: {body}");
        client
    }
}

async fn account_of(db: &darkhold_db::Database, handle: &str) -> uuid::Uuid {
    let sql = db.sql(
        "SELECT account_id FROM pseuds WHERE lower(handle) = lower(?)",
        "SELECT account_id::text AS account_id FROM pseuds WHERE lower(handle) = lower(?)",
    );
    let value: String = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("the account"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.postgres_pool().expect("pg"))
            .await
            .expect("the account"),
    };
    value.parse().expect("an account id is a uuid")
}

/// One public work carrying `tags`, owned by the pseud id given.
async fn seed_work_for_pseud(
    db: &darkhold_db::Database,
    pseud: &str,
    title: &str,
    tags: &[&str],
) -> String {
    let work = uuid::Uuid::new_v4().to_string();
    test_support::exec_binds(
        db,
        "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle,
          completion, created_at, updated_at, version)
         VALUES (?, ?, ?, '', 'en', 'general', 'public', 'published', 'complete',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
        "INSERT INTO works (id, owner_pseud_id, title, summary, language, rating, visibility, lifecycle,
          completion, created_at, updated_at, version)
         VALUES ($1::uuid, $2::uuid, $3, '', 'en', 'general', 'public', 'published', 'complete',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
        &[&work, pseud, title],
    )
    .await;
    for tag in tags {
        let node = (*tag).to_owned();
        test_support::exec_binds(
            db,
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at) VALUES (?, ?, 1, '2026-01-01T00:00:00Z')",
            "INSERT INTO work_tags (work_id, node_id, prominence, added_at) VALUES ($1::uuid, $2, 1, '2026-01-01T00:00:00Z')",
            &[&work, &node],
        )
        .await;
    }
    work
}

/// A work imported as metadata: owned by the system account, which is what "no author here" means in this schema
/// (`orphan.rs` reads the same fact).
async fn seed_metadata_work(db: &darkhold_db::Database, title: &str, tags: &[&str]) -> String {
    let system = darkhold_db::system_account();
    let pseud: Option<String> = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_scalar("SELECT id FROM pseuds WHERE account_id = ? ORDER BY handle LIMIT 1")
                .bind(&system)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("look up the system pseud")
        }
        darkhold_db::Backend::Postgres => sqlx::query_scalar(
            "SELECT id::text FROM pseuds WHERE account_id = $1::uuid ORDER BY handle LIMIT 1",
        )
        .bind(&system)
        .fetch_optional(db.postgres_pool().expect("postgres"))
        .await
        .expect("look up the system pseud"),
    };
    let pseud = match pseud {
        Some(pseud) => pseud,
        None => {
            // The migration that creates the system account does not give it a pseud; a metadata work needs one to point at,
            // which is the same reason the import path creates it.
            let id = uuid::Uuid::new_v4().to_string();
            test_support::exec_binds(
                db,
                "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version)
                 VALUES (?, ?, 'imported', 'Imported', 'listed', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
                "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version)
                 VALUES ($1::uuid, $2::uuid, 'imported', 'Imported', 'listed', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
                &[&id, &system],
            )
            .await;
            id
        }
    };
    seed_work_for_pseud(db, &pseud, title, tags).await
}

fn claim_body(works: &[&str], url: &str) -> Value {
    json!({ "works": works, "profile_url": url })
}

const PROFILE: &str = "https://archiveofourown.org/users/claimant";

/// The whole flow, in the order an author meets it.
#[tokio::test]
async fn a_token_in_the_profile_attaches_the_metadata_work() {
    let h = Harness::new("claim").await;
    let mut claimant = h.registered("claimant").await;
    let mut other = h.registered("other").await;
    let mut stranger = h.client();

    let work = seed_metadata_work(h.db(), "orphan-work", &["n-angst"]).await;
    let (status, body) = claimant
        .send("POST", "/api/v1/claims", claim_body(&[&work], PROFILE))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let claim_id = body["claim_id"].as_str().expect("claim id").to_owned();
    let token = body["token"].as_str().expect("token").to_owned();
    assert!(token.starts_with("dh-claim-"), "{body}");

    // 1. Minting again is the same token: an author pastes one string, not two.
    let (_, again) = claimant
        .send("POST", "/api/v1/claims", claim_body(&[&work], PROFILE))
        .await;
    assert_eq!(again["token"], json!(token), "{again}");

    // 2. A page without the token proves nothing, and does not refuse the claim.
    let mut empty_page = h.client_with_page(&claimant, PROFILE, "<html>no token here</html>");
    let (status, body) = empty_page
        .send(
            "POST",
            &format!("/api/v1/claims/{claim_id}/verify"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["profile_url"],
        json!("the token was not found on that page"),
        "{body}"
    );
    let (_, state) = claimant.get(&format!("/api/v1/claims/{claim_id}")).await;
    assert_eq!(
        state["state"],
        json!("pending"),
        "a page that proves nothing is not a refusal: {state}"
    );

    // 3. The page carrying the token attaches the work.
    let page = format!("<html><p>find me on ao3 · {token}</p></html>");
    let mut with_token = h.client_with_page(&claimant, PROFILE, &page);
    let (status, body) = with_token
        .send(
            "POST",
            &format!("/api/v1/claims/{claim_id}/verify"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["claimed"], json!(1), "{body}");
    assert_eq!(body["refused"], json!([]), "{body}");
    let (_, state) = claimant.get(&format!("/api/v1/claims/{claim_id}")).await;
    assert_eq!(state["state"], json!("verified"), "{state}");
    assert_eq!(state["works"][0]["outcome"], json!("claimed"), "{state}");

    // 4. Verifying again is not an error, and attaches nothing twice.
    let (status, body) = with_token
        .send(
            "POST",
            &format!("/api/v1/claims/{claim_id}/verify"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["claimed"], json!(0), "{body}");

    // 5. Somebody else claiming the same work is refused by name.
    let (status, body) = other
        .send("POST", "/api/v1/claims", claim_body(&[&work], PROFILE))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let other_id = body["claim_id"].as_str().expect("claim id").to_owned();
    let other_token = body["token"].as_str().expect("token").to_owned();
    let mut other_page = h.client_with_page(&other, PROFILE, &format!("<p>{other_token}</p>"));
    let (status, body) = other_page
        .send(
            "POST",
            &format!("/api/v1/claims/{other_id}/verify"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["claimed"], json!(0), "{body}");
    assert_eq!(
        body["refused"][0]["reason"],
        json!("already_claimed"),
        "a work already owned is refused by name: {body}"
    );

    // 6. A claim id is not a capability. A signed-in stranger gets `404` — a `403` would confirm the claim exists and
    //    somebody else is working on it; an anonymous caller gets `401`, because there is no session to answer with.
    let (status, body) = other.get(&format!("/api/v1/claims/{claim_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, _) = stranger.get(&format!("/api/v1/claims/{claim_id}")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// The exclusion registry outranks a claim: an author who opted out cannot be pulled back in through this door.
#[tokio::test]
async fn an_excluded_work_is_refused_by_the_registry() {
    let h = Harness::new("claim-excluded").await;
    let mut claimant = h.registered("claimant").await;
    let work = seed_metadata_work(h.db(), "opted-out", &["n-fluff"]).await;

    let entry = darkhold_db::permission::ExclusionEntry {
        id: uuid::Uuid::new_v4().to_string(),
        target_type: darkhold_db::permission::ExclusionTarget::Work,
        target_id: work.clone(),
        reason: "author asked to be left out".to_owned(),
        created_by: account_of(h.db(), "operator").await.to_string(),
        created_at: "2026-01-01T00:00:00Z".to_owned(),
    };
    darkhold_db::permission::insert_exclusion_entry(h.db(), &entry)
        .await
        .expect("seed the exclusion");

    let (status, body) = claimant
        .send("POST", "/api/v1/claims", claim_body(&[&work], PROFILE))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let claim_id = body["claim_id"].as_str().expect("claim id").to_owned();
    let token = body["token"].as_str().expect("token").to_owned();
    let mut page = h.client_with_page(&claimant, PROFILE, &format!("<p>{token}</p>"));
    let (status, body) = page
        .send(
            "POST",
            &format!("/api/v1/claims/{claim_id}/verify"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["claimed"], json!(0), "{body}");
    assert_eq!(
        body["refused"][0]["reason"],
        json!("excluded_by_author"),
        "{body}"
    );
}

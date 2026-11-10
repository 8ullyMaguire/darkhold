//! M45-41 — share attribution: a link carries the sharer's code, and both sides earn once.
//!
//! What only the routes can get wrong:
//!
//! 1. **A code belongs to a work.** A link minted for one work, used on another, attributes nothing — otherwise one popular
//!    share pays for traffic to a page the sharer never recommended.
//! 2. **A visit pays once per visitor, not once per view.** A refresh loop is the cheapest way to farm a credit, and the
//!    idempotency key is what makes it impossible rather than merely discouraged.
//! 3. **Only two different people earn.** The sharer following their own link is one reader looking at a page, and `both
//!    sides` would mean paying somebody for agreeing with themselves.
//! 4. **Minting is idempotent per (sharer, work).** Reposting a work should reuse the link, or the same reader's second visit
//!    to the second link is a second credit.
//! 5. **An anonymous reader is nobody to credit.** Attribution needs an account on both sides; a cookie-clearing visitor must
//!    not be able to earn for one.
//!
//! Both engines: `TestDb` is SQLite by default and PostgreSQL under `DARKHOLD_TEST_PG_URL`.

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

/// The pseud id of a handle — the author column on `works` is a pseud, not an account.
async fn pseud_of(db: &darkhold_db::Database, handle: &str) -> String {
    let sql = db.sql(
        "SELECT id FROM pseuds WHERE lower(handle) = lower(?)",
        "SELECT id::text AS id FROM pseuds WHERE lower(handle) = lower(?)",
    );
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("the pseud"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(handle)
            .fetch_one(db.postgres_pool().expect("pg"))
            .await
            .expect("the pseud"),
    }
}

async fn seed_work(db: &darkhold_db::Database, owner: &str, title: &str, tags: &[&str]) -> String {
    let pseud = pseud_of(db, owner).await;
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
        &[&work, &pseud, title],
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

#[tokio::test]
async fn a_share_link_pays_both_sides_once_per_visitor() {
    let h = Harness::new("share").await;
    let mut sharer = h.registered("sharer").await;
    let mut visitor = h.registered("visitor").await;
    let mut bystander = h.registered("bystander").await;
    let mut anonymous = h.client();

    let work = seed_work(h.db(), "sharer", "share-me", &["n-angst"]).await;
    let other = seed_work(h.db(), "sharer", "some-other-work", &["n-fluff"]).await;

    // --- 1. minting is idempotent per (sharer, work) -------------------------------------------------
    let (status, body) = sharer
        .send("POST", &format!("/api/v1/works/{work}/share"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let code = body["code"].as_str().expect("a code").to_owned();
    let url = body["url"].as_str().expect("a url").to_owned();
    assert!(url.contains(&code), "the link carries the code: {body}");

    let (status, again) = sharer
        .send("POST", &format!("/api/v1/works/{work}/share"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert_eq!(
        again["code"],
        json!(code),
        "reposting a work reuses its link: a second code would pay a second time for the same reader"
    );

    // --- 2. a visitor earns for both sides, once -----------------------------------------------------
    let sharer_before = share_credits(&mut sharer).await;
    let visitor_before = share_credits(&mut visitor).await;
    let (status, _) = visitor
        .get(&format!("/api/v1/works/{work}?ref={code}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        share_credits(&mut sharer).await,
        sharer_before + 1,
        "the sharer is paid for the visit"
    );
    assert_eq!(
        share_credits(&mut visitor).await,
        visitor_before + 1,
        "and so is the visitor: `both sides` is the whole point"
    );

    // --- 3. the same visitor again is the same visit -------------------------------------------------
    let sharer_after = share_credits(&mut sharer).await;
    let visitor_after = share_credits(&mut visitor).await;
    let (status, _) = visitor
        .get(&format!("/api/v1/works/{work}?ref={code}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        share_credits(&mut sharer).await,
        sharer_after,
        "a refresh is not a second reader"
    );
    assert_eq!(share_credits(&mut visitor).await, visitor_after);

    // --- 4. the sharer following their own link earns nothing -----------------------------------------
    let sharer_self = share_credits(&mut sharer).await;
    let (status, _) = sharer
        .get(&format!("/api/v1/works/{work}?ref={code}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        share_credits(&mut sharer).await,
        sharer_self,
        "one person agreeing with themselves is not an attribution"
    );

    // --- 5. a plain visit with no code earns nothing --------------------------------------------------
    let bystander_before = share_credits(&mut bystander).await;
    let sharer_before = share_credits(&mut sharer).await;
    let (status, _) = bystander.get(&format!("/api/v1/works/{work}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(share_credits(&mut bystander).await, bystander_before);
    assert_eq!(share_credits(&mut sharer).await, sharer_before);

    // --- 6. a code used on the wrong work attributes nothing ------------------------------------------
    let bystander_before = share_credits(&mut bystander).await;
    let sharer_before = share_credits(&mut sharer).await;
    let (status, _) = bystander
        .get(&format!("/api/v1/works/{other}?ref={code}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        share_credits(&mut bystander).await,
        bystander_before,
        "the code names a work; on another work it is not an attribution"
    );
    assert_eq!(share_credits(&mut sharer).await, sharer_before);

    // --- 7. an anonymous reader is nobody to credit ---------------------------------------------------
    let sharer_before = share_credits(&mut sharer).await;
    let (status, _) = anonymous
        .get(&format!("/api/v1/works/{work}?ref={code}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        share_credits(&mut sharer).await,
        sharer_before,
        "an unauthenticated visitor has no balance to earn into"
    );
}

/// The `share` bucket only, so an unrelated tip or bounty cannot make this suite pass or fail.
async fn share_credits(client: &mut Client) -> i64 {
    let (status, body) = client.get("/api/v1/credits").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["balances"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["bucket"] == json!("share"))
                .filter_map(|row| row["amount_bp"].as_i64())
                .sum()
        })
        .unwrap_or(0)
}

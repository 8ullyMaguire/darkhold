//! M45-40 — §0.3 curator lenses: a rec-blogger publishes a view, and the operator's own lens stays invisible.
//!
//! What only the routes can get wrong:
//!
//! 1. **A lens is published by its owner, nobody else.** Publishing is a claim to be looking through this person's eyes,
//!    so the handle in the path must be the caller's own.
//! 2. **Only a rec-blogger may publish one.** The role is what makes the view worth reading to somebody else; a plain
//!    reader holding a lens door open would be a lens of one reader, published.
//! 3. **An unpublished lens is `404`, and so is one that never existed.** A `403` would confirm that some curator exists
//!    under that handle and has a view — which is a fact about them, not a refusal to the asker.
//! 4. **The operator's lens is refused even when published.** §0.3 forbids the operator's lens being inferable through any
//!    user-facing label, and a catalogue ordered by it, served to the public, is the strongest possible label.
//! 5. **The ordering is the curator's, not the catalogue's.** The candidate set comes from the real search entry point, so
//!    visibility and blocked tags are the search's business; only the order is the lens's.
//!
//! Both engines: `TestDb` is SQLite by default and PostgreSQL under `DARKHOLD_TEST_PG_URL`.

//! What only the routes can get wrong:
//!
//! 1. **The document is readable with no session at all.** The reader who needs it is holding a dead
//!    link and may never have had an account here.
//! 2. **A plan with no export is refused** — the refusal §11.11a exists to make.
//! 3. **A non-operator gets 404, not 403**, for set, confirm and withdraw.
//! 4. **Resolution takes the longest matching prefix**, so a rescued work can override a host-wide
//!    pointer, and a path matches only at a segment boundary.
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

/// The whole door, in the order a reader meets it.
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

/// Grant the rec-blogger role. `curator_roles.account_id` is the account, not the pseud.
async fn grant_curator(db: &darkhold_db::Database, handle: &str) {
    let account = account_of(db, handle).await.to_string();
    test_support::exec_binds(
        db,
        "INSERT INTO curator_roles (account_id, opted_in_at, opted_in_by)
         VALUES (?, '2026-01-01T00:00:00Z', 'operator') ON CONFLICT DO NOTHING",
        "INSERT INTO curator_roles (account_id, opted_in_at, opted_in_by)
         VALUES ($1::uuid, '2026-01-01T00:00:00Z', 'operator') ON CONFLICT DO NOTHING",
        &[&account],
    )
    .await;
}

/// Publish one public work carrying `tags`, owned by `owner`.
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

/// The curator leaves a kudos — the only taste signal that is theirs to give and nobody else's to infer.
async fn kudos(db: &darkhold_db::Database, handle: &str, work: &str) {
    let account = account_of(db, handle).await.to_string();
    test_support::exec_binds(
        db,
        "INSERT INTO work_kudos (work_id, account_id, created_at) VALUES (?, ?, '2026-01-01T00:00:00Z')",
        "INSERT INTO work_kudos (work_id, account_id, created_at) VALUES ($1::uuid, $2::uuid, '2026-01-01T00:00:00Z')",
        &[work, &account],
    )
    .await;
}

#[tokio::test]
async fn a_published_lens_orders_the_catalogue_and_the_operators_lens_is_refused() {
    let h = Harness::new("lens").await;

    // --- a curator with taste, and three works to look at ---------------------------------------------
    let mut curie = h.registered("curie").await; // rec-blogger
    let mut plain = h.registered("plain").await; // reader, no role
    let mut anon = h.client();
    grant_curator(h.db(), "curie").await;

    // Two works the curator liked establish the weights: `n-angst` twice, `n-fluff` once.
    let liked_both = seed_work(h.db(), "curie", "liked-both", &["n-angst", "n-fluff"]).await;
    let liked_angst = seed_work(h.db(), "curie", "liked-angst", &["n-angst"]).await;
    kudos(h.db(), "curie", &liked_both).await;
    kudos(h.db(), "curie", &liked_angst).await;

    // Three works the curator never touched. Only the tags decide, and each has a distinct overlap:
    // 3 (angst+fluff), 2 (angst), 1 (fluff) — so the order cannot be a coincidence of ties.
    let both = seed_work(h.db(), "curie", "cand-both", &["n-angst", "n-fluff"]).await;
    let angst = seed_work(h.db(), "curie", "cand-angst", &["n-angst"]).await;
    let fluff = seed_work(h.db(), "curie", "cand-fluff", &["n-fluff"]).await;

    // --- 1. nobody has published anything -------------------------------------------------------------
    let (status, body) = anon.get("/api/v1/lens").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["lenses"], json!([]), "{body}");
    let (status, _) = anon.get("/api/v1/lens/curie").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an unpublished lens is not a lens"
    );

    // --- 2. a plain reader may not publish, and cannot publish under somebody else's name --------------
    let (status, _) = plain
        .send("PUT", "/api/v1/lens/plain", json!({ "blurb": "my taste" }))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a lens needs the rec-blogger role"
    );
    let (status, _) = curie
        .send(
            "PUT",
            "/api/v1/lens/plain",
            json!({ "blurb": "their taste" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a lens is its owner's to publish: nobody publishes a view in somebody else's name"
    );

    // --- 3. the curator publishes ------------------------------------------------------------------
    let (status, body) = curie
        .send(
            "PUT",
            "/api/v1/lens/curie",
            json!({ "blurb": "slow burns, no fluff" }),
        )
        .await;
    assert!(status.is_success(), "{body}");

    let (status, body) = anon.get("/api/v1/lens").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let lenses = body["lenses"].as_array().expect("lenses");
    assert_eq!(lenses.len(), 1, "{body}");
    assert_eq!(lenses[0]["handle"], json!("curie"), "{body}");
    assert_eq!(lenses[0]["blurb"], json!("slow burns, no fluff"), "{body}");

    // --- 4. the lens orders the catalogue: `b` first, because that is what this curator liked ----------
    let (status, body) = anon.get("/api/v1/lens/curie").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let works = body["works"].as_array().expect("works");
    let ids: Vec<&str> = works.iter().filter_map(|w| w["work_id"].as_str()).collect();
    let position = |id: &str| ids.iter().position(|served| *served == id);
    let both_at = position(&both).expect("the best-overlap work is served");
    let angst_at = position(&angst).expect("the angst work is served");
    let fluff_at = position(&fluff).expect("the fluff work is served");
    assert!(
        both_at < angst_at && angst_at < fluff_at,
        "the lens orders by the curator's own weights (3, 2, 1), not the catalogue's: {body}"
    );
    assert_eq!(body["lens"]["handle"], json!("curie"), "{body}");
    assert_eq!(body["lens"]["basis"], json!("taste"), "{body}");

    // --- 5. the operator's lens is refused, on both doors ---------------------------------------------
    let mut op = h.signed_in("operator").await;
    grant_curator(h.db(), "operator").await;
    let (status, _) = op
        .send(
            "PUT",
            "/api/v1/lens/operator",
            json!({ "blurb": "the house view" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "§0.3: the operator's lens is not publishable at all"
    );

    // And the read door is not decoration: a row written by anything else — an older build, an import, an operator who held
    // the curator role before the rule existed — must not become a public view of the operator's taste. Seeded through the
    // store, which is exactly how such a row would arrive; the write door cannot be the only place the rule holds.
    let operator_account = account_of(h.db(), "operator").await.to_string();
    darkhold_db::lens::publish(h.db(), &operator_account, "seeded by an older build")
        .await
        .expect("seed the row the write door would have refused");
    let (status, body) = anon.get("/api/v1/lens/operator").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "§0.3: a row that exists is still not a lens this instance serves: {body}"
    );
    let (_, listed) = anon.get("/api/v1/lens").await;
    assert_eq!(
        listed["lenses"].as_array().map(|l| l.len()),
        Some(1),
        "the list must not name it either: {listed}"
    );
    darkhold_db::lens::withdraw(h.db(), &operator_account)
        .await
        .expect("clean up the seeded row");

    // --- 6. withdrawing a lens puts it back out of reach ----------------------------------------------
    let (status, _) = curie.request("DELETE", "/api/v1/lens/curie", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = anon.get("/api/v1/lens/curie").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, body) = anon.get("/api/v1/lens").await;
    assert_eq!(body["lenses"], json!([]), "{body}");
}

/// A lens whose curator never liked anything still answers: it is the catalogue's order, and the document says so rather
/// than pretending the curator has taste it does not have.
#[tokio::test]
async fn a_lens_with_no_signal_serves_the_catalogue_order_and_says_why() {
    let h = Harness::new("lens-empty").await;
    let mut curie = h.registered("curie").await;
    grant_curator(h.db(), "curie").await;
    let ids = [
        seed_work(h.db(), "curie", "empty-a", &["n-angst"]).await,
        seed_work(h.db(), "curie", "empty-b", &["n-fluff"]).await,
    ];
    let (status, _) = curie
        .send(
            "PUT",
            "/api/v1/lens/curie",
            json!({ "blurb": "no kudos yet" }),
        )
        .await;
    assert!(status.is_success());

    let mut anon = h.client();
    let (status, body) = anon.get("/api/v1/lens/curie").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["lens"]["basis"],
        json!("catalogue"),
        "with no signal the lens must say it is the catalogue's order: {body}"
    );
    let served: Vec<&str> = body["works"]
        .as_array()
        .expect("works")
        .iter()
        .filter_map(|w| w["work_id"].as_str())
        .collect();
    for id in ids {
        assert!(served.contains(&id.as_str()), "{body}");
    }
}

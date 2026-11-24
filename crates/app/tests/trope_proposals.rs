//! M45-77 Step 10: dual-engine acceptance for community trope proposals.
//!
//! Every case runs against the same `TestDb` the app router uses, so SQLite and
//! PostgreSQL exercise identical handler logic. PostgreSQL is selected only by
//! `DARKHOLD_TEST_PG_URL`; no case invents a session or bypasses a route.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use serde_json::{json, Value};
use test_support::{exec_pair, scratch_dir, set_account_trust, sign_in_as, TestClient, TestDb};

const NOW: &str = "2026-01-01T00:00:00Z";

struct Harness {
    tdb: TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str, min_trust: u8, quorum: u8) -> Self {
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        let mut config = Config::development_defaults();
        config.storage.root = dir;
        config.tropes.proposal_min_trust = i64::from(min_trust);
        config.tropes.quorum = u32::from(quorum);
        config.tropes.voting_days = 7;
        // These tests create several accounts per database; the operator's
        // default auth burst is a real gate, not part of this governance rule.
        for quota in [
            &mut config.rate_limits.auth,
            &mut config.rate_limits.write,
            &mut config.rate_limits.search,
            &mut config.rate_limits.default,
        ] {
            quota.burst = quota.burst.saturating_mul(100);
        }
        Self { tdb, config }
    }

    fn client(&self) -> TestClient {
        TestClient::new(server::build_router(AppState::new(
            self.config.clone(),
            self.tdb.db().clone(),
        )))
    }

    async fn account(
        &self,
        email: &str,
        handle: &str,
        level: u8,
        curator: bool,
        role: bool,
    ) -> (TestClient, String) {
        let mut client = self.client();
        let account = sign_in_as(&mut client, &self.tdb, email, handle).await;
        set_account_trust(self.tdb.db(), &account, level).await;
        if curator {
            exec_pair(
                self.tdb.db(),
                &format!("UPDATE accounts SET is_curator = true WHERE id = '{account}'"),
                &format!("UPDATE accounts SET is_curator = true WHERE id = '{account}'"),
            )
            .await;
        }
        if role {
            exec_pair(
                self.tdb.db(),
                &format!("INSERT INTO curator_roles (account_id, opted_in_by) VALUES ('{account}','tests')"),
                &format!("INSERT INTO curator_roles (account_id, opted_in_by) VALUES ('{account}','tests')"),
            )
            .await;
        }
        (client, account)
    }
}

async fn tag(h: &Harness, name: &str) {
    let suffix = match name {
        "alpha" => 1_u64,
        "beta" => 2,
        _ => 3 + name.len() as u64,
    };
    let id = format!("01957a00-0000-4000-8000-{suffix:012x}");
    // PostgreSQL's canonical_entities has a UUID id that the SQLite arm does
    // not require; the name is the identity and uses the renderer's norm.
    exec_pair(
        h.tdb.db(),
        &format!("INSERT INTO canonical_entities (kind,norm,canonical,review_status,signal_count,created_at,updated_at) VALUES ('tag','{name}','{name}','unverified',1,'{NOW}','{NOW}')"),
        &format!("INSERT INTO canonical_entities (id,kind,norm,canonical,review_status,signal_count,created_at,updated_at) VALUES ('{id}'::uuid,'tag','{name}','{name}','unverified',1,'{NOW}','{NOW}')"),
    )
    .await;
}

fn proposal_json(slug: &str, members: &[&str]) -> Value {
    json!({
        "slug": slug,
        "action": "create",
        "label": slug,
        "summary": "acceptance fixture",
        "members": members,
        "target_slug": null,
        "target_version": null,
    })
}

async fn propose(client: &mut TestClient, slug: &str, members: &[&str]) -> (StatusCode, Value) {
    client
        .post("/api/v1/tropes/proposals", proposal_json(slug, members))
        .await
}

async fn vote(client: &mut TestClient, id: &str, value: &str) -> (StatusCode, Value) {
    client
        .post(
            &format!("/api/v1/tropes/proposals/{id}/vote"),
            json!({ "value": value }),
        )
        .await
}

async fn pass_group(tag_name: &str) -> (Harness, String, String, String) {
    let h = Harness::new(tag_name, 4, 2).await;
    tag(&h, "alpha").await;
    tag(&h, "beta").await;
    let (mut proposer, _) = h
        .account("proposer@trope.test", "propsone", 4, false, false)
        .await;
    let (status, body) = propose(&mut proposer, tag_name, &["alpha", "beta"]).await;
    assert_eq!(status, StatusCode::CREATED, "proposal: {body}");
    let id = body["id"].as_str().expect("proposal id").to_string();
    for (i, value) in ["yes", "yes"].iter().enumerate() {
        let (mut voter, _) = h
            .account(
                &format!("voter{i}@trope.test"),
                &format!("voter{i}"),
                4,
                false,
                false,
            )
            .await;
        let (status, body) = vote(&mut voter, &id, value).await;
        assert_eq!(status, StatusCode::OK, "vote {i}: {body}");
    }
    let version = darkhold_db::trope_governance::active_group_details(h.tdb.db())
        .await
        .expect("active groups")
        .into_iter()
        .find(|group| group.slug == tag_name)
        .expect("the passed group became active")
        .version;
    (h, id, tag_name.to_string(), version)
}

#[tokio::test]
async fn tl2_post_is_403_naming_the_bar_and_level() {
    let h = Harness::new("tl2_door", 4, 2).await;
    tag(&h, "alpha").await;
    let (mut client, _) = h.account("tl2@trope.test", "tl2one", 2, false, false).await;
    let (status, body) = propose(&mut client, "tl-two", &["alpha"]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let text = body.to_string();
    assert!(text.contains("4"), "bar must be named: {text}");
    assert!(text.contains("2"), "level must be named: {text}");
}

#[tokio::test]
async fn tl4_post_is_201_with_proposed_status_and_quorum() {
    let h = Harness::new("tl4_door", 4, 2).await;
    tag(&h, "alpha").await;
    assert!(
        darkhold_db::taxonomy::tag_exists(h.tdb.db(), "alpha")
            .await
            .expect("tag lookup"),
        "the fixture tag must be found"
    );
    let (mut client, _) = h.account("tl4@trope.test", "tl4one", 4, false, false).await;
    let (status, body) = propose(&mut client, "tl-four", &["alpha"]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["status"], "proposed");
    assert_eq!(body["quorum_needed"], 2);
}

#[tokio::test]
async fn both_curator_forms_may_propose() {
    let h = Harness::new("curator_forms", 4, 2).await;
    tag(&h, "alpha").await;
    let (mut flag, _) = h
        .account("flag@trope.test", "flagcurator", 2, true, false)
        .await;
    let (status, body) = propose(&mut flag, "flag-group", &["alpha"]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (mut role, _) = h
        .account("role@trope.test", "rolecurator", 2, false, true)
        .await;
    let (status, body) = propose(&mut role, "role-group", &["alpha"]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn a_seed_slug_is_refused() {
    let h = Harness::new("seed_dup", 4, 2).await;
    tag(&h, "alpha").await;
    let (mut client, _) = h
        .account("seed@trope.test", "seedone", 4, false, false)
        .await;
    let (status, body) = propose(&mut client, "system-grind", &["alpha"]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("system-grind"), "{body}");
}

#[tokio::test]
async fn an_unknown_member_is_named() {
    let h = Harness::new("unknown_member", 4, 2).await;
    tag(&h, "alpha").await;
    let (mut client, _) = h
        .account("unknown@trope.test", "unknownone", 4, false, false)
        .await;
    let (status, body) =
        propose(&mut client, "unknown-group", &["alpha", "ghost", "phantom"]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let text = body.to_string();
    assert!(text.contains("ghost") && text.contains("phantom"), "{text}");
}

#[tokio::test]
async fn eleven_members_are_refused() {
    let h = Harness::new("too_many", 4, 2).await;
    let (mut client, _) = h
        .account("many@trope.test", "manyone", 4, false, false)
        .await;
    let members = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"];
    let (status, body) = propose(&mut client, "too-many", &members).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let text = body.to_string();
    assert!(text.contains("10") || text.contains("budget"), "{text}");
}

#[tokio::test]
async fn the_proposer_cannot_vote() {
    let h = Harness::new("self_vote", 4, 2).await;
    tag(&h, "alpha").await;
    let (mut client, _) = h
        .account("self@trope.test", "selfone", 4, false, false)
        .await;
    let (status, body) = propose(&mut client, "self-group", &["alpha"]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("proposal id");
    let (status, _) = vote(&mut client, id, "yes").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn two_yes_votes_pass_and_the_group_matches_its_tags() {
    let (h, _id, slug, _) = pass_group("pass-group").await;
    let mut client = h.client();
    let (status, body) = client.get("/api/v1/tropes").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let groups = body["items"].as_array().expect("items");
    assert!(
        groups
            .iter()
            .any(|g| g["slug"] == slug && g["source"] == "community"),
        "{body}"
    );

    // The one parser must produce exactly the AST of the member tags OR-ed
    // together; this is the resolver-level equivalent of the reader's search.
    let extra = darkhold_db::trope_governance::active_community_groups(h.tdb.db())
        .await
        .expect("active groups");
    let slugs = extra
        .iter()
        .map(|(slug, _)| slug.clone())
        .collect::<Vec<_>>();
    let resolver = |candidate: &str| -> Option<Vec<String>> {
        extra
            .iter()
            .find(|(s, _)| s == candidate)
            .map(|(_, members)| members.clone())
    };
    let trope = darkhold_domain::query::parse_query_with_tropes(
        &format!("trope:{slug}"),
        darkhold_domain::query::QueryBudget::default(),
        &resolver,
        &slugs,
    )
    .expect("trope query parses");
    let tags = darkhold_domain::query::parse_query("tag:alpha OR tag:beta")
        .expect("equivalent tag query parses");
    assert_eq!(trope, tags, "the resolver must expand to the same AST");
}

#[tokio::test]
async fn a_second_vote_from_one_account_changes_nothing() {
    let h = Harness::new("double_vote", 4, 2).await;
    tag(&h, "alpha").await;
    let (mut proposer, _) = h.account("dvp@trope.test", "dvpone", 4, false, false).await;
    let (status, body) = propose(&mut proposer, "dv-group", &["alpha"]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("proposal id").to_string();
    let (mut voter, _) = h.account("dvv@trope.test", "dvvone", 4, false, false).await;
    let (_, first) = vote(&mut voter, &id, "yes").await;
    let (_, second) = vote(&mut voter, &id, "no").await;
    assert_eq!(second["changed"], false, "{second}");
    assert_eq!(first["proposal_id"], second["proposal_id"]);
    let row = darkhold_db::trope_governance::get_proposal(h.tdb.db(), &id)
        .await
        .expect("read proposal")
        .expect("proposal exists");
    assert_eq!((row.yes_votes, row.no_votes), (1, 0));
}

#[tokio::test]
async fn two_no_votes_fail_without_an_active_group() {
    let h = Harness::new("no_votes", 4, 2).await;
    tag(&h, "alpha").await;
    let (mut proposer, _) = h.account("nvp@trope.test", "nvpone", 4, false, false).await;
    let (status, body) = propose(&mut proposer, "no-group", &["alpha"]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("proposal id").to_string();
    for i in 0..2 {
        let (mut voter, _) = h
            .account(
                &format!("nvv{i}@trope.test"),
                &format!("nvv{i}"),
                4,
                false,
                false,
            )
            .await;
        let (status, body) = vote(&mut voter, &id, "no").await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let row = darkhold_db::trope_governance::get_proposal(h.tdb.db(), &id)
        .await
        .expect("read proposal")
        .expect("proposal exists");
    assert_eq!(row.status, "failed");
    let active = darkhold_db::trope_governance::active_group_details(h.tdb.db())
        .await
        .expect("active groups");
    assert!(!active.iter().any(|g| g.slug == "no-group"));
}

#[tokio::test]
async fn update_with_stale_then_current_version() {
    let (h, _id, slug, version) = pass_group("update-group").await;
    let (mut editor, _) = h
        .account("editor@trope.test", "editorone", 4, false, false)
        .await;
    let mut body = proposal_json(&slug, &["alpha", "beta"]);
    body["action"] = json!("update");
    body["target_slug"] = json!(slug);
    body["target_version"] = json!("stale-version");
    let (status, body) = editor.post("/api/v1/tropes/proposals", body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let mut body = proposal_json(&slug, &["alpha"]);
    body["action"] = json!("update");
    body["target_slug"] = json!(slug);
    body["target_version"] = json!(version);
    let (status, body) = editor.post("/api/v1/tropes/proposals", body).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("update proposal id").to_string();
    for i in 0..2 {
        let (mut voter, _) = h
            .account(
                &format!("upv{i}@trope.test"),
                &format!("upv{i}"),
                4,
                false,
                false,
            )
            .await;
        let (status, body) = vote(&mut voter, &id, "yes").await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let active = darkhold_db::trope_governance::active_group_details(h.tdb.db())
        .await
        .expect("active groups");
    let group = active
        .iter()
        .find(|g| g.slug == slug)
        .expect("updated group");
    assert_eq!(group.members, vec!["alpha"]);
    assert_ne!(
        group.version, version,
        "the update carries a new version token"
    );
}

#[tokio::test]
async fn retire_removes_group_from_the_public_tropes() {
    let (h, _id, slug, version) = pass_group("retire-group").await;
    let (mut editor, _) = h
        .account("retire@trope.test", "retireone", 4, false, false)
        .await;
    let mut body = proposal_json(&slug, &["alpha", "beta"]);
    body["action"] = json!("retire");
    body["target_slug"] = json!(slug);
    body["target_version"] = json!(version);
    let (status, body) = editor.post("/api/v1/tropes/proposals", body).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("retire proposal id").to_string();
    for i in 0..2 {
        let (mut voter, _) = h
            .account(
                &format!("rtv{i}@trope.test"),
                &format!("rtv{i}"),
                4,
                false,
                false,
            )
            .await;
        let (status, body) = vote(&mut voter, &id, "yes").await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let mut client = TestClient::new(server::build_router(AppState::new(
        h.config.clone(),
        h.tdb.db().clone(),
    )));
    let (_, body) = client.get("/api/v1/tropes").await;
    assert!(
        !body
            .to_string()
            .contains(&format!("\\\"slug\\\":\\\"{slug}\\\"")),
        "{body}"
    );
}

#[tokio::test]
async fn public_reads_expose_no_account_id() {
    let (h, id, slug, _) = pass_group("public-read").await;
    let mut client = h.client();
    let (status, body) = client.get("/api/v1/tropes/proposals").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let text = body.to_string();
    // Fetch the real ids rather than guessing their values.
    let rows = match h.tdb.db().backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar::<_, String>("SELECT id FROM accounts")
            .fetch_all(h.tdb.db().sqlite_pool().expect("sqlite"))
            .await
            .expect("accounts"),
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar::<_, String>("SELECT id::text FROM accounts")
                .fetch_all(h.tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("accounts")
        }
    };
    assert!(!rows.is_empty());
    for account in &rows {
        assert!(!text.contains(account), "account id leaked: {account}");
    }
    let (status, detail) = client.get(&format!("/api/v1/tropes/proposals/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["slug"], slug);
}

#[tokio::test]
async fn an_expired_proposal_is_written_after_settle() {
    let h = Harness::new("expired_group", 4, 2).await;
    tag(&h, "alpha").await;
    let (mut proposer, _) = h.account("exp@trope.test", "expone", 4, false, false).await;
    let (status, body) = propose(&mut proposer, "expired-group", &["alpha"]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("proposal id").to_string();
    exec_pair(
        h.tdb.db(),
        &format!("UPDATE trope_proposals SET closes_at = '2001-01-01T00:00:00Z' WHERE id = '{id}'"),
        &format!("UPDATE trope_proposals SET closes_at = '2001-01-01T00:00:00Z'::timestamptz WHERE id = '{id}'::uuid"),
    )
    .await;
    let before = darkhold_db::trope_governance::get_proposal(h.tdb.db(), &id)
        .await
        .expect("read")
        .expect("proposal");
    assert_eq!(before.status, "proposed", "expiry is derived, not written");
    let report = darkhold_db::trope_governance::settle_expired(h.tdb.db(), 100, NOW)
        .await
        .expect("settle");
    assert!(report.expired >= 1, "{report:?}");
    let after = darkhold_db::trope_governance::get_proposal(h.tdb.db(), &id)
        .await
        .expect("read")
        .expect("proposal");
    assert_eq!(after.status, "expired");
}

#[tokio::test]
async fn a_one_vote_quorum_is_refused_by_the_loader() {
    use darkhold_app::cli::GlobalArgs;
    let dir = scratch_dir("trope_quorum_one_acceptance");
    let path = dir.join("darkhold.toml");
    std::fs::write(&path, "[tropes]\nquorum = 1\n").expect("write config");
    let args = GlobalArgs {
        config: Some(path),
        ..GlobalArgs::default()
    };
    let error = Config::load(&args).expect_err("quorum 1 must be refused");
    assert!(error.to_string().contains("quorum below 2"), "{error}");
}

#[tokio::test]
async fn a_split_vote_stays_proposed_until_quorum() {
    let h = Harness::new("split-vote", 4, 2).await;
    tag(&h, "alpha").await;
    let (mut proposer, _) = h
        .account("splitp@trope.test", "splitpone", 4, false, false)
        .await;
    let (status, body) = propose(&mut proposer, "split-group", &["alpha"]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("proposal id").to_string();
    for (i, value) in ["yes", "no"].iter().enumerate() {
        let (mut voter, _) = h
            .account(
                &format!("splitv{i}@trope.test"),
                &format!("splitv{i}"),
                4,
                false,
                false,
            )
            .await;
        let (status, body) = vote(&mut voter, &id, value).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let row = darkhold_db::trope_governance::get_proposal(h.tdb.db(), &id)
        .await
        .expect("read proposal")
        .expect("proposal exists");
    assert_eq!(row.status, "proposed", "one vote on each side is undecided");
}

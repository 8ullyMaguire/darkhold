//! M45-77 Step 6: community trope repository, proven on both engines.

use darkhold_db::{trope_governance as tropes, Backend, Database};
use darkhold_domain::trope_governance::effective_status;
use std::path::PathBuf;

const CREATOR: &str = "01957700-0000-4000-8000-00000000a001";
const VOTER: &str = "01957700-0000-4000-8000-00000000a002";
const NOW: &str = "2026-11-24T00:00:00Z";
const FUTURE: &str = "2026-12-31T00:00:00Z";

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-tropes-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn exec(db: &Database, sql: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await
                .unwrap_or_else(|e| panic!("fixture on SQLite: {e}"));
        }
        Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await
                .unwrap_or_else(|e| panic!("fixture on PostgreSQL: {e}"));
        }
    }
}

async fn seed_accounts(db: &Database) {
    for id in [CREATOR, VOTER] {
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) \
                 VALUES ('{id}','active','{id}@test.dev','adult','{NOW}','{NOW}',1)"
            ),
        )
        .await;
    }
}

fn payload(members: &[&str], target_version: Option<&str>) -> String {
    serde_json::json!({
        "label": "System progression",
        "summary": "Levels are the plot.",
        "members": members,
        "target_version": target_version,
    })
    .to_string()
}

fn update_payload(target_slug: &str, target_version: &str, members: &[&str]) -> String {
    serde_json::json!({
        "label": "System progression",
        "summary": "Levels are the plot.",
        "members": members,
        "target_slug": target_slug,
        "target_version": target_version,
    })
    .to_string()
}

async fn propose(db: &Database, slug: &str, action: &str, body: &str) -> String {
    tropes::insert_proposal(db, slug, action, body, 2, FUTURE, CREATOR, NOW)
        .await
        .expect("insert proposal")
}

async fn pass_with_two_yes(db: &Database, id: &str, voter2: &str) {
    tropes::insert_vote(db, id, CREATOR, "yes", NOW)
        .await
        .expect("first yes");
    assert_eq!(
        tropes::recount_and_decide(db, id, NOW, CREATOR)
            .await
            .expect("first recount"),
        None
    );
    assert!(tropes::insert_vote(db, id, voter2, "yes", NOW)
        .await
        .expect("second yes"));
    assert_eq!(
        tropes::recount_and_decide(db, id, NOW, voter2)
            .await
            .expect("second recount"),
        Some(true)
    );
}

#[tokio::test]
async fn two_distinct_yes_votes_pass_and_apply() {
    let tdb = test_support::TestDb::connect_with_dir("tg-two-yes", &scratch("two-yes")).await;
    let db = tdb.db();
    seed_accounts(db).await;
    let id = propose(
        db,
        "system-progression",
        "create",
        &payload(&["gamer", "litrpg"], None),
    )
    .await;
    pass_with_two_yes(db, &id, VOTER).await;

    let row = tropes::get_proposal(db, &id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, "passed", "{row:?}");
    let groups = tropes::active_community_groups(db)
        .await
        .expect("active groups");
    let group = groups
        .iter()
        .find(|(slug, _)| slug == "system-progression")
        .expect("the passed proposal is now an active group");
    assert_eq!(group.1, vec!["gamer", "litrpg"]);
    tdb.cleanup().await;
}

#[tokio::test]
async fn a_second_vote_from_the_same_account_does_not_count() {
    let tdb = test_support::TestDb::connect_with_dir("tg-duplicate", &scratch("duplicate")).await;
    let db = tdb.db();
    seed_accounts(db).await;
    let id = propose(
        db,
        "double-vote-group",
        "create",
        &payload(&["gamer"], None),
    )
    .await;
    assert!(tropes::insert_vote(db, &id, CREATOR, "yes", NOW)
        .await
        .expect("first"));
    assert!(!tropes::insert_vote(db, &id, CREATOR, "no", NOW)
        .await
        .expect("duplicate"));
    assert_eq!(
        tropes::recount_and_decide(db, &id, NOW, CREATOR)
            .await
            .expect("recount"),
        None
    );
    let row = tropes::get_proposal(db, &id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.yes_votes, 1, "{row:?}");
    assert_eq!(row.no_votes, 0, "{row:?}");
    tdb.cleanup().await;
}

#[tokio::test]
async fn two_no_votes_fail_without_applying() {
    let tdb = test_support::TestDb::connect_with_dir("tg-two-no", &scratch("two-no")).await;
    let db = tdb.db();
    seed_accounts(db).await;
    let id = propose(db, "rejected-group", "create", &payload(&["gamer"], None)).await;
    for voter in [CREATOR, VOTER] {
        tropes::insert_vote(db, &id, voter, "no", NOW)
            .await
            .expect("no vote");
        let _ = tropes::recount_and_decide(db, &id, NOW, voter)
            .await
            .expect("recount");
    }
    let row = tropes::get_proposal(db, &id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, "failed", "{row:?}");
    assert!(
        tropes::active_community_groups(db)
            .await
            .expect("active")
            .is_empty(),
        "a failed proposal must not become a group"
    );
    tdb.cleanup().await;
}

#[tokio::test]
async fn stale_target_version_refuses_an_update() {
    let tdb = test_support::TestDb::connect_with_dir("tg-stale", &scratch("stale")).await;
    let db = tdb.db();
    seed_accounts(db).await;
    let first = propose(db, "versioned-group", "create", &payload(&["gamer"], None)).await;
    pass_with_two_yes(db, &first, VOTER).await;
    let active = tropes::get_proposal(db, &first)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(active.status, "passed");

    // The live-slug index allows only one proposed/active row per slug, so an
    // update proposal has its own slug and names its target in the payload.
    let second = propose(
        db,
        "versioned-group-update",
        "update",
        &update_payload("versioned-group", "2000-01-01T00:00:00Z", &["litrpg"]),
    )
    .await;
    tropes::insert_vote(db, &second, CREATOR, "yes", NOW)
        .await
        .expect("first yes");
    let _ = tropes::recount_and_decide(db, &second, NOW, CREATOR)
        .await
        .expect("first recount");
    assert!(tropes::insert_vote(db, &second, VOTER, "yes", NOW)
        .await
        .expect("second yes"));
    let error = tropes::recount_and_decide(db, &second, NOW, VOTER)
        .await
        .expect_err("a stale target version must not be applied");
    assert!(error.to_string().contains("target version"), "{error}");
    tdb.cleanup().await;
}

#[tokio::test]
async fn expiry_is_derived_before_the_sweep_and_written_after_it() {
    let tdb = test_support::TestDb::connect_with_dir("tg-expiry", &scratch("expiry")).await;
    let db = tdb.db();
    seed_accounts(db).await;
    let id = tropes::insert_proposal(
        db,
        "old-proposal",
        "create",
        &payload(&["gamer"], None),
        2,
        "2026-01-01T00:00:00Z",
        CREATOR,
        "2025-12-01T00:00:00Z",
    )
    .await
    .expect("insert");
    let now = "2026-01-02T00:00:00Z";
    let row = tropes::get_proposal(db, &id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(
        effective_status(&row.status, &row.closes_at, now),
        "expired"
    );
    assert_eq!(row.status, "proposed", "the sweep has not run yet");

    let report = tropes::settle_expired(db, 10, now).await.expect("settle");
    assert_eq!(report.expired, 1, "{report:?}");
    let row = tropes::get_proposal(db, &id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, "expired", "{row:?}");
    tdb.cleanup().await;
}

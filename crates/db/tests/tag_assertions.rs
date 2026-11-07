//! M69-17 — a tag records who asserted it, and a model may only propose.
//!
//! The second half is the one worth testing: a model's tag must not be readable as a fact. It is asserted through the
//! **reader-facing read** (`taxonomy::tag_names_for_work`), not through this module's own listing — a proposal that is invisible in
//! the curator's view and visible on the work page would be the worst of both.
//!
//! Asserts:
//! 1. a human assertion is visible to readers, and stored as `curator`;
//! 2. a model assertion is stored as `inferred` + `unconfirmed` and is **not** visible to readers;
//! 3. a reader confirmation makes it visible;
//! 4. a model re-asserting a pair a person already confirmed does **not** demote it — a proposal cannot take a tag away;
//! 5. an author's own tag is stored as `author`;
//! 6. retraction removes the claim.

use darkhold_db::tag_assertions::{
    assert_tag, assertions_for_work, confirm, retract, Claimant, ConfirmLevel,
};
use darkhold_db::{Backend, Database, DatabaseConfig};
use std::path::PathBuf;

const AUTHOR: &str = "01955600-0000-4000-8000-000000000001";
const PSEUD: &str = "01955600-0000-4000-8000-000000000002";
const WORK: &str = "01955600-0000-4000-8000-000000000003";
const HUMAN_TAG: &str = "01955600-0000-4000-8000-000000000004";
const MODEL_TAG: &str = "01955600-0000-4000-8000-000000000005";
const OWN_TAG: &str = "01955600-0000-4000-8000-000000000006";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-assert-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn database(tag: &str) -> (Database, PathBuf) {
    let dir = scratch_dir(tag);
    let db = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let name = format!(
                "lh_test_assert_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config =
                DatabaseConfig::new(format!("sqlite://{}/assert.sqlite?mode=rwc", dir.display()));
            Database::connect(&config).await.expect("connect")
        }
    };
    db.migrate().await.expect("migrate");
    (db, dir)
}

async fn exec(db: &Database, statement: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(statement)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
        Backend::Postgres => {
            sqlx::query(statement)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|error| panic!("{statement}\n{error}"));
        }
    }
}

async fn seed(db: &Database) {
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
             VALUES ('{AUTHOR}','active','author@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{AUTHOR}','assertor','Assertor','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, completion,
                                created_at, updated_at, version)
             VALUES ('{WORK}','{PSEUD}','Asserted','published','public','complete',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    for (node, canonical) in [
        (HUMAN_TAG, "slow burn"),
        (MODEL_TAG, "model guess"),
        (OWN_TAG, "author tag"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at)
                 VALUES ('{node}','tag','{canonical}','{canonical}','2026-01-01T00:00:00Z')"
            ),
        )
        .await;
    }
}

#[tokio::test]
async fn a_model_may_only_propose_and_readers_never_see_a_proposal() {
    let (db, dir) = database("proposal").await;
    seed(&db).await;

    // 1. A human assertion is a fact, and says who made it.
    assert_tag(&db, WORK, HUMAN_TAG, Claimant::Human, false, 0.9)
        .await
        .expect("human assertion");
    assert_eq!(
        darkhold_db::taxonomy::tag_names_for_work(&db, WORK)
            .await
            .expect("tags"),
        vec!["slow burn".to_owned()]
    );

    // 5. The author's own tag is `author`, not `curator`.
    assert_tag(&db, WORK, OWN_TAG, Claimant::Human, true, 1.0)
        .await
        .expect("author assertion");
    let own = assertions_for_work(&db, WORK)
        .await
        .expect("assertions")
        .into_iter()
        .find(|a| a.node_id == OWN_TAG)
        .expect("author row");
    assert_eq!(own.source, "author");

    // 2. A model's inference is recorded and invisible to readers.
    assert_tag(&db, WORK, MODEL_TAG, Claimant::Model, false, 0.4)
        .await
        .expect("model proposal");
    let visible = darkhold_db::taxonomy::tag_names_for_work(&db, WORK)
        .await
        .expect("tags");
    assert!(
        !visible.iter().any(|tag| tag == "model guess"),
        "a proposal is not a fact: {visible:?}"
    );
    let proposal = assertions_for_work(&db, WORK)
        .await
        .expect("assertions")
        .into_iter()
        .find(|a| a.node_id == MODEL_TAG)
        .expect("the proposal is recorded");
    assert_eq!(proposal.source, "inferred");
    assert_eq!(proposal.confirmation, "unconfirmed");
    assert!(proposal.is_proposal());

    // 4. A model re-asserting a confirmed pair cannot demote it.
    assert_tag(&db, WORK, HUMAN_TAG, Claimant::Model, false, 0.2)
        .await
        .expect("re-assert");
    let human = assertions_for_work(&db, WORK)
        .await
        .expect("assertions")
        .into_iter()
        .find(|a| a.node_id == HUMAN_TAG)
        .expect("human row");
    assert_eq!(
        human.source, "curator",
        "a model cannot overwrite the row a person stands behind"
    );
    let visible = darkhold_db::taxonomy::tag_names_for_work(&db, WORK)
        .await
        .expect("tags");
    assert!(
        visible.iter().any(|tag| tag == "slow burn"),
        "and the tag does not disappear from the reader's page: {visible:?}"
    );

    // 3. A confirmation makes it a fact.
    assert!(confirm(&db, WORK, MODEL_TAG, ConfirmLevel::Reader)
        .await
        .expect("confirm"));
    let visible = darkhold_db::taxonomy::tag_names_for_work(&db, WORK)
        .await
        .expect("tags");
    assert!(
        visible.iter().any(|tag| tag == "model guess"),
        "a confirmed inference is a fact: {visible:?}"
    );

    // 6. Retraction removes it.
    assert!(retract(&db, WORK, MODEL_TAG).await.expect("retract"));
    let visible = darkhold_db::taxonomy::tag_names_for_work(&db, WORK)
        .await
        .expect("tags");
    assert!(!visible.iter().any(|tag| tag == "model guess"));

    let _ = std::fs::remove_dir_all(dir);
}

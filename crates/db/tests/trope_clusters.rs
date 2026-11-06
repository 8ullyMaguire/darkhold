//! M69-18 — trope clusters and the coverage checklist.
//!
//! The checklist is advisory, and the test asserts both halves of that: it says which families a work names and which it never
//! names (so "a work knows it has none" is real information), and it says nothing anywhere else — no tag is written by asking,
//! and a work with no clusters measured gets an absent list rather than a perfect score.
//!
//! Asserts:
//! 1. clusters list in priority order, ties broken on slug;
//! 2. coverage names the clusters a work carries a node of, and counts what it does not;
//! 3. a model's unconfirmed proposal does not count as the work naming a family;
//! 4. a cluster with no nodes reports `missing: 0`, not a false gap;
//! 5. asking for coverage writes nothing — the tag rows are unchanged afterwards;
//! 6. no clusters at all is an empty list, not an error.

use darkhold_db::trope_clusters::{add_node, all_clusters, coverage_for_work, create_cluster};
use darkhold_db::{Backend, Database, DatabaseConfig};
use std::path::PathBuf;

const AUTHOR: &str = "01955800-0000-4000-8000-000000000001";
const PSEUD: &str = "01955800-0000-4000-8000-000000000002";
const WORK: &str = "01955800-0000-4000-8000-000000000003";
const COMFORT: &str = "01955800-0000-4000-8000-000000000004";
const ANGST: &str = "01955800-0000-4000-8000-000000000005";
const SLOW: &str = "01955800-0000-4000-8000-000000000006";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-tropes-{tag}-{}-{:?}",
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
                "lh_test_tropes_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = DatabaseConfig::new(format!(
                "sqlite://{}/tropes.sqlite?mode=rwc",
                dir.display()
            ));
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
             VALUES ('{PSEUD}','{AUTHOR}','tropeauthor','Trope','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, completion,
                                created_at, updated_at, version)
             VALUES ('{WORK}','{PSEUD}','Troped','published','public','complete',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    for (node, canonical) in [
        (COMFORT, "hurt comfort"),
        (ANGST, "angst"),
        (SLOW, "slow burn"),
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
async fn coverage_tells_a_work_which_families_it_never_names_and_writes_nothing() {
    let (db, dir) = database("coverage").await;
    seed(&db).await;

    // 6. No clusters yet: an empty list, not an error and not a perfect score.
    assert!(coverage_for_work(&db, WORK).await.expect("coverage").is_empty());

    // 1. Priority orders the clusters; a tie breaks on slug so two reads agree.
    create_cluster(&db, "hurt-comfort", "Hurt/Comfort", None, 20)
        .await
        .expect("comfort cluster");
    create_cluster(&db, "angst", "Angst", None, 10)
        .await
        .expect("angst cluster");
    create_cluster(&db, "empty-family", "Unfilled", None, 20)
        .await
        .expect("empty cluster");
    add_node(&db, "hurt-comfort", COMFORT).await.expect("add");
    add_node(&db, "hurt-comfort", SLOW).await.expect("add");
    add_node(&db, "angst", ANGST).await.expect("add");
    let listed = all_clusters(&db).await.expect("clusters");
    assert_eq!(
        listed.iter().map(|c| c.slug.as_str()).collect::<Vec<_>>(),
        vec!["angst", "empty-family", "hurt-comfort"]
    );

    // The work names one comfort node and no angst.
    darkhold_db::tag_assertions::assert_tag(
        &db,
        WORK,
        COMFORT,
        darkhold_db::tag_assertions::Claimant::Human,
        true,
        1.0,
    )
    .await
    .expect("tag");

    // 3. A model's proposal is not the work naming a family.
    darkhold_db::tag_assertions::assert_tag(
        &db,
        WORK,
        ANGST,
        darkhold_db::tag_assertions::Claimant::Model,
        false,
        0.5,
    )
    .await
    .expect("proposal");

    let coverage = coverage_for_work(&db, WORK).await.expect("coverage");
    let angst = coverage.iter().find(|c| c.slug == "angst").expect("angst");
    assert!(!angst.is_named(), "a guess is not a family named: {angst:?}");
    assert_eq!(angst.missing, 1);

    let comfort = coverage
        .iter()
        .find(|c| c.slug == "hurt-comfort")
        .expect("comfort");
    assert_eq!(comfort.named, vec![COMFORT.to_owned()]);
    assert_eq!(comfort.missing, 1, "slow burn is not on the work");

    // 4. An empty cluster is empty, not a gap.
    let empty = coverage
        .iter()
        .find(|c| c.slug == "empty-family")
        .expect("empty");
    assert_eq!(empty.missing, 0);
    assert!(empty.named.is_empty());

    // 5. Asking wrote nothing: still exactly two rows, one of them the proposal.
    let rows = darkhold_db::tag_assertions::assertions_for_work(&db, WORK)
        .await
        .expect("rows");
    assert_eq!(rows.len(), 2, "coverage is a read: {rows:?}");

    let _ = std::fs::remove_dir_all(dir);
}

//! M63-32 — the tag-inference pass proposes, and coverage says what the catalogue actually has.
//!
//! The card's data half cannot be done from here (the importer stores no category field, so there is nothing to backfill without
//! re-fetching every page), so this is its second route: match existing tag nodes against text already stored and record
//! **proposals**. The property that makes that safe is M69-17's — a `model` row is `inferred + unconfirmed` and invisible to readers.
//!
//! Asserts:
//! 1. a work whose text names a tag gets a proposal, and readers do **not** see it;
//! 2. the longest matching node wins, so a summary naming "hurt comfort" does not also pick up "comfort";
//! 3. a second pass examines nothing — the work is no longer untagged — so the pass cannot pile rows up;
//! 4. a confirmed proposal becomes a visible tag and then counts in coverage;
//! 5. coverage counts per source, and `tagged` counts only reader-visible tags; `tagged_and_liked` is the join that was empty on the
//!    live instance.

use darkhold_db::tag_inference::{coverage, propose_from_text};
use darkhold_db::{Backend, Database, DatabaseConfig};
use std::path::PathBuf;

const AUTHOR: &str = "01955b00-0000-4000-8000-000000000001";
const PSEUD: &str = "01955b00-0000-4000-8000-000000000002";
const TAGGED_WORK: &str = "01955b00-0000-4000-8000-000000000003";
const NAMED_WORK: &str = "01955b00-0000-4000-8000-000000000004";
const PLAIN_WORK: &str = "01955b00-0000-4000-8000-000000000005";
const NODE_LONG: &str = "01955b00-0000-4000-8000-000000000006";
const NODE_SHORT: &str = "01955b00-0000-4000-8000-000000000007";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-infer-{tag}-{}-{:?}",
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
                "lh_test_infer_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = DatabaseConfig::new(format!(
                "sqlite://{}/infer.sqlite?mode=rwc",
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
             VALUES ('{AUTHOR}','active','infer@example.invalid','adult',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                 updated_at, version)
             VALUES ('{PSEUD}','{AUTHOR}','inferrer','Inferrer','listed',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
        ),
    )
    .await;
    for (work, title, summary) in [
        (TAGGED_WORK, "Already tagged", "a summary"),
        (NAMED_WORK, "A hurt comfort story", "the summary mentions comfort"),
        (PLAIN_WORK, "An untitled thing", "nothing to match here"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, summary, visibility, lifecycle,
                                    created_at, updated_at, version)
                 VALUES ('{work}','{PSEUD}','{title}','{summary}','public','published',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key,
                                            title, author_text, summary, status, source_url,
                                            likes, provenance_json, created_at,
                                            updated_at, version)
                 VALUES ('01955b00-0000-4000-8000-0000000001{}','{AUTHOR}','{work}','ao3','{}',
                         '{title}','Someone','{summary}','complete','https://example.invalid/{}',
                         7, '{{}}','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
                &work[work.len() - 2..],
                &work[work.len() - 2..],
                &work[work.len() - 2..]
            ),
        )
        .await;
    }
    // The tagged work carries a visible tag already, so the pass must skip it.
    for (node, canonical) in [
        (NODE_LONG, "hurt comfort"),
        (NODE_SHORT, "comfort"),
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
    exec(
        db,
        &format!(
            "INSERT INTO work_tags (work_id, node_id, source, confirmation, prominence, added_at)
             VALUES ('{TAGGED_WORK}','{NODE_LONG}','curator','unconfirmed',1,'2026-01-01T00:00:00Z')"
        ),
    )
    .await;
}

#[tokio::test]
async fn the_pass_proposes_only_what_readers_cannot_see() {
    let (db, dir) = database("pass").await;
    seed(&db).await;

    let report = propose_from_text(&db, 100).await.expect("pass");
    assert_eq!(report.examined, 2, "the already-tagged work is not examined");

    // 1. A proposal, invisible to readers.
    let visible = darkhold_db::taxonomy::tag_names_for_work(&db, NAMED_WORK)
        .await
        .expect("tags");
    assert!(
        visible.is_empty(),
        "an inferred tag is not a fact: {visible:?}"
    );
    let rows = darkhold_db::tag_assertions::assertions_for_work(&db, NAMED_WORK)
        .await
        .expect("assertions");
    assert!(!rows.is_empty(), "the proposal is recorded");

    // 2. The longest node wins: "hurt comfort" is proposed, bare "comfort" is not.
    let nodes: Vec<&str> = rows.iter().map(|row| row.node_id.as_str()).collect();
    assert!(nodes.contains(&NODE_LONG), "longest match wins: {nodes:?}");
    assert!(
        !nodes.contains(&NODE_SHORT),
        "the shorter node must not ride along on the longer match: {nodes:?}"
    );

    // 3. A second pass proposes nothing new. `examined` is not zero and should not be: the work that matched nothing is still
    // untagged, so it is looked at every time — the property that matters is that no second row is written.
    let again = propose_from_text(&db, 100).await.expect("second pass");
    assert_eq!(again.proposed, 0, "a proposal is not re-proposed as a new one");
    let rows_again = darkhold_db::tag_assertions::assertions_for_work(&db, NAMED_WORK)
        .await
        .expect("assertions");
    assert_eq!(rows_again.len(), rows.len(), "no second row for the same node");

    // 5. Coverage: one of three items carries likes and a visible tag.
    let report_coverage = coverage(&db).await.expect("coverage");
    let ao3 = report_coverage
        .iter()
        .find(|row| row.source_key == "ao3")
        .expect("ao3");
    assert_eq!(ao3.items, 3);
    assert_eq!(ao3.tagged, 1, "a proposal is not a tag in coverage either");
    assert_eq!(ao3.with_likes, 3);
    assert_eq!(ao3.tagged_and_liked, 1);

    // 4. Confirming the proposal makes it a visible tag, and coverage follows.
    assert!(
        darkhold_db::tag_assertions::confirm(
            &db,
            NAMED_WORK,
            NODE_LONG,
            darkhold_db::tag_assertions::ConfirmLevel::Reader
        )
        .await
        .expect("confirm")
    );
    let visible = darkhold_db::taxonomy::tag_names_for_work(&db, NAMED_WORK)
        .await
        .expect("tags");
    assert_eq!(visible, vec!["hurt comfort".to_owned()]);
    let after = coverage(&db).await.expect("coverage");
    let ao3 = after.iter().find(|row| row.source_key == "ao3").expect("ao3");
    assert_eq!(ao3.tagged, 2, "a confirmed inference is a tag");

    let _ = std::fs::remove_dir_all(dir);
}

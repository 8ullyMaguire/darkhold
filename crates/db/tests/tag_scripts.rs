//! M69-20 — a tag script applies a kept set of assertions while browsing.
//!
//! The property that matters is attribution: a script is a mode, and everything it writes must look exactly like a tag a person added
//! by hand — `curator` on somebody else's work, `author` on your own — because a script is a shortcut for typing, not a separate
//! kind of claim.
//!
//! Asserts:
//! 1. a script keeps its nodes in order and lists them back;
//! 2. applying writes every node as a human assertion and reports `applied`;
//! 3. applying twice is idempotent and reports `already_present`;
//! 4. the claimant follows ownership — `curator` on another's work, `author` on your own;
//! 5. applying somebody else's script is `None`, not a write;
//! 6. the tags a script wrote are visible on the reader-facing read.

use darkhold_db::tag_scripts::{add_node, apply_to_work, create_script, scripts_for};
use darkhold_db::{Backend, Database, DatabaseConfig};
use std::path::PathBuf;

const ME: &str = "01955700-0000-4000-8000-000000000001";
const OTHER: &str = "01955700-0000-4000-8000-000000000002";
const MY_PSEUD: &str = "01955700-0000-4000-8000-000000000003";
const THEIR_PSEUD: &str = "01955700-0000-4000-8000-000000000004";
const MY_WORK: &str = "01955700-0000-4000-8000-000000000005";
const THEIR_WORK: &str = "01955700-0000-4000-8000-000000000006";
const TAG_A: &str = "01955700-0000-4000-8000-000000000007";
const TAG_B: &str = "01955700-0000-4000-8000-000000000008";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-scripts-{tag}-{}-{:?}",
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
                "lh_test_scripts_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("an empty PostgreSQL database")
        }
        _ => {
            let config = DatabaseConfig::new(format!(
                "sqlite://{}/scripts.sqlite?mode=rwc",
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
    for (account, email) in [(ME, "me@example.invalid"), (OTHER, "other@example.invalid")] {
        exec(
            db,
            &format!(
                "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version)
                 VALUES ('{account}','active','{email}','adult',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    for (pseud, account, handle) in [(MY_PSEUD, ME, "mine"), (THEIR_PSEUD, OTHER, "theirs")] {
        exec(
            db,
            &format!(
                "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at,
                                     updated_at, version)
                 VALUES ('{pseud}','{account}','{handle}','{handle}','listed',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    for (work, owner, title) in [
        (MY_WORK, MY_PSEUD, "Mine"),
        (THEIR_WORK, THEIR_PSEUD, "Theirs"),
    ] {
        exec(
            db,
            &format!(
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, completion,
                                    created_at, updated_at, version)
                 VALUES ('{work}','{owner}','{title}','published','public','complete',
                         '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)"
            ),
        )
        .await;
    }
    for (node, canonical) in [(TAG_A, "slow burn"), (TAG_B, "hurt comfort")] {
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
async fn a_script_is_a_mode_that_writes_attributed_tags() {
    let (db, dir) = database("mode").await;
    seed(&db).await;

    // 1. Nodes keep their order and come back in it.
    let script = create_script(&db, ME, "Comfort reads")
        .await
        .expect("create");
    assert!(add_node(&db, ME, &script.id, TAG_B).await.expect("add b"));
    assert!(add_node(&db, ME, &script.id, TAG_A).await.expect("add a"));
    assert!(
        !add_node(&db, ME, &script.id, TAG_A)
            .await
            .expect("add a again"),
        "adding what is already there is a no-op"
    );
    let listed = scripts_for(&db, ME).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].node_ids, vec![TAG_B.to_owned(), TAG_A.to_owned()]);

    // 5. Somebody else's script is not applicable, and nothing is written.
    assert!(
        apply_to_work(&db, OTHER, &script.id, THEIR_WORK)
            .await
            .expect("apply")
            .is_none(),
        "a script belongs to the reader who made it"
    );

    // 4 + 6. Applied to their work: curator, visible.
    let report = apply_to_work(&db, ME, &script.id, THEIR_WORK)
        .await
        .expect("apply")
        .expect("their script");
    assert_eq!(report.applied, 2);
    let tags = darkhold_db::taxonomy::tag_names_for_work(&db, THEIR_WORK)
        .await
        .expect("tags");
    assert_eq!(tags.len(), 2, "both tags landed: {tags:?}");
    let rows = darkhold_db::tag_assertions::assertions_for_work(&db, THEIR_WORK)
        .await
        .expect("assertions");
    assert!(
        rows.iter().all(|row| row.source == "curator"),
        "somebody else's work takes a curator claim: {rows:?}"
    );

    // 3. Applying twice reports what was already there.
    let again = apply_to_work(&db, ME, &script.id, THEIR_WORK)
        .await
        .expect("apply")
        .expect("script");
    assert_eq!(again.applied, 0);
    assert_eq!(again.already_present, 2);

    // 4. My own work takes an author claim.
    apply_to_work(&db, ME, &script.id, MY_WORK)
        .await
        .expect("apply")
        .expect("script");
    let mine = darkhold_db::tag_assertions::assertions_for_work(&db, MY_WORK)
        .await
        .expect("assertions");
    assert!(
        mine.iter().all(|row| row.source == "author"),
        "your own work takes an author claim: {mine:?}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

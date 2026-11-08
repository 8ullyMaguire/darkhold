//! The maintenance pass actually maintains (M70-13).
//!
//! Three derived tables had an idempotent writer and no caller: the entity closure, the floor counts, and the experiment
//! snapshots. A schedule is what they were missing, and the worker's sweep is the schedule this instance already runs — so
//! the test drives the sweep and then looks at the tables, because "the function is called from somewhere" is a claim about
//! source text and this is a claim about a database.
//!
//! The snapshots step is asserted by its *decision*, not its arithmetic: with no impressions recorded for a day, the pass
//! runs `day_snapshots` and finds nothing to write, and the assertion is that the pass still completes — a maintenance step
//! that fails on an empty instance would have made every other step in the pass conditional on traffic.

use std::path::PathBuf;

use darkhold_app::config::Config;
use darkhold_app::state::AppState;
use darkhold_app::worker::{Worker, WorkerOptions};
use test_support::TestDb;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-m7013-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

async fn exec(tdb: &TestDb, sql: &str) {
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .execute(tdb.db().sqlite_pool().expect("sqlite pool"))
                .await
                .expect("exec sqlite");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .execute(tdb.db().postgres_pool().expect("postgres pool"))
                .await
                .expect("exec postgres");
        }
    }
}

async fn count(tdb: &TestDb, sql: &str) -> i64 {
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(sql)
            .fetch_one(tdb.db().sqlite_pool().expect("sqlite pool"))
            .await
            .expect("count sqlite"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(sql)
            .fetch_one(tdb.db().postgres_pool().expect("postgres pool"))
            .await
            .expect("count postgres"),
    }
}

fn state(dir: &std::path::Path, tdb: &TestDb) -> AppState {
    let mut config = Config::development_defaults();
    config.storage.root = dir.to_path_buf();
    AppState::new(config, tdb.db().clone())
}

#[tokio::test]
async fn the_sweep_rebuilds_the_closure_and_recounts_the_floors() {
    let dir = scratch_dir("closure");
    let tdb = TestDb::connect_with_dir("m7013-closure", &dir).await;

    // A small tree: one browsable parent and one browsable child. Both start with a count of 10 that nothing wrote, so a
    // recount that runs is visible as a change rather than as a number that happened to be right already.
    for (id, parent) in [("root-ent", None), ("child-ent", Some("root-ent"))] {
        let sql = match parent {
            Some(parent) => format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count, node_state, node_count, parent_id)
                 VALUES ('{id}', 'tag', '{id}', '{id}', '2026-01-01T00:00:00Z', 'curated', 0, 'browsable', 10, '{parent}')"
            ),
            None => format!(
                "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count, node_state, node_count)
                 VALUES ('{id}', 'tag', '{id}', '{id}', '2026-01-01T00:00:00Z', 'curated', 0, 'browsable', 10)"
            ),
        };
        exec(&tdb, &sql).await;
    }
    assert_eq!(
        count(&tdb, "SELECT COUNT(*) FROM entity_closure").await,
        0,
        "nothing has built the closure yet"
    );

    let state = state(&dir, &tdb);
    Worker::new(WorkerOptions::named("m7013-maintenance"))
        .maintenance_pass(&state)
        .await
        .expect("maintenance pass");

    assert!(
        count(&tdb, "SELECT COUNT(*) FROM entity_closure").await > 0,
        "the sweep builds the closure: a parent with a browsable child has a closure row"
    );
    assert_eq!(
        count(
            &tdb,
            "SELECT node_count FROM canonical_entities WHERE id = 'child-ent'"
        )
        .await,
        0,
        "the sweep recounts: a browsable entity with no works is counted at zero, not left at the number it was seeded with"
    );
}

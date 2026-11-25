//! FS-01: crossover include/exclude is derived from fandom taxonomy, not a stored flag.
//!
//! Runs through `test_support::TestDb` so SQLite and PostgreSQL execute the same
//! parser, renderer and search handler. PostgreSQL is selected only by
//! `DARKHOLD_TEST_PG_URL`; the fixture invents no session and bypasses no route.

use darkhold_db::search::{search_works_ast_scoped, SearchFilters};
use std::path::PathBuf;

const A: &str = "0195f001-0000-4000-8000-000000000001";
const U: &str = "0195f001-0000-4000-8000-000000000002";
const F1: &str = "0195f001-0000-4000-8000-000000000011";
const F2: &str = "0195f001-0000-4000-8000-000000000012";
const XC: &str = "0195f001-0000-4000-8000-000000000021";
const SINGLE: &str = "0195f001-0000-4000-8000-000000000022";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-fs01-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn exec(db: &darkhold_db::Database, sql: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .unwrap_or_else(|e| panic!("fixture SQLite: {sql}: {e}"));
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .unwrap_or_else(|e| panic!("fixture PG: {sql}: {e}"));
        }
    }
}

async fn search(db: &darkhold_db::Database, query: &str) -> Vec<String> {
    let rows = search_works_ast_scoped(db, query, None, 20, &[], &SearchFilters::default())
        .await
        .unwrap_or_else(|e| panic!("search {query:?}: {e:#}"));
    rows.into_iter().map(|r| r.work_id).collect()
}

#[tokio::test]
async fn crossover_exclude_drops_it_and_only_keeps_it_on_both_engines() {
    let tdb = test_support::TestDb::connect_with_dir("fs01", &scratch()).await;
    let db = tdb.db();
    exec(db, &format!("INSERT INTO accounts (id,status,email,age_state,created_at,updated_at,version) VALUES ('{A}','active','fs01@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(db, &format!("INSERT INTO pseuds (id,account_id,handle,display_name,discoverability,created_at,updated_at,version) VALUES ('{U}','{A}','fs01','FS01','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    for node in [F1, F2] {
        exec(db, &format!("INSERT INTO canonical_entities (id,kind,canonical,norm,created_at) VALUES ('{node}','fandom','Fandom {node}','fandom {node}','2026-01-01T00:00:00Z')")).await;
    }
    for (work, title) in [(XC, "Crossover"), (SINGLE, "Single")] {
        exec(db, &format!("INSERT INTO works (id,owner_pseud_id,title,visibility,lifecycle,completion,created_at,updated_at) VALUES ('{work}','{U}','{title}','public','published','complete','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")).await;
    }
    exec(db, &format!("INSERT INTO work_tags (work_id,node_id,prominence,added_at) VALUES ('{XC}','{F1}',1,'2026-01-01T00:00:00Z')")).await;
    exec(db, &format!("INSERT INTO work_tags (work_id,node_id,prominence,added_at) VALUES ('{XC}','{F2}',1,'2026-01-01T00:00:00Z')")).await;
    exec(db, &format!("INSERT INTO work_tags (work_id,node_id,prominence,added_at) VALUES ('{SINGLE}','{F1}',1,'2026-01-01T00:00:00Z')")).await;

    let excluded = search(db, "crossover:exclude").await;
    assert_eq!(excluded, vec![SINGLE.to_owned()], "exclude must drop the crossover");
    let only = search(db, "crossover:only").await;
    assert_eq!(only, vec![XC.to_owned()], "only must keep exactly the crossover");
    assert!(search(db, "NOT crossover:only").await.contains(&SINGLE.to_owned()));
}

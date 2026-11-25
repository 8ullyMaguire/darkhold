//! M45-75: source clocks, source keys, viewer-rating exclusion and real sort.
//!
//! Runs through `TestDb` so SQLite and PostgreSQL execute the same handler and
//! the same SQL. PostgreSQL is selected only by `DARKHOLD_TEST_PG_URL`; no case
//! invents a session or bypasses a route.

use darkhold_db::search::{search_works_ast_scoped, SearchFilters};
use std::path::PathBuf;

const A: &str = "01957700-0000-4000-8000-000000000001";
const U: &str = "01957700-0000-4000-8000-000000000003";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-m45-75-{}", std::process::id()));
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

async fn search(db: &darkhold_db::Database, q: &str, viewer: Option<&str>, sort: &str) -> Vec<String> {
    let mut scope = SearchFilters::default();
    scope.sort = Some(sort.to_owned());
    let rows = search_works_ast_scoped(db, q, viewer, 20, &[], &scope)
        .await
        .unwrap_or_else(|e| panic!("search {q:?} sort={sort}: {e:#}"));
    rows.into_iter().map(|r| r.work_id).collect()
}

#[tokio::test]
async fn source_clock_viewer_and_sort_are_honoured_on_both_engines() {
    let tdb = test_support::TestDb::connect_with_dir("m45-75", &scratch()).await;
    let db = tdb.db();
    exec(db, &format!("INSERT INTO accounts (id,status,email,age_state,created_at,updated_at,version) VALUES ('{A}','active','m4575@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(db, &format!("INSERT INTO pseuds (id,account_id,handle,display_name,discoverability,created_at,updated_at,version) VALUES ('{U}','{A}','m4575','M4575','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;

    // Two works in the two named sources; one old source clock, one fresh.
    for (work, key, at, likes) in [
        ("01", "qq", "2020-01-01T00:00:00Z", 5),
        ("02", "sv", "2026-11-20T00:00:00Z", 90),
    ] {
        exec(db, &format!("INSERT INTO works (id,owner_pseud_id,title,visibility,lifecycle,completion,created_at,updated_at) VALUES ('01957700-0000-4000-8000-0000000000{work}','{U}','Work {work}','public','published','in_progress','2026-11-20T00:00:00Z','2026-11-20T00:00:00Z')")).await;
        exec(db, &format!("INSERT INTO library_items (id,account_id,work_id,source_key,source_work_key,title,status,source_updated_at,source_url,likes,created_at,updated_at) VALUES ('01957700-0000-4000-8000-0000000010{work}','{A}','01957700-0000-4000-8000-0000000000{work}','{key}','w{work}','Work {work}','ongoing','{at}','https://example.invalid/{work}',{likes},'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")).await;
    }
    // The clocks deliberately disagree with the likes: work 01 is newer but
    // work 02 is more popular, so the two orderings must name different hits.
    exec(db, "UPDATE works SET updated_at='2026-11-21T00:00:00Z' WHERE id='01957700-0000-4000-8000-000000000001'").await;
    exec(db, "UPDATE works SET updated_at='2026-11-19T00:00:00Z' WHERE id='01957700-0000-4000-8000-000000000002'").await;

    // The viewer rated work 01; work 02 stays unrated.
    exec(db, &format!("INSERT INTO rating (id,account_id,pseud_id,work_id,stars,created_at,updated_at) VALUES ('01957700-0000-4000-8000-000000000021','{A}','{U}','01957700-0000-4000-8000-000000000001',5,'2026-11-20T00:00:00Z','2026-11-20T00:00:00Z')")).await;

    let both = search(db, "source:qq OR source:sv", None, "").await;
    assert_eq!(both.len(), 2, "both source keys must match: {both:?}");
    assert!(search(db, "source:ffnet", None, "").await.is_empty());

    let stale = search(db, "source_updated:<=-4m", None, "").await;
    assert_eq!(stale, vec!["01957700-0000-4000-8000-000000000001".to_owned()]);
    assert!(search(db, "updated:<=-4m", None, "").await.is_empty(), "our clock must not move");

    let unrated = search(db, "NOT me:rated", Some(&A), "").await;
    assert_eq!(unrated, vec!["01957700-0000-4000-8000-000000000002".to_owned()]);
    assert!(search(db, "me:rated", Some(&A), "").await.len() == 1);
    let anon = search_works_ast_scoped(db, "NOT me:rated", None, 20, &[], &SearchFilters::default()).await;
    assert!(anon.is_err(), "an anonymous viewer must get a named 401, not a silent page");
    assert!(format!("{:?}", anon.as_ref().err()).contains("me:rated"), "{anon:?}");

    let popular = search(db, "", None, "popularity").await;
    assert_eq!(popular.first().map(String::as_str), Some("01957700-0000-4000-8000-000000000002"));
    let newest = search(db, "", None, "newest").await;
    assert_eq!(
        newest.first().map(String::as_str),
        Some("01957700-0000-4000-8000-000000000001")
    );
    assert_ne!(popular.first(), newest.first(), "the two orders must differ");
    let bad = search_works_ast_scoped(db, "", None, 20, &[], &SearchFilters { sort: Some("nope".to_owned()), ..Default::default() }).await;
    assert!(bad.is_err(), "an unknown sort must be refused by name");
    assert!(format!("{:?}", bad.err()).contains("nope"));
}

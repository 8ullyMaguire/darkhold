//! Cross-source fanfic filter proof: tags + word count + language + stale source clock.
//!
//! This is a dual-engine test through the same AST search the API uses. The
//! live catalogue has uneven source-date coverage, so a live-only check cannot
//! prove the combination; these fixtures give three sources complete metadata.

use darkhold_db::search::{search_works_ast_scoped, SearchFilters};
use std::path::PathBuf;

const A: &str = "0195aa00-0000-4000-8000-000000000001";
const U: &str = "0195aa00-0000-4000-8000-000000000002";
const OLD_AO3: &str = "0195aa00-0000-4000-8000-000000000011";
const OLD_PAWCHIVE: &str = "0195aa00-0000-4000-8000-000000000012";
const FRESH_SB: &str = "0195aa00-0000-4000-8000-000000000013";
const CYOA: &str = "0195aa00-0000-4000-8000-000000000021";
const APOCALYPSE: &str = "0195aa00-0000-4000-8000-000000000022";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-cross-source-tags-{}", std::process::id()));
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

#[tokio::test]
async fn tags_words_language_and_source_staleness_search_every_source() {
    let tdb = test_support::TestDb::connect_with_dir("cross-source-tags", &scratch()).await;
    let db = tdb.db();
    exec(db, &format!("INSERT INTO accounts (id,status,email,age_state,created_at,updated_at,version) VALUES ('{A}','active','cross-source@test.dev','adult','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z',1)")).await;
    exec(db, &format!("INSERT INTO pseuds (id,account_id,handle,display_name,discoverability,created_at,updated_at,version) VALUES ('{U}','{A}','crosssource','Cross Source','listed','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z',1)")).await;

    for (kind, id, label) in [
        ("tag", CYOA, "cyoa"),
        ("tag", APOCALYPSE, "apocalypse"),
    ] {
        exec(db, &format!("INSERT INTO canonical_entities (id,kind,canonical,norm,created_at,node_state) VALUES ('{id}','{kind}','{label}','{label}','2020-01-01T00:00:00Z','browsable')")).await;
    }

    for (work, source, tag, clock) in [
        (OLD_AO3, "ao3", CYOA, "2000-01-01T00:00:00Z"),
        (OLD_PAWCHIVE, "pawchive", APOCALYPSE, "2000-01-01T00:00:00Z"),
        (FRESH_SB, "sb", CYOA, "2099-01-01T00:00:00Z"),
    ] {
        exec(db, &format!("INSERT INTO works (id,owner_pseud_id,title,summary,visibility,lifecycle,completion,language,created_at,updated_at,version) VALUES ('{work}','{U}','Work {work}','summary','public','published','complete','en','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z',1)")).await;
        exec(db, &format!("INSERT INTO library_items (id,account_id,work_id,source_key,source_work_key,title,author_text,source_url,status,word_count,source_updated_at,created_at,updated_at,version) VALUES ('{work}','{A}','{work}','{source}','w{work}','Work {work}','Author','https://example.invalid/{work}','complete',50000,'{clock}','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z',1)")).await;
        exec(db, &format!("INSERT INTO work_tags (work_id,node_id,role,source,added_at) VALUES ('{work}','{tag}','primary','imported','2020-01-01T00:00:00Z')")).await;
        let revision = format!("{}3{}", &work[..work.len() - 2], &work[work.len() - 1..]);
        exec(db, &format!("INSERT INTO units (id,work_id,order_key,title,created_at,updated_at,version) VALUES ('{work}','{work}',1,'Chapter 1','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z',1)")).await;
        exec(db, &format!("INSERT INTO revisions (id,unit_id,revision_number,document_json,sanitized_html,plain_text,word_count,created_by_pseud_id,created_at) VALUES ('{revision}','{work}',1,'{{\"type\":\"doc\"}}','<p>x</p>','x',50000,'{U}','2020-01-01T00:00:00Z')")).await;
        exec(db, &format!("UPDATE units SET current_revision_id='{revision}' WHERE id='{work}'")).await;
    }

    let hits = search_works_ast_scoped(
        db,
        "(tag:cyoa OR tag:apocalypse) words:>30000 language:en source_updated:<=-3m",
        None,
        20,
        &[],
        &SearchFilters::default(),
    )
    .await
    .expect("cross-source query");
    let ids: Vec<&str> = hits.iter().map(|r| r.work_id.as_str()).collect();
    assert_eq!(ids.len(), 2, "two stale works must match across sources: {ids:?}");
    assert!(ids.contains(&OLD_AO3), "ao3 hit missing: {ids:?}");
    assert!(ids.contains(&OLD_PAWCHIVE), "pawchive hit missing: {ids:?}");
    assert!(!ids.contains(&FRESH_SB), "a freshly updated work must not match: {ids:?}");

    let words = search_works_ast_scoped(
        db,
        "(tag:cyoa OR tag:apocalypse) words:>60000 language:en source_updated:<=-3m",
        None,
        20,
        &[],
        &SearchFilters::default(),
    )
    .await
    .expect("word threshold query");
    assert!(words.is_empty(), "the word threshold must exclude 50k works");

    let _ = std::fs::remove_dir_all(scratch());
}

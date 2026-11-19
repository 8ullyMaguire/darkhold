//! M71-13 — the hosted/referenced split uses the same provenance rule on both engines.

use darkhold_db::taxonomy::dimension_coverage;
use darkhold_db::{Backend, Database};
use test_support::TestDb;

const ACCOUNT: &str = "01955000-0000-4000-8000-0000000e0001";
const PSEUD: &str = "01955000-0000-4000-8000-0000000e0002";
const TAG: &str = "01955000-0000-4000-8000-0000000e0010";
const HOSTED: &str = "01956000-0000-4000-8000-0000000f0001";
const REFERENCED: &str = "01956000-0000-4000-8000-0000000f0002";

async fn exec(db: &Database, sql: &str) {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("fixture");
        }
        Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("fixture");
        }
    }
}

#[tokio::test]
async fn coverage_splits_provenance_on_both_engines() {
    let dir = std::env::temp_dir().join(format!("darkhold-coverage-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let tdb = TestDb::connect_with_dir("coverage-split", &dir).await;
    let db = tdb.db();
    exec(db, &format!("INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, permission_statement, version) VALUES ('{ACCOUNT}','active','cov@example.invalid','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','minimal',1)")).await;
    exec(db, &format!("INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at, version) VALUES ('{PSEUD}','{ACCOUNT}','writer','Writer','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
    exec(db, &format!("INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count) VALUES ('{TAG}','tag','slow burn','slow burn','2026-01-01T00:00:00Z','confirmed',0)")).await;
    for (id, title) in [(HOSTED, "Hosted"), (REFERENCED, "Referenced")] {
        exec(db, &format!("INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at, version) VALUES ('{id}','{PSEUD}','{title}','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)")).await;
        exec(db, &format!("INSERT INTO work_tags (work_id, node_id, prominence, added_at) VALUES ('{id}','{TAG}',1,'2026-01-01T00:00:00Z')")).await;
    }
    exec(db, "INSERT INTO units (id, work_id, order_key, title, current_revision_id, created_at, updated_at, version) VALUES ('01956000-0000-4000-8000-0000000a0011','01956000-0000-4000-8000-0000000f0001',10,'One',NULL,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)").await;
    exec(db, "INSERT INTO revisions (id, unit_id, revision_number, document_json, sanitized_html, plain_text, word_count, created_by_pseud_id, created_at) VALUES ('01956000-0000-4000-8000-0000000a0001','01956000-0000-4000-8000-0000000a0011',1,'{}','<p>hi</p>','hi',1,'01955000-0000-4000-8000-0000000e0002','2026-01-01T00:00:00Z')").await;
    exec(db, "UPDATE units SET current_revision_id = '01956000-0000-4000-8000-0000000a0001' WHERE id = '01956000-0000-4000-8000-0000000a0011'").await;
    exec(db, "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, source_url, created_at, updated_at, version) VALUES ('01956000-0000-4000-8000-0000000a0021','01955000-0000-4000-8000-0000000e0001','01956000-0000-4000-8000-0000000f0002','source','story-2','Referenced','https://example.invalid/2','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)").await;

    let coverage = dimension_coverage(db, "tag", "slow burn")
        .await
        .expect("coverage");
    assert_eq!(coverage.works, 2);
    assert_eq!(coverage.hosted, 1, "{coverage:?}");
    assert_eq!(coverage.referenced, 1, "{coverage:?}");
    assert_eq!(coverage.known, 0, "{coverage:?}");
    assert!(
        coverage.split_sentence.contains("1 hosted")
            && coverage.split_sentence.contains("1 referenced")
    );
}

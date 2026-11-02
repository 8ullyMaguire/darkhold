//! §11.5 rung 2 — a work the reader has **not opened** leads one they have.
//!
//! `same_author_works` ordered by `published_at DESC` alone, so a reader who had finished an author's
//! newest work was offered it again at the top of "more by this author", ahead of the one they had
//! never opened. §11.5 rung 2 says *unread-first, then affinity, then recency*; within a single
//! author the affinity ratio is constant, so unread-first is the half that can be implemented, and
//! recency stays the order underneath it.
//!
//! Three tests, and the third is a definition: `want-to-read` is intent, not evidence, so it must not
//! demote a work. Runs on SQLite, and on PostgreSQL when `DARKHOLD_TEST_PG_URL` names an admin
//! database.

use darkhold_db::Database;
use std::time::Duration;

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    darkhold_db::DatabaseConfig {
        url,
        max_connections: 5,
        acquire_timeout: Duration::from_secs(10),
        slow_query_warn: Duration::ZERO,
    }
}

async fn connect(tag: &str) -> Database {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "darkhold-author-{}-{}-{}",
        tag,
        uuid::Uuid::new_v4(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let url = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin) => {
            let name = format!(
                "lh_author_{}_{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            );
            let admin_db = Database::connect(&make_config(admin.clone()))
                .await
                .expect("connect to the admin database");
            sqlx::query(&format!("CREATE DATABASE {name}"))
                .execute(admin_db.postgres_pool().expect("postgres pool"))
                .await
                .expect("create a scratch database");
            admin_db.close().await;
            let (prefix, _) = admin
                .rsplit_once('/')
                .expect("the admin URL ends in a database");
            format!("{prefix}/{name}")
        }
        Err(_) => format!("sqlite://{}/darkhold.sqlite?mode=rwc", dir.display()),
    };
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

/// One statement, both dialects. `?N#u` names a uuid; SQLite drops the cast.
async fn insert(db: &Database, sqlite: &str, args: &[&str]) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            let sqlite = &sqlite.replace("#u", "");
            let mut q = sqlx::query(sqlite);
            for a in args {
                q = q.bind(*a);
            }
            q.execute(db.sqlite_pool().expect("sqlite pool"))
                .await
                .expect("insert");
        }
        darkhold_db::Backend::Postgres => {
            let postgres = (1..=6).fold(sqlite.to_string(), |acc, n| {
                acc.replace(&format!("?{n}#u"), &format!("${n}::uuid"))
                    .replace(&format!("?{n}"), &format!("${n}"))
            });
            let mut q = sqlx::query(&postgres);
            for a in args {
                match uuid::Uuid::parse_str(a) {
                    Ok(u) => q = q.bind(u),
                    Err(_) => q = q.bind(*a),
                };
            }
            q.execute(db.postgres_pool().expect("postgres pool"))
                .await
                .expect("insert");
        }
    }
}

struct Fixture {
    db: Database,
    reader: String,
    /// The work whose page the rung is shown on — excluded from its own rung.
    seed: String,
    /// Published before `newer`, so recency alone puts `newer` first.
    older: String,
    newer: String,
}

impl Fixture {
    async fn build(tag: &str) -> Self {
        let db = connect(tag).await;
        let run = uuid::Uuid::new_v4().to_string();
        let author_account = uuid::Uuid::new_v4().to_string();
        let author_pseud = uuid::Uuid::new_v4().to_string();
        let reader = uuid::Uuid::new_v4().to_string();
        let reader_pseud = uuid::Uuid::new_v4().to_string();
        for (account, pseud, label) in [
            (&author_account, &author_pseud, "a"),
            (&reader, &reader_pseud, "r"),
        ] {
            insert(
                &db,
                "INSERT INTO accounts (id, email, created_at, updated_at) \
                 VALUES (?1#u, ?2, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                &[account, &format!("{label}-{tag}-{run}@example.com")],
            )
            .await;
            insert(
                &db,
                "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) \
                 VALUES (?1#u, ?2#u, ?3, ?3, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                &[pseud, account, &format!("{label}{tag}-{run}")],
            )
            .await;
        }

        // THREE works, not two: `same_author_works` excludes the work it is asked about, so a
        // two-work fixture would have exactly one candidate and any assertion about order would be
        // vacuous — it would pass with the CASE deleted.
        let seed = uuid::Uuid::new_v4().to_string();
        let older = uuid::Uuid::new_v4().to_string();
        let newer = uuid::Uuid::new_v4().to_string();
        for (id, title, at) in [
            (seed.clone(), "Seed", "2026-01-01T00:00:00Z"),
            (older.clone(), "Older", "2026-02-01T00:00:00Z"),
            (newer.clone(), "Newer", "2026-06-01T00:00:00Z"),
        ] {
            insert(
                &db,
                "INSERT INTO works (id, owner_pseud_id, title, lifecycle, visibility, created_at, \
                 updated_at, published_at, generated_content_posture) \
                 VALUES (?1#u, ?2#u, ?3, 'published', 'public', ?4, ?4, ?4, 'forbid')",
                &[&id, &author_pseud, title, at],
            )
            .await;
        }
        Self {
            db,
            reader,
            seed,
            older,
            newer,
        }
    }

    /// Mark `work` as `status` for the reader.
    async fn mark(&self, work: &str, status: &str) {
        insert(
            &self.db,
            "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, \
             updated_at) VALUES (?1#u, ?2#u, 'work', ?3#u, ?4, '2026-07-01T00:00:00Z')",
            &[
                &uuid::Uuid::new_v4().to_string(),
                &self.reader,
                work,
                status,
            ],
        )
        .await;
    }

    /// Mark `work` as `status` through the reader's **library item**, the other subject the app
    /// writes: `PUT /api/v1/library/items/{id}/status` keys on the item, `PUT /api/v1/works/{id}/status`
    /// keys on the work, and both land in `reading_status`. A rung that read only one of the two
    /// would demote half the readers who finished something.
    async fn mark_via_library_item(&self, work: &str, status: &str) {
        let item = uuid::Uuid::new_v4().to_string();
        insert(
            &self.db,
            "INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, \
             title, author_text, summary, status, source_url, provenance_json, created_at, \
             updated_at) \
             VALUES (?1#u, ?2#u, ?3#u, 'royalroad', ?4, 'Item', 'An Author', '', 'complete', \
             'https://example.test/item', '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            &[&item, &self.reader, work, &format!("s-{work}")],
        )
        .await;
        insert(
            &self.db,
            "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, \
             updated_at) VALUES (?1#u, ?2#u, 'library_item', ?3#u, ?4, '2026-07-01T00:00:00Z')",
            &[
                &uuid::Uuid::new_v4().to_string(),
                &self.reader,
                &item,
                status,
            ],
        )
        .await;
    }

    async fn order(&self, viewer: Option<&str>) -> Vec<String> {
        darkhold_db::series_recs::same_author_works(&self.db, &self.seed, viewer)
            .await
            .expect("same author")
            .iter()
            .map(|r| r.work_id.clone())
            .collect()
    }
}

/// The assertion the old code fails: recency-only puts the finished work first.
///
/// The reader finished the **newer** work, so recency and "unread first" disagree — recency offers
/// the one they just finished at the top, which is the defect.
#[tokio::test]
async fn an_unread_work_comes_before_a_read_one_even_when_older() {
    let fixture = Fixture::build("unread").await;
    fixture.mark(&fixture.newer, "finished").await;
    let served = fixture.order(Some(&fixture.reader)).await;
    assert_eq!(
        served,
        vec![fixture.older.clone(), fixture.newer.clone()],
        "the unread older work leads the finished newer one: {served:?}"
    );
}

/// An anonymous visitor has no reading history, so nothing is demoted and `published_at DESC` stands.
///
/// Same fixture and the same `finished` row as the test above: the ONLY difference is the viewer, so
/// the two tests together prove the viewer is what changes the answer. Without this arm, the CASE
/// could be `1` for everyone and the first test would still pass.
#[tokio::test]
async fn an_anonymous_viewer_keeps_published_at_order() {
    let fixture = Fixture::build("anon").await;
    fixture.mark(&fixture.newer, "finished").await;
    let served = fixture.order(None).await;
    assert_eq!(
        served,
        vec![fixture.newer.clone(), fixture.older.clone()],
        "a visitor gets `published_at DESC`, newest first: {served:?}"
    );
}

/// `want-to-read` is intent, not evidence: it must not demote a work the way `finished` does.
#[tokio::test]
async fn want_to_read_is_not_read() {
    let fixture = Fixture::build("intent").await;
    fixture.mark(&fixture.newer, "want-to-read").await;
    let served = fixture.order(Some(&fixture.reader)).await;
    assert_eq!(
        served,
        vec![fixture.newer.clone(), fixture.older.clone()],
        "a want-to-read row reorders nothing, so recency still leads: {served:?}"
    );
}
/// A status set through the **library item** counts exactly like one set on the work.
///
/// This is the assertion that stops the rung from reading half the table: the two routes write the
/// same rows under different `subject_type`s, and the failure mode is silent — the list simply looks
/// slightly wrong for readers who finish works from their library rather than the work page.
#[tokio::test]
async fn a_library_item_status_counts_as_read() {
    let fixture = Fixture::build("item").await;
    fixture
        .mark_via_library_item(&fixture.newer, "finished")
        .await;
    let served = fixture.order(Some(&fixture.reader)).await;
    assert_eq!(
        served,
        vec![fixture.older.clone(), fixture.newer.clone()],
        "a finished library item demotes its work just as the work's own status does: {served:?}"
    );
}

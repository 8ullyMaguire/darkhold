//! M63-34 — a catalogue `q` reads the summary as well as title and author, and can be narrowed to one field.

use darkhold_db::catalogue::{self, CatalogueQuery, SearchScope};
use darkhold_db::Database;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-q-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

async fn database(tag: &str) -> (Database, std::path::PathBuf) {
    let dir = scratch_dir(tag);
    let db = match std::env::var("DARKHOLD_TEST_PG_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let name = format!(
                "lh_test_q_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            test_support::empty_postgres(&admin_url, &name)
                .await
                .expect("empty PostgreSQL database")
        }
        _ => {
            let config = darkhold_db::DatabaseConfig::new(format!(
                "sqlite://{}/darkhold.sqlite?mode=rwc",
                dir.display()
            ));
            Database::connect(&config).await.expect("connect")
        }
    };
    db.migrate().await.expect("migrate");
    // `library_items.account_id` is a foreign key on both engines, so the account has to exist before a row can.
    let account = "INSERT INTO accounts (id, email, created_at, updated_at) \
                   VALUES ('11111111-1111-4111-8111-111111111111', 'q-test@example.invalid', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')";
    let seeded = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query(account)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .map(|_| ()),
        darkhold_db::Backend::Postgres => sqlx::query(account)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .map(|_| ()),
    };
    seeded.expect("account");
    (db, dir)
}

async fn seed(db: &Database, id: &str, title: &str, author: &str, summary: &str) {
    let statement = format!(
        "INSERT INTO library_items \
         (id, account_id, source_key, source_work_key, title, author_text, summary, word_count, status, \
          source_url, source_updated_at, provenance_json, created_at, updated_at, likes, views) \
         VALUES ('{id}', '11111111-1111-4111-8111-111111111111', 'sb', 'k-{id}', '{title}', '{author}', '{summary}', 90000, 'ongoing', \
                 'https://example.invalid/{id}', '2026-10-01T10:00:00Z', '{{}}', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 10, 100)"
    );
    let result = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query(&statement)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .map(|_| ()),
        darkhold_db::Backend::Postgres => sqlx::query(&statement)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .map(|_| ()),
    };
    result.unwrap_or_else(|error| panic!("{error}"));
}

fn query(q: &str) -> CatalogueQuery {
    CatalogueQuery {
        q: Some(q.to_owned()),
        ..CatalogueQuery::default()
    }
}

#[tokio::test]
async fn a_term_that_only_appears_in_the_summary_is_found() {
    let (db, dir) = database("summary").await;
    seed(
        &db,
        "22222222-2222-4222-8222-222222222222",
        "Rooftop",
        "Orrin",
        "A seal breaks and the rain never stops",
    )
    .await;
    seed(
        &db,
        "33333333-3333-4333-8333-333333333333",
        "The Long Road",
        "Kestrel",
        "Nothing relevant here",
    )
    .await;

    let hits = catalogue::search(&db, &query("rain"))
        .await
        .expect("search");
    assert_eq!(hits.len(), 1, "the summary is searched");
    assert_eq!(hits[0].id, "22222222-2222-4222-8222-222222222222");

    // Title and author still work, and the summary did not replace them.
    assert_eq!(
        catalogue::search(&db, &query("rooftop"))
            .await
            .expect("search")
            .len(),
        1
    );
    assert_eq!(
        catalogue::search(&db, &query("kestrel"))
            .await
            .expect("search")
            .len(),
        1
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_scope_prefix_narrows_the_field_and_a_stray_colon_stays_text() {
    let (db, dir) = database("scope").await;
    seed(
        &db,
        "22222222-2222-4222-8222-222222222222",
        "Rooftop",
        "Orrin",
        "a seal breaks and the rain never stops",
    )
    .await;
    seed(
        &db,
        "33333333-3333-4333-8333-333333333333",
        "Star Wars: A New Hope",
        "Orrin",
        "space",
    )
    .await;

    // `summary:` finds the row whose summary holds the word but whose title does not.
    let scoped = catalogue::search(&db, &query("summary:rain"))
        .await
        .expect("search");
    assert_eq!(scoped.len(), 1, "summary: reads only the summary");
    assert_eq!(scoped[0].id, "22222222-2222-4222-8222-222222222222");

    // `title:` does NOT find it: the word is in the summary, not the title.
    assert!(catalogue::search(&db, &query("title:rain"))
        .await
        .expect("search")
        .is_empty());

    // `author:` reads the author column.
    assert_eq!(
        catalogue::search(&db, &query("author:orrin"))
            .await
            .expect("search")
            .len(),
        2
    );

    // A colon that is not a scope is part of the text — a title with a colon must stay findable.
    assert_eq!(
        catalogue::search(&db, &query("Star Wars: A New Hope"))
            .await
            .expect("search")
            .len(),
        1
    );

    // An empty scope falls back to the whole query rather than matching everything.
    assert!(catalogue::search(&db, &query("summary:"))
        .await
        .expect("search")
        .is_empty());

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_scope_parser_rejects_a_field_it_does_not_know() {
    assert_eq!(SearchScope::parse("title"), Some(SearchScope::Title));
    assert_eq!(SearchScope::parse(" Summary "), Some(SearchScope::Summary));
    assert_eq!(SearchScope::parse("author"), Some(SearchScope::Author));
    assert_eq!(SearchScope::parse("fandom"), None);
    // The default haystack carries all three, so a bare `q` cannot silently stop reading one of them.
    let all = SearchScope::All.haystack();
    assert!(
        all.contains("title") && all.contains("author_text") && all.contains("summary"),
        "{all}"
    );
}

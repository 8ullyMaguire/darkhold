//! M45-37 — the identifier store on both engines.
//!
//! The row's claim is "identity over location", and these are the properties that make it true rather than
//! aspirational:
//! * one identifier names one thing — a second work claiming the same ISBN is told who holds it;
//! * the same claim twice is an update, not a conflict (imports re-run);
//! * an invalid identifier is refused before it can match the wrong row;
//! * a work cannot detach an identifier it does not hold.

use darkhold_db::identifier::{self as store, Attached, Target};
use darkhold_db::Database;
use darkhold_domain::identifier::Scheme;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-ident-{tag}-{}-{:?}",
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
                "lh_test_ident_{}_{}",
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
    (db, dir)
}

async fn exec(db: &Database, statement: &str) {
    let result = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query(statement)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .map(|_| ()),
        darkhold_db::Backend::Postgres => sqlx::query(statement)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .map(|_| ()),
    };
    result.unwrap_or_else(|error| panic!("{statement}\n{error}"));
}

async fn works(db: &Database, count: usize) -> (Vec<String>, Vec<String>) {
    let account = uuid::Uuid::new_v4().to_string();
    let pseud = uuid::Uuid::new_v4().to_string();
    exec(db, &format!(
        "INSERT INTO accounts (id, email, created_at, updated_at) VALUES ('{account}', 'id-{account}@x.test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
    exec(db, &format!(
        "INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES ('{pseud}', '{account}', 'id{pseud}', 'Id', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
    let mut work_ids = Vec::new();
    let mut edition_ids = Vec::new();
    for index in 0..count {
        let work = uuid::Uuid::new_v4().to_string();
        exec(db, &format!(
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ('{work}', '{pseud}', 'Ident {index}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
        let edition = uuid::Uuid::new_v4().to_string();
        exec(db, &format!(
            "INSERT INTO media_editions (id, work_id, edition_kind, created_at, updated_at) VALUES ('{edition}', '{work}', 'printing', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
        work_ids.push(work);
        edition_ids.push(edition);
    }
    (work_ids, edition_ids)
}

#[tokio::test]
async fn one_identifier_names_one_thing_and_a_second_claim_is_told_who_holds_it() {
    let (db, dir) = database("claim").await;
    let (work_ids, edition_ids) = works(&db, 2).await;
    let first = Target::Work(work_ids[0].clone());

    let stored = store::attach(&db, Scheme::Isbn, "978-0-306-40615-7", &first, "imported")
        .await
        .expect("attach");
    let Attached::Stored(row) = stored else {
        panic!("expected a store, got {stored:?}");
    };
    assert_eq!(
        row.value, "9780306406157",
        "the canonical spelling, not the input"
    );
    assert_eq!(row.work_id.as_deref(), Some(work_ids[0].as_str()));
    assert_eq!(row.source, "imported");

    // The same book, spelled differently, claimed by another work: the answer is the holder.
    let second = Target::Work(work_ids[1].clone());
    match store::attach(&db, Scheme::Isbn, "9780306406157", &second, "curator")
        .await
        .expect("attach")
    {
        Attached::Taken(holder) => {
            assert_eq!(holder.work_id.as_deref(), Some(work_ids[0].as_str()));
            assert_eq!(holder.value, "9780306406157");
        }
        other => panic!("a second claim must be refused with the holder, got {other:?}"),
    }

    // Lookup works from either spelling, because it canonicalises first.
    assert!(store::lookup(&db, Scheme::Isbn, "978 0 306 40615 7")
        .await
        .expect("lookup")
        .is_some());
    assert_eq!(
        store::for_work(&db, &work_ids[0])
            .await
            .expect("for work")
            .len(),
        1
    );
    assert_eq!(
        store::for_work(&db, &work_ids[1])
            .await
            .expect("for work")
            .len(),
        0
    );

    // An edition can hold one too, and it does not collide with the work's.
    let edition = Target::Edition(edition_ids[0].clone());
    let edition_isbn = store::attach(&db, Scheme::Isbn, "0-306-40615-2", &edition, "author")
        .await
        .expect("attach");
    match edition_isbn {
        Attached::Stored(row) => {
            assert_eq!(row.edition_id.as_deref(), Some(edition_ids[0].as_str()));
            assert!(row.work_id.is_none(), "exactly one target");
        }
        other => panic!("{other:?}"),
    }

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_same_claim_twice_is_an_update_not_a_conflict() {
    let (db, dir) = database("rerun").await;
    let (work_ids, _) = works(&db, 1).await;
    let target = Target::Work(work_ids[0].clone());

    let first = store::attach(
        &db,
        Scheme::Musicbrainz,
        "0f2b0f8f-1f5e-4c3a-9e3b-0f8e5c1a2b3c",
        &target,
        "imported",
    )
    .await
    .expect("attach");
    assert!(matches!(first, Attached::Stored(_)));

    // Second import pass, same value in uuid braces, provenance says the author now vouches for it.
    let second = store::attach(
        &db,
        Scheme::Musicbrainz,
        "{0F2B0F8F-1F5E-4C3A-9E3B-0F8E5C1A2B3C}",
        &target,
        "author",
    )
    .await
    .expect("attach");
    match second {
        Attached::Stored(row) => assert_eq!(row.source, "author", "provenance is updated in place"),
        other => panic!("a re-run must not conflict, got {other:?}"),
    }
    assert_eq!(
        store::for_work(&db, &work_ids[0])
            .await
            .expect("for work")
            .len(),
        1,
        "still one row"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_invalid_identifier_is_refused_before_it_can_match_anything() {
    let (db, dir) = database("refuse").await;
    let (work_ids, _) = works(&db, 1).await;
    let target = Target::Work(work_ids[0].clone());

    match store::attach(&db, Scheme::Isbn, "9780306406158", &target, "curator")
        .await
        .expect("attach")
    {
        Attached::Refused(reason) => assert!(reason.contains("check digit"), "{reason}"),
        other => panic!("a bad checksum must be refused, got {other:?}"),
    }
    assert!(
        store::lookup(&db, Scheme::Isbn, "9780306406158")
            .await
            .expect("lookup")
            .is_none(),
        "nothing was written"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_work_cannot_detach_an_identifier_it_does_not_hold() {
    let (db, dir) = database("detach").await;
    let (work_ids, _) = works(&db, 2).await;
    let holder = Target::Work(work_ids[0].clone());
    let other = Target::Work(work_ids[1].clone());

    store::attach(&db, Scheme::Wikidata, "q42", &holder, "curator")
        .await
        .expect("attach");

    assert!(
        !store::detach(&db, Scheme::Wikidata, "Q42", &other)
            .await
            .expect("detach"),
        "the non-holder detaches nothing"
    );
    assert!(
        store::lookup(&db, Scheme::Wikidata, "Q42")
            .await
            .expect("lookup")
            .is_some(),
        "and the row is still there"
    );
    assert!(store::detach(&db, Scheme::Wikidata, "Q42", &holder)
        .await
        .expect("detach"));
    assert!(store::lookup(&db, Scheme::Wikidata, "Q42")
        .await
        .expect("lookup")
        .is_none());

    let _ = std::fs::remove_dir_all(dir);
}

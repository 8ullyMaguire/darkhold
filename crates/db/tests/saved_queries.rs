//! M63-06: the saved-query store, on both engines.
//!
//! Four properties, each a way this feature could be wrong while looking done:
//!
//! 1. **A version 1 document still decodes.** The `saved_views` table is a year
//!    old and its rows were written by a build that knew one query language and
//!    wrote no `kind`. Those rows must keep working, because "upgrade the
//!    document format" is not a reason to break a named query somebody uses.
//! 2. **A catalogue view stores the string it was given, byte for byte.** The
//!    parameters are replayed verbatim later, so a normalisation here is a
//!    different question answered later.
//! 3. **A run diffs against the previous answer, not a timestamp.** A row can
//!    enter a saved query without being new — a recheck moves its
//!    `source_updated_at`, a backfill gives it a fandom, a `-7d` window slides.
//!    Only the previous answer's ids can say what is new.
//! 4. **The run belongs to the view's owner.** A run against a view id from
//!    another account is `None`, not an empty diff, and it does not touch the
//!    owner's memory.
//!
//! Run on SQLite by default; set `DARKHOLD_TEST_PG_URL` to exercise PostgreSQL,
//! which is the arm with the `?::uuid` binds.

use darkhold_db::library::{self, SavedViewKind, SEEN_CAP};
use darkhold_db::Backend;
use test_support::TestDb;

const T0: &str = "2026-01-01T00:00:00Z";

async fn connect(tag: &str) -> TestDb {
    let dir = test_support::scratch_dir(tag);
    TestDb::connect_with_dir(tag, &dir).await
}

/// An account row, because `saved_views.account_id` is a foreign key.
async fn account(tdb: &TestDb, id: &str, email: &str) {
    let db = tdb.db();
    match db.backend() {
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, \
                 permission_statement) VALUES ($1::uuid, $2, 'active', 'adult', $3, $3, 'minimal')",
            )
            .bind(uuid::Uuid::parse_str(id).expect("uuid"))
            .bind(email)
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("account");
        }
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO accounts (id, email, status, age_state, created_at, updated_at, \
                 permission_statement) VALUES (?, ?, 'active', 'adult', ?, ?, 'minimal')",
            )
            .bind(id)
            .bind(email)
            .bind(T0)
            .bind(T0)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("account");
        }
    }
}

/// Write a document exactly as an older build would have written it.
async fn plant_document(tdb: &TestDb, account_id: &str, id: &str, document: &str) {
    let db = tdb.db();
    match db.backend() {
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO saved_views (id, account_id, name, query_json, query_version, sort, \
                 scope, pinned, is_public, created_at, updated_at, version) VALUES \
                 ($1::uuid, $2::uuid, 'planted', $3, 1, 'recent', 'library', false, false, $4, $4, 1)",
            )
            .bind(uuid::Uuid::parse_str(id).expect("uuid"))
            .bind(uuid::Uuid::parse_str(account_id).expect("uuid"))
            .bind(document)
            .bind(T0)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("planted view");
        }
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO saved_views (id, account_id, name, query_json, query_version, sort, \
                 scope, pinned, is_public, created_at, updated_at, version) VALUES \
                 (?, ?, 'planted', ?, 1, 'recent', 'library', 0, 0, ?, ?, 1)",
            )
            .bind(id)
            .bind(account_id)
            .bind(document)
            .bind(T0)
            .bind(T0)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("planted view");
        }
    }
}

async fn delete_view(tdb: &TestDb, id: &str) {
    let db = tdb.db();
    match db.backend() {
        Backend::Postgres => {
            sqlx::query("DELETE FROM saved_views WHERE id = $1::uuid")
                .bind(uuid::Uuid::parse_str(id).expect("uuid"))
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("delete");
        }
        Backend::Sqlite => {
            sqlx::query("DELETE FROM saved_views WHERE id = ?")
                .bind(id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("delete");
        }
    }
}

#[tokio::test]
async fn a_version_1_document_still_decodes_as_a_library_view() {
    let tdb = connect("m6306v1").await;
    let account_id = "11111111-1111-4111-8111-111111111111";
    account(&tdb, account_id, "v1@example.com").await;

    // Exactly what the shipped build wrote: version 1, a query, no `kind`.
    plant_document(
        &tdb,
        account_id,
        "22222222-2222-4222-8222-222222222222",
        r#"{"version":1,"query":{"shelves":["later"],"tags":["naruto"],"sort":"recent"}}"#,
    )
    .await;

    let view =
        library::find_saved_view(tdb.db(), account_id, "22222222-2222-4222-8222-222222222222")
            .await
            .expect("read")
            .expect("the row is there");
    assert_eq!(view.kind, SavedViewKind::Library);
    assert!(!view.needs_repair, "a version 1 document is readable");
    assert_eq!(view.params, None);
    let query = view.query.expect("the query decoded");
    assert_eq!(query.tags, vec!["naruto".to_owned()]);
    assert_eq!(query.shelves, vec!["later".to_owned()]);
}

#[tokio::test]
async fn a_document_from_a_future_build_needs_repair_rather_than_guessing() {
    let tdb = connect("m6306future").await;
    let account_id = "33333333-3333-4333-8333-333333333333";
    account(&tdb, account_id, "future@example.com").await;
    plant_document(
        &tdb,
        account_id,
        "44444444-4444-4444-8444-444444444444",
        r#"{"version":99,"kind":"catalogue","params":"fandom=naruto"}"#,
    )
    .await;

    let view =
        library::find_saved_view(tdb.db(), account_id, "44444444-4444-4444-8444-444444444444")
            .await
            .expect("read")
            .expect("the row is there");
    assert!(view.needs_repair);
    assert!(
        view.query.is_none(),
        "a future document is not read as a query"
    );
    assert!(
        view.params.is_none(),
        "a future document is not read as params either"
    );
}

#[tokio::test]
async fn a_catalogue_view_round_trips_its_params_byte_for_byte() {
    let tdb = connect("m6306params").await;
    let account_id = "55555555-5555-4555-8555-555555555555";
    account(&tdb, account_id, "params@example.com").await;

    // Deliberately untidy: order, a repeated key and a spelling the server would
    // normalise. What is stored must be what was sent, or "the page I ran" is not
    // "the page this answers".
    let params = "limit=2&sort=kudos&fandom=naruto&q=time+travel&sort=kudos";
    let created = library::create_catalogue_view(
        tdb.db(),
        account_id,
        "weekly",
        params,
        darkhold_domain::library::ViewScope::Public,
        false,
    )
    .await
    .expect("create");
    assert_eq!(created.kind, SavedViewKind::Catalogue);
    assert_eq!(created.params.as_deref(), Some(params));
    assert!(created.query.is_none());
    assert_eq!(created.query_version, 2);
    assert!(!created.needs_repair);

    let read = library::find_saved_view(tdb.db(), account_id, &created.id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(read.params.as_deref(), Some(params));
    assert_eq!(read.kind, SavedViewKind::Catalogue);
}

#[tokio::test]
async fn a_query_that_reads_history_is_refused_as_a_public_view() {
    let tdb = connect("m6306history").await;
    let account_id = "66666666-6666-4666-8666-666666666666";
    account(&tdb, account_id, "history@example.com").await;

    let refused = library::create_catalogue_view(
        tdb.db(),
        account_id,
        "unread",
        "fandom=naruto&exclude_read=1",
        darkhold_domain::library::ViewScope::Public,
        false,
    )
    .await
    .expect_err("a shared view cannot depend on one account's history");
    assert!(
        refused.to_string().contains("exclude_read"),
        "the refusal must name the parameter: {refused}"
    );

    // Scoped to the library it is allowed, because it says whose history it reads.
    library::create_catalogue_view(
        tdb.db(),
        account_id,
        "unread",
        "fandom=naruto&exclude_read=1",
        darkhold_domain::library::ViewScope::Library,
        false,
    )
    .await
    .expect("a library-scoped view may read history");
}

#[tokio::test]
async fn a_run_diffs_against_the_previous_answer() {
    let tdb = connect("m6306run").await;
    let account_id = "77777777-7777-4777-8777-777777777777";
    account(&tdb, account_id, "run@example.com").await;
    let view = library::create_catalogue_view(
        tdb.db(),
        account_id,
        "weekly",
        "fandom=naruto&limit=3",
        darkhold_domain::library::ViewScope::Public,
        false,
    )
    .await
    .expect("create");

    let first = library::record_run(
        tdb.db(),
        account_id,
        &view.id,
        &["a".to_owned(), "b".to_owned()],
    )
    .await
    .expect("run")
    .expect("the view exists");
    assert_eq!(first.new_ids, vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(
        first.previous, 0,
        "nothing was recorded before the first run"
    );

    let repeat = library::record_run(
        tdb.db(),
        account_id,
        &view.id,
        &["b".to_owned(), "a".to_owned()],
    )
    .await
    .expect("run")
    .expect("the view exists");
    assert_eq!(
        repeat.new_ids,
        Vec::<String>::new(),
        "the same answer in another order is not new"
    );
    assert_eq!(repeat.previous, 2);

    // `a` left the page and `c` entered it: a diff by creation date would report
    // neither of these.
    let third = library::record_run(
        tdb.db(),
        account_id,
        &view.id,
        &["c".to_owned(), "b".to_owned()],
    )
    .await
    .expect("run")
    .expect("the view exists");
    assert_eq!(third.new_ids, vec!["c".to_owned()]);
    assert_eq!(third.previous, 2);
}

#[tokio::test]
async fn a_run_records_at_most_the_cap_so_one_row_cannot_grow_without_bound() {
    let tdb = connect("m6306cap").await;
    let account_id = "88888888-8888-4888-8888-888888888888";
    account(&tdb, account_id, "cap@example.com").await;
    let view = library::create_catalogue_view(
        tdb.db(),
        account_id,
        "big",
        "limit=100",
        darkhold_domain::library::ViewScope::Public,
        false,
    )
    .await
    .expect("create");

    let ids: Vec<String> = (0..(SEEN_CAP + 10)).map(|i| format!("id-{i}")).collect();
    let run = library::record_run(tdb.db(), account_id, &view.id, &ids)
        .await
        .expect("run")
        .expect("the view exists");
    assert_eq!(
        run.new_ids.len(),
        SEEN_CAP + 10,
        "the answer is about the page the caller sent"
    );

    let stored = library::record_run(tdb.db(), account_id, &view.id, &ids)
        .await
        .expect("run")
        .expect("the view exists");
    assert_eq!(
        stored.previous,
        i64::try_from(SEEN_CAP).expect("cap fits"),
        "only the cap is remembered"
    );
}

#[tokio::test]
async fn a_run_against_another_accounts_view_is_none_and_changes_nothing() {
    let tdb = connect("m6306theirs").await;
    let owner = "99999999-9999-4999-8999-999999999999";
    let stranger = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    account(&tdb, owner, "owner@example.com").await;
    account(&tdb, stranger, "stranger@example.com").await;
    let view = library::create_catalogue_view(
        tdb.db(),
        owner,
        "weekly",
        "fandom=naruto",
        darkhold_domain::library::ViewScope::Public,
        false,
    )
    .await
    .expect("create");

    let theirs = library::record_run(tdb.db(), stranger, &view.id, &["a".to_owned()])
        .await
        .expect("run");
    assert!(theirs.is_none(), "a view id is not a capability");

    // The owner's memory is untouched: their first run is still their first.
    let own = library::record_run(tdb.db(), owner, &view.id, &["a".to_owned()])
        .await
        .expect("run")
        .expect("the view exists");
    assert_eq!(own.previous, 0);
    assert_eq!(own.new_ids, vec!["a".to_owned()]);
}

#[tokio::test]
async fn deleting_a_view_forgets_its_run() {
    let tdb = connect("m6306cascade").await;
    let account_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    account(&tdb, account_id, "cascade@example.com").await;
    let view = library::create_catalogue_view(
        tdb.db(),
        account_id,
        "weekly",
        "fandom=naruto",
        darkhold_domain::library::ViewScope::Public,
        false,
    )
    .await
    .expect("create");
    library::record_run(tdb.db(), account_id, &view.id, &["a".to_owned()])
        .await
        .expect("run")
        .expect("view");

    delete_view(&tdb, &view.id).await;

    let after = library::record_run(tdb.db(), account_id, &view.id, &["a".to_owned()])
        .await
        .expect("run");
    assert!(after.is_none(), "a deleted view has no memory to diff");
}

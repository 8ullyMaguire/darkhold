//! M45-61 — `comments.created_at` is a shape the database keeps.
//!
//! The defect is a wrong answer, not a slow one, so it is worth stating as the failure rather than
//! as the assertion: the welcome rota compares `created_at >= assigned_at` in **TEXT**. A row
//! stored as `2026-11-02 02:13:48+00` is never `>=` a bound written `2026-11-02T02:13:48Z` — the
//! comparison is between strings, and a space is not a `T` — so the query matches nothing and
//! raises nothing. The reader who commented nine minutes after being asked is told they have not
//! commented at all, and no log line disagrees.
//!
//! `community::insert_comment` writes `now_rfc3339`, so live rows are canonical. What was missing
//! was anything stopping the *next* writer, and a test fixture had already produced the bad shape
//! (it bound `$4::timestamptz`, PostgreSQL stored the space-and-offset form, the window matched
//! nothing). Migration 0138 puts the rule in the database as a CHECK on PostgreSQL and a trigger
//! pair on SQLite.
//!
//! Both engines run, because the mechanism differs and only running both proves both: a CHECK and
//! a BEFORE INSERT trigger can each be individually wrong while the other is right.

use darkhold_db::welcome::{complete, open_for, Complete};
use darkhold_db::Backend;
use test_support::TestDb;

/// Insert a comment with a caller-chosen `created_at`, on whichever engine this run is using.
///
/// The point of the helper is that the *test* chooses the shape, not a writer function: the
/// constraint must hold for a writer that has not read the comment in 0138, and a helper built on
/// `insert_comment` could only ever write the shape that function writes.
async fn insert_comment(tdb: &TestDb, id: &str, created_at: &str) -> Result<(), String> {
    let sql = tdb.db().sql(
        "INSERT INTO comments \
         (id, subject_type, subject_id, author_pseud, body, body_version, created_at) \
         VALUES (?, 'work', 'w1', 'p1', 'hi', 'v1', ?)",
        "INSERT INTO comments \
         (id, subject_type, subject_id, author_pseud, body, body_version, created_at) \
         VALUES ($1, 'work', 'w1', 'p1', 'hi', 'v1', $2)",
    );
    match tdb.db().backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(id)
            .bind(created_at)
            .execute(tdb.db().sqlite_pool().expect("sqlite handle"))
            .await
            .map(|_| ())
            .map_err(|error| error.to_string()),
        Backend::Postgres => sqlx::query(&sql)
            .bind(id)
            .bind(created_at)
            .execute(tdb.db().postgres_pool().expect("postgres handle"))
            .await
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}

/// Insert an open rota assignment, `assigned_at` in the canonical shape.
async fn insert_assignment(tdb: &TestDb, id: &str, pseud_id: &str, assigned_at: &str) {
    let sql = tdb.db().sql(
        "INSERT INTO welcome_rota (id, work_id, pseud_id, author_key, assigned_at, due_at) \
         VALUES (?, 'w1', ?, 'author', ?, ?)",
        "INSERT INTO welcome_rota (id, work_id, pseud_id, author_key, assigned_at, due_at) \
         VALUES ($1, 'w1', $2, 'author', $3, $4)",
    );
    let due = "2099-01-01T00:00:00Z";
    match tdb.db().backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(id)
            .bind(pseud_id)
            .bind(assigned_at)
            .bind(due)
            .execute(tdb.db().sqlite_pool().expect("sqlite handle"))
            .await
            .map(|_| ()),
        Backend::Postgres => sqlx::query(&sql)
            .bind(id)
            .bind(pseud_id)
            .bind(assigned_at)
            .bind(due)
            .execute(tdb.db().postgres_pool().expect("postgres handle"))
            .await
            .map(|_| ()),
    }
    .unwrap_or_else(|error| panic!("insert assignment {id}: {error}"));
}

#[tokio::test]
async fn a_non_canonical_created_at_is_refused_by_the_database() {
    let tdb = TestDb::connect_with_dir(
        "m45_61_refused",
        &test_support::scratch_dir("m45_61_refused"),
    )
    .await;

    // The exact shape a `::timestamptz` bind produced. On the unfixed tree this INSERT succeeds,
    // which is the whole bug: the row is stored, lookupable, and invisible to every window.
    let error = insert_comment(&tdb, "c-non-canonical", "2026-11-02 02:13:48+00")
        .await
        .expect_err("a space-and-offset timestamp must be refused");

    assert!(
        error.to_lowercase().contains("canonical"),
        "the error must name the rule, not just fail: {error}"
    );

    tdb.cleanup().await;
}

#[tokio::test]
async fn the_canonical_shape_is_accepted() {
    let tdb = TestDb::connect_with_dir(
        "m45_61_accepted",
        &test_support::scratch_dir("m45_61_accepted"),
    )
    .await;

    // `now_rfc3339`'s shape. A constraint the real writer trips over would be a worse bug than the
    // one it fixes, so this is the test that keeps 0138 honest.
    insert_comment(&tdb, "c-canonical", "2026-11-02T02:13:48Z")
        .await
        .expect("the canonical shape must insert");

    tdb.cleanup().await;
}

#[tokio::test]
async fn an_update_to_a_non_canonical_value_is_refused() {
    let tdb =
        TestDb::connect_with_dir("m45_61_update", &test_support::scratch_dir("m45_61_update"))
            .await;

    insert_comment(&tdb, "c-update", "2026-11-02T02:13:48Z")
        .await
        .expect("canonical insert");

    // A trigger on INSERT alone leaves the same hole one statement later: a backfill or an admin
    // fix that writes the old shape would be accepted and the window would go silent again.
    let sql = tdb.db().sql(
        "UPDATE comments SET created_at = ? WHERE id = ?",
        "UPDATE comments SET created_at = $1 WHERE id = $2",
    );
    let outcome = match tdb.db().backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind("2026-11-02 02:13:48+00")
            .bind("c-update")
            .execute(tdb.db().sqlite_pool().expect("sqlite handle"))
            .await
            .map(|_| ()),
        Backend::Postgres => sqlx::query(&sql)
            .bind("2026-11-02 02:13:48+00")
            .bind("c-update")
            .execute(tdb.db().postgres_pool().expect("postgres handle"))
            .await
            .map(|_| ()),
    };
    let error = outcome.expect_err("an UPDATE to a non-canonical shape must be refused");
    assert!(
        error.to_string().to_lowercase().contains("canonical"),
        "the error must name the rule: {error}"
    );

    tdb.cleanup().await;
}

#[tokio::test]
async fn the_rota_finds_a_comment_written_after_the_assignment() {
    let tdb =
        TestDb::connect_with_dir("m45_61_rota", &test_support::scratch_dir("m45_61_rota")).await;

    // The good path, through the real function: an assignment at 02:00 and a comment nine minutes
    // later is evidence. This is the behaviour the constraint must not break, and the behaviour
    // the old shape made silently unreachable.
    insert_assignment(&tdb, "a1", "p1", "2026-11-02T02:00:00Z").await;
    insert_comment(&tdb, "c-after", "2026-11-02T02:09:00Z")
        .await
        .expect("canonical insert");

    let outcome = complete(tdb.db(), "p1", "a1").await.expect("complete");
    assert!(
        matches!(outcome, Complete::Done(_)),
        "a comment written after the assignment is the evidence the rota asks for, and the \
         comparison is by text: {outcome:?}"
    );

    // And the other direction, so the test cannot pass on "any comment counts": a comment written
    // *before* the assignment is not evidence, which is what makes the window a window.
    insert_assignment(&tdb, "a2", "p2", "2026-11-02T02:00:00Z").await;
    let before = tdb.db().sql(
        "INSERT INTO comments (id, subject_type, subject_id, author_pseud, body, body_version, \
         created_at) VALUES (?, 'work', 'w1', 'p2', 'earlier', 'v1', ?)",
        "INSERT INTO comments (id, subject_type, subject_id, author_pseud, body, body_version, \
         created_at) VALUES ($1, 'work', 'w1', 'p2', 'earlier', 'v1', $2)",
    );
    match tdb.db().backend() {
        Backend::Sqlite => sqlx::query(&before)
            .bind("c-before")
            .bind("2026-11-02T01:00:00Z")
            .execute(tdb.db().sqlite_pool().expect("sqlite handle"))
            .await
            .map(|_| ()),
        Backend::Postgres => sqlx::query(&before)
            .bind("c-before")
            .bind("2026-11-02T01:00:00Z")
            .execute(tdb.db().postgres_pool().expect("postgres handle"))
            .await
            .map(|_| ()),
    }
    .expect("insert the earlier comment");

    let outcome = complete(tdb.db(), "p2", "a2").await.expect("complete");
    assert!(
        matches!(outcome, Complete::NoCommentYet { .. }),
        "a comment written before the assignment is not evidence: {outcome:?}"
    );

    tdb.cleanup().await;
}

#[tokio::test]
async fn an_assignment_whose_library_row_is_gone_still_decodes() {
    let tdb =
        TestDb::connect_with_dir("m45_61_decode", &test_support::scratch_dir("m45_61_decode"))
            .await;

    // Found by the first PostgreSQL run of this suite, not by reading: the assignment query read
    // `li.title` through a LEFT JOIN to `library_items`, and a rota row whose library entry is gone
    // (the column is `ON DELETE SET NULL`, and a reader unshelving takes the row) handed back NULL.
    // `get::<String, _>` on NULL raises `UnexpectedNullError` — a 500 on PostgreSQL — while SQLite
    // returns an empty string and renders. An assignment whose work cannot be named is still an
    // assignment the reader was asked to welcome, so the query now COALESCEs both columns.
    insert_assignment(&tdb, "a-gone", "p1", "2026-11-02T02:00:00Z").await;

    let assignment = open_for(tdb.db(), "p1")
        .await
        .expect("an assignment with no library row must decode, not fail")
        .expect("the assignment is open");

    assert_eq!(
        assignment.title, "",
        "a work that cannot be named reads as unnamed, not as a decode error"
    );

    tdb.cleanup().await;
}

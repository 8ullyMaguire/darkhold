//! M43 — Instance themes (`crates/db/src/instance_theme.rs`, spec §43), the
//! federation-discovery side of a Darkhold instance.
//!
//! Three `pub async fn` plus `tag_weights`, and all four are live on
//! `routes/federation.rs`: read the local theme, upsert it, list the public ones
//! for peer discovery, and read the weights back out of a vector.
//!
//! A fourth function, `compute_theme_from_bookmarks`, was deleted in M70-13. Its
//! tag query joined tables that do not exist in any migration and it answered
//! `{}` on both engines with the error swallowed, and it had no caller: the
//! federation route takes the vector from the operator. The section of tests
//! that kept it "covered" went with it — a suite guarding a function nobody can
//! call is the reason the defect sat here for months.
//!
//! The schema is friendly here: `theme_vector` and the timestamps are TEXT on
//! both engines (ADR 0004) and `public` is BOOLEAN on both, so there is no
//! native-type decode to get wrong. What the suite checks is the behaviour:
//!
//! - **A theme is private by default and stays that way unless asked.** The
//!   federation surface is opt-in; an instance that never opts in must not
//!   appear in `list_public_themes`.
//! - **A theme vector is a JSON object of `tag -> weight`**, stored as text and
//!   parsed back. `tag_weights` is the only consumer that cares about the
//!   weight being a number.
//! - **Upsert replaces rather than accumulates**, and recomputes
//!   `computed_at`, so a stale vector cannot linger under a fresh timestamp.
//!
//! One real defect is found and fixed here; see `compute_theme_ignores_`
//! below and `docs/known-gaps.md`.

use std::path::PathBuf;

use darkhold_db::instance_theme::{get_local_theme, list_public_themes, tag_weights, upsert_theme};
use serde_json::json;
use test_support::TestDb;
use uuid::Uuid;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-theme-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

struct Harness {
    tdb: TestDb,
    #[allow(dead_code)]
    dir: PathBuf,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let tdb = TestDb::connect_with_dir(tag, &dir).await;
        Self { tdb, dir }
    }

    fn db(&self) -> &darkhold_db::Database {
        self.tdb.db()
    }

    #[allow(dead_code)]
    fn is_pg(&self) -> bool {
        self.db().backend() == darkhold_db::Backend::Postgres
    }

    /// Kept even though this suite no longer calls it: `is_pg`/`exec` are how the
    /// suites in this directory seed rows the API cannot produce, and the next
    /// test added here will need them. A `#[allow(dead_code)]` says that on
    /// purpose, where a deletion would make the next author re-derive it.
    #[allow(dead_code)]
    async fn exec(&self, query: &str) {
        let q = self.tdb.sql(query);
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(&q)
                    .execute(self.db().sqlite_pool().expect("sqlite"))
                    .await
                    .expect("exec");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(&q)
                    .execute(self.db().postgres_pool().expect("pg"))
                    .await
                    .expect("exec");
            }
        }
    }

    async fn count(&self, table: &str) -> i64 {
        let q = self.tdb.sql(&format!("SELECT COUNT(*) FROM {table}"));
        match self.db().backend() {
            darkhold_db::Backend::Sqlite => sqlx::query_scalar::<_, i64>(&q)
                .fetch_one(self.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("count"),
            darkhold_db::Backend::Postgres => sqlx::query_scalar::<_, i64>(&q)
                .fetch_one(self.db().postgres_pool().expect("pg"))
                .await
                .expect("count"),
        }
    }

    fn instance() -> String {
        format!("peer-{}", Uuid::new_v4())
    }
}

// ---------------------------------------------------------------------------
// upsert_theme / get_local_theme
// ---------------------------------------------------------------------------

/// An upserted theme reads back with its vector, privacy and timestamps.
#[tokio::test]
async fn a_theme_round_trips() {
    let h = Harness::new("theme-roundtrip").await;
    let id = Harness::instance();
    let vector = json!({"slowburn": 0.8, "angst": 0.4});
    upsert_theme(h.db(), &id, &vector, false, "2026-01-01T00:00:00Z")
        .await
        .expect("upsert");

    let theme = get_local_theme(h.db(), &id)
        .await
        .expect("read")
        .expect("present");
    assert_eq!(theme.instance_id, id);
    assert_eq!(
        theme.theme_vector, vector,
        "the vector survives the round trip"
    );
    assert!(!theme.public, "private by default");
    assert_eq!(theme.computed_at, "2026-01-01T00:00:00Z");
    assert_eq!(theme.updated_at, "2026-01-01T00:00:00Z");
}

/// **Upserting twice replaces rather than accumulating**, and re-stamps
/// `computed_at` — a stale vector must not linger under a fresh timestamp.
#[tokio::test]
async fn a_second_upsert_replaces_the_vector() {
    let h = Harness::new("theme-replace").await;
    let id = Harness::instance();
    upsert_theme(
        h.db(),
        &id,
        &json!({"old": 1.0}),
        false,
        "2026-01-01T00:00:00Z",
    )
    .await
    .expect("first");
    upsert_theme(
        h.db(),
        &id,
        &json!({"new": 0.5}),
        true,
        "2026-02-01T00:00:00Z",
    )
    .await
    .expect("second");

    assert_eq!(h.count("instance_themes").await, 1, "one row, replaced");
    let theme = get_local_theme(h.db(), &id)
        .await
        .expect("read")
        .expect("present");
    assert_eq!(
        theme.theme_vector,
        json!({"new": 0.5}),
        "the new vector wins"
    );
    assert!(theme.public, "and privacy can change on update");
    assert_eq!(theme.computed_at, "2026-02-01T00:00:00Z", "recomputed");
}

/// An empty vector is legal and reads back as an empty object.
#[tokio::test]
async fn an_empty_vector_round_trips() {
    let h = Harness::new("theme-empty").await;
    let id = Harness::instance();
    upsert_theme(h.db(), &id, &json!({}), false, "2026-01-01T00:00:00Z")
        .await
        .expect("upsert");
    let theme = get_local_theme(h.db(), &id)
        .await
        .expect("read")
        .expect("present");
    assert_eq!(theme.theme_vector, json!({}));
}

/// An instance with no theme reads as `None`, not as an empty theme.
#[tokio::test]
async fn an_unknown_instance_has_no_theme() {
    let h = Harness::new("theme-absent").await;
    assert!(get_local_theme(h.db(), &Harness::instance())
        .await
        .expect("read")
        .is_none());
}

/// Two instances keep separate themes.
#[tokio::test]
async fn two_instances_keep_separate_themes() {
    let h = Harness::new("theme-two").await;
    let a = Harness::instance();
    let b = Harness::instance();
    upsert_theme(
        h.db(),
        &a,
        &json!({"x": 1.0}),
        false,
        "2026-01-01T00:00:00Z",
    )
    .await
    .expect("a");
    upsert_theme(
        h.db(),
        &b,
        &json!({"y": 1.0}),
        false,
        "2026-01-01T00:00:00Z",
    )
    .await
    .expect("b");

    assert_eq!(h.count("instance_themes").await, 2);
    assert_eq!(
        get_local_theme(h.db(), &a)
            .await
            .expect("read")
            .expect("a")
            .theme_vector,
        json!({"x": 1.0})
    );
    assert_eq!(
        get_local_theme(h.db(), &b)
            .await
            .expect("read")
            .expect("b")
            .theme_vector,
        json!({"y": 1.0})
    );
}

// ---------------------------------------------------------------------------
// list_public_themes
// ---------------------------------------------------------------------------

/// **A private theme is not listed.** The whole federation surface is opt-in, so
/// an instance that never published must not appear.
#[tokio::test]
async fn private_themes_are_not_listed() {
    let h = Harness::new("theme-private").await;
    upsert_theme(
        h.db(),
        &Harness::instance(),
        &json!({"secret": 1.0}),
        false,
        "2026-01-01T00:00:00Z",
    )
    .await
    .expect("upsert");
    assert!(list_public_themes(h.db()).await.expect("list").is_empty());
}

/// A public theme is listed, newest first.
#[tokio::test]
async fn public_themes_are_listed_newest_first() {
    let h = Harness::new("theme-public").await;
    let old = Harness::instance();
    let new = Harness::instance();
    upsert_theme(
        h.db(),
        &old,
        &json!({"a": 1.0}),
        true,
        "2026-01-01T00:00:00Z",
    )
    .await
    .expect("old");
    upsert_theme(
        h.db(),
        &new,
        &json!({"b": 1.0}),
        true,
        "2026-02-01T00:00:00Z",
    )
    .await
    .expect("new");

    let listed = list_public_themes(h.db()).await.expect("list");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].instance_id, new, "newest first");
    assert_eq!(listed[1].instance_id, old);
    assert!(listed.iter().all(|t| t.public));
}

/// Public and private themes coexist; only the public one is listed.
#[tokio::test]
async fn only_the_public_theme_of_two_is_listed() {
    let h = Harness::new("theme-mixed").await;
    let public = Harness::instance();
    let private = Harness::instance();
    upsert_theme(
        h.db(),
        &public,
        &json!({"a": 1.0}),
        true,
        "2026-01-01T00:00:00Z",
    )
    .await
    .expect("public");
    upsert_theme(
        h.db(),
        &private,
        &json!({"b": 1.0}),
        false,
        "2026-01-01T00:00:00Z",
    )
    .await
    .expect("private");

    let listed = list_public_themes(h.db()).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].instance_id, public);
}

/// **Going from public back to private withdraws the instance from discovery.**
#[tokio::test]
async fn flipping_to_private_withdraws_from_discovery() {
    let h = Harness::new("theme-withdraw").await;
    let id = Harness::instance();
    upsert_theme(
        h.db(),
        &id,
        &json!({"a": 1.0}),
        true,
        "2026-01-01T00:00:00Z",
    )
    .await
    .expect("publish");
    assert_eq!(list_public_themes(h.db()).await.expect("list").len(), 1);

    upsert_theme(
        h.db(),
        &id,
        &json!({"a": 1.0}),
        false,
        "2026-02-01T00:00:00Z",
    )
    .await
    .expect("unpublish");
    assert!(
        list_public_themes(h.db()).await.expect("list").is_empty(),
        "and it is no longer discoverable"
    );
}

/// With no themes at all the list is empty, not an error.
#[tokio::test]
async fn an_empty_instance_lists_nothing() {
    let h = Harness::new("theme-list-empty").await;
    assert!(list_public_themes(h.db()).await.expect("list").is_empty());
}

/// A listed theme carries its vector, so a peer can match on it.
#[tokio::test]
async fn a_listed_theme_carries_its_vector() {
    let h = Harness::new("theme-list-vector").await;
    let vector = json!({"slowburn": 0.75, "found-family": 0.25});
    upsert_theme(
        h.db(),
        &Harness::instance(),
        &vector,
        true,
        "2026-01-01T00:00:00Z",
    )
    .await
    .expect("upsert");

    let listed = list_public_themes(h.db()).await.expect("list");
    assert_eq!(listed[0].theme_vector, vector);
    let weights = tag_weights(&listed[0].theme_vector);
    assert_eq!(weights.len(), 2);
    assert!((weights["slowburn"] - 0.75).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------------------
// tag_weights
// ---------------------------------------------------------------------------

/// Weights come back as numbers.
#[test]
fn weights_are_read_from_the_vector() {
    let weights = tag_weights(&json!({"a": 0.5, "b": 0.25}));
    assert_eq!(weights.len(), 2);
    assert!((weights["a"] - 0.5).abs() < f64::EPSILON);
    assert!((weights["b"] - 0.25).abs() < f64::EPSILON);
}

/// **A non-numeric weight is skipped rather than becoming zero.** `as_f64`
/// returns `None` for a string or bool, and a silent `0.0` would quietly rank a
/// malformed tag last instead of excluding it.
#[test]
fn a_non_numeric_weight_is_skipped() {
    let weights = tag_weights(&json!({"good": 0.5, "bad": "high", "also_bad": true}));
    assert_eq!(weights.len(), 1, "only the numeric weight survives");
    assert!(weights.contains_key("good"));
}

/// A vector that is not an object yields no weights.
#[test]
fn a_non_object_vector_yields_no_weights() {
    assert!(tag_weights(&json!([1, 2, 3])).is_empty());
    assert!(tag_weights(&json!("nope")).is_empty());
    assert!(tag_weights(&json!(null)).is_empty());
}

/// A null weight is skipped, like any other non-number.
#[test]
fn a_null_weight_is_skipped() {
    let weights = tag_weights(&json!({"a": null, "b": 0.5}));
    assert_eq!(weights.len(), 1);
    assert!(weights.contains_key("b"));
}

/// An integer weight is a number.
#[test]
fn an_integer_weight_is_a_number() {
    let weights = tag_weights(&json!({"a": 1}));
    assert!((weights["a"] - 1.0).abs() < f64::EPSILON);
}

/// An empty object yields no weights.
#[test]
fn an_empty_vector_yields_no_weights() {
    assert!(tag_weights(&json!({})).is_empty());
}

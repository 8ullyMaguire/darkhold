//! M45-70 — reading a reader's signals out of the tables that already hold them.
//!
//! Nothing here recomputes a ranking: the explanation is about the *acts* a reader performed, which the
//! tree already records. Five queries, one per signal, each returning `(work, tag)` pairs; the weighting
//! and the ordering are in `darkhold_domain::taste_explain`, so this module can stay a read.
//!
//! `reading_status.subject_id` and `bookmarks.subject_id` are polymorphic (`subject_type`), so both
//! queries pin `subject_type = 'work'`: a chapter bookmark says something about a chapter, and treating it
//! as evidence about the work's tags would attribute a reader's place-marker to a whole story.

use crate::{Backend, Database};
use anyhow::Result;
use darkhold_domain::taste_explain::{Event, Signal};

/// Every act this reader has performed that carries tags, as `(work, tag, signal)`.
pub async fn events(db: &Database, account_id: &str) -> Result<Vec<Event>> {
    let mut out = Vec::new();
    for (signal, sql_sqlite, sql_postgres) in statements() {
        let rows: Vec<(String, String)> = match db.backend() {
            Backend::Sqlite => {
                sqlx::query_as(sql_sqlite)
                    .bind(account_id)
                    .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                    .await?
            }
            Backend::Postgres => {
                sqlx::query_as(sql_postgres)
                    .bind(account_id)
                    .fetch_all(db.postgres_pool().expect("postgres handle"))
                    .await?
            }
        };
        out.extend(
            rows.into_iter()
                .map(|(work, tag)| Event { work, tag, signal }),
        );
    }
    Ok(out)
}

/// The five reads, as `(signal, sqlite, postgres)`.
///
/// One statement per engine rather than a shared string with `?`: the `dialects` gate refuses a `?` in a
/// PostgreSQL arm, and a `::uuid` cast in a SQLite one.
fn statements() -> [(Signal, &'static str, &'static str); 5] {
    [
        (
            Signal::Finished,
            "SELECT rs.subject_id, tn.canonical
             FROM reading_status rs
             JOIN work_tags wt ON wt.work_id = rs.subject_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE rs.account_id = ? AND rs.subject_type = 'work' AND rs.status = 'finished'",
            "SELECT rs.subject_id::text AS subject_id, tn.canonical
             FROM reading_status rs
             JOIN work_tags wt ON wt.work_id = rs.subject_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE rs.account_id = $1::uuid AND rs.subject_type = 'work' AND rs.status = 'finished'",
        ),
        (
            Signal::Bookmark,
            "SELECT b.subject_id, tn.canonical
             FROM bookmarks b
             JOIN work_tags wt ON wt.work_id = b.subject_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE b.account_id = ? AND b.subject_type = 'work'",
            "SELECT b.subject_id::text AS subject_id, tn.canonical
             FROM bookmarks b
             JOIN work_tags wt ON wt.work_id = b.subject_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE b.account_id = $1::uuid AND b.subject_type = 'work'",
        ),
        (
            Signal::RatingUp,
            "SELECT r.work_id, tn.canonical
             FROM rating r
             JOIN work_tags wt ON wt.work_id = r.work_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE r.account_id = ? AND r.stars >= 4 AND r.deleted_at IS NULL",
            "SELECT r.work_id::text AS subject_id, tn.canonical
             FROM rating r
             JOIN work_tags wt ON wt.work_id = r.work_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE r.account_id = $1::uuid AND r.stars >= 4 AND r.deleted_at IS NULL",
        ),
        (
            Signal::RatingDown,
            "SELECT r.work_id, tn.canonical
             FROM rating r
             JOIN work_tags wt ON wt.work_id = r.work_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE r.account_id = ? AND r.stars <= 2 AND r.deleted_at IS NULL",
            "SELECT r.work_id::text AS subject_id, tn.canonical
             FROM rating r
             JOIN work_tags wt ON wt.work_id = r.work_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE r.account_id = $1::uuid AND r.stars <= 2 AND r.deleted_at IS NULL",
        ),
        (
            Signal::Dropped,
            "SELECT rs.subject_id, tn.canonical
             FROM reading_status rs
             JOIN work_tags wt ON wt.work_id = rs.subject_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE rs.account_id = ? AND rs.subject_type = 'work' AND rs.status = 'dropped'",
            "SELECT rs.subject_id::text AS subject_id, tn.canonical
             FROM reading_status rs
             JOIN work_tags wt ON wt.work_id = rs.subject_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE rs.account_id = $1::uuid AND rs.subject_type = 'work' AND rs.status = 'dropped'",
        ),
    ]
}

/// The signals this reader has switched off, by key.
///
/// Absence means on: a preference table that listed every signal as "on" would be a second copy of the
/// defaults, and a second copy is a thing that drifts.
pub async fn disabled_signals(db: &Database, account_id: &str) -> Result<Vec<String>> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT signal FROM taste_signal_prefs WHERE account_id = ? AND enabled = FALSE"
        }
        Backend::Postgres => {
            "SELECT signal FROM taste_signal_prefs WHERE account_id = $1::uuid AND enabled = FALSE"
        }
    };
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(|(signal,)| signal).collect())
}

/// Turn one signal on or off.
pub async fn set_signal(
    db: &Database,
    account_id: &str,
    signal: &str,
    enabled: bool,
    now: &str,
) -> Result<()> {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO taste_signal_prefs (account_id, signal, enabled, updated_at)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(account_id, signal) DO UPDATE SET
                   enabled = excluded.enabled,
                   updated_at = excluded.updated_at",
            )
            .bind(account_id)
            .bind(signal)
            .bind(enabled)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO taste_signal_prefs (account_id, signal, enabled, updated_at)
                 VALUES ($1::uuid, $2, $3, $4)
                 ON CONFLICT(account_id, signal) DO UPDATE SET
                   enabled = EXCLUDED.enabled,
                   updated_at = EXCLUDED.updated_at",
            )
            .bind(account_id)
            .bind(signal)
            .bind(enabled)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?;
        }
    }
    Ok(())
}

//! §9.6 / M47-08b: a reader's private search history.
//!
//! Three operations and no more: record one successful search, read the newest few, clear them
//! all. Everything the surface needs is here, and nothing reads this table for anything except
//! the reader who owns it — it is not a recommendation input, which is why there is no aggregate,
//! no cross-pseud query and no account-level row.

use anyhow::Result;
use serde::Serialize;
use uuid::Uuid;

use crate::{Backend, Database};

/// One recorded search, as the reader's own UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryEntry {
    pub query: String,
    /// How many works the query returned when it was run. Stored, not recomputed: the same query
    /// run tomorrow returns a different number, and a history that disagreed with what the reader
    /// saw would be worse than one showing no number.
    pub result_count: i64,
    /// Canonical `YYYY-MM-DDTHH:MM:SSZ`.
    pub searched_at: String,
}

/// Normalise a query for the uniqueness key.
///
/// Trimmed, internal runs of whitespace collapsed to one space, lowercased. This is the piece
/// that must not live in SQL: SQLite folds ASCII only, so `Éowyn` and `éowyn` would be two rows on
/// that engine and one on PostgreSQL. One Rust function means both engines compare the same bytes.
#[must_use]
pub fn query_key(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether a query is worth recording at all.
///
/// A one-character query is a keystroke the reader was still typing, and an empty one is the
/// search box being cleared. Neither is a search the reader would want offered back.
#[must_use]
pub fn worth_recording(query: &str) -> bool {
    query.trim().chars().count() >= 2
}

/// Record one successful search, keeping only the newest `cap` per pseud.
///
/// Re-running a query updates its row (new count, new timestamp) instead of adding a second one,
/// and the trim afterwards is not optional: without it the table grows one row per distinct query
/// forever, which turns a history into a log — the one thing the reader did not agree to.
pub async fn record(
    db: &Database,
    pseud_id: Uuid,
    query: &str,
    result_count: i64,
    now: &str,
    cap: i64,
) -> Result<()> {
    if !worth_recording(query) {
        return Ok(());
    }
    let trimmed = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let key = query_key(query);
    let id = Uuid::new_v4().to_string();
    let cap = cap.max(1);

    let upsert = db.sql(
        "INSERT INTO search_history (id, pseud_id, query, query_key, result_count, searched_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (pseud_id, query_key)
         DO UPDATE SET query = excluded.query,
                       result_count = excluded.result_count,
                       searched_at = excluded.searched_at",
        "INSERT INTO search_history (id, pseud_id, query, query_key, result_count, searched_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (pseud_id, query_key)
         DO UPDATE SET query = excluded.query,
                       result_count = excluded.result_count,
                       searched_at = excluded.searched_at",
    );
    let trim = db.sql(
        "DELETE FROM search_history
         WHERE pseud_id = ?
           AND id NOT IN (
             SELECT id FROM search_history WHERE pseud_id = ?
             ORDER BY searched_at DESC, id DESC LIMIT ?
           )",
        "DELETE FROM search_history
         WHERE pseud_id = ?
           AND id NOT IN (
             SELECT id FROM search_history WHERE pseud_id = ?
             ORDER BY searched_at DESC, id DESC LIMIT ?
           )",
    );

    match db.backend() {
        Backend::Sqlite => {
            let pool = db.sqlite_pool().expect("sqlite handle");
            sqlx::query(&upsert)
                .bind(&id)
                .bind(pseud_id.to_string())
                .bind(&trimmed)
                .bind(&key)
                .bind(result_count)
                .bind(now)
                .execute(pool)
                .await?;
            sqlx::query(&trim)
                .bind(pseud_id.to_string())
                .bind(pseud_id.to_string())
                .bind(cap)
                .execute(pool)
                .await?;
        }
        Backend::Postgres => {
            let pool = db.postgres_pool().expect("postgres handle");
            sqlx::query(&upsert)
                .bind(&id)
                .bind(pseud_id.to_string())
                .bind(&trimmed)
                .bind(&key)
                .bind(result_count)
                .bind(now)
                .execute(pool)
                .await?;
            sqlx::query(&trim)
                .bind(pseud_id.to_string())
                .bind(pseud_id.to_string())
                .bind(cap)
                .execute(pool)
                .await?;
        }
    }

    Ok(())
}

/// The newest `limit` entries for one pseud, newest first.
pub async fn recent(db: &Database, pseud_id: Uuid, limit: i64) -> Result<Vec<HistoryEntry>> {
    let limit = limit.clamp(1, 50);
    let sql = db.sql(
        "SELECT query, result_count, searched_at
         FROM search_history
         WHERE pseud_id = ?
         ORDER BY searched_at DESC, id DESC
         LIMIT ?",
        "SELECT query, result_count, searched_at
         FROM search_history
         WHERE pseud_id = ?
         ORDER BY searched_at DESC, id DESC
         LIMIT ?",
    );

    let rows: Vec<(String, i64, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(pseud_id.to_string())
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(pseud_id.to_string())
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(|(query, result_count, searched_at)| HistoryEntry {
            query,
            result_count,
            searched_at,
        })
        .collect())
}

/// Remove every row for one pseud; the count is what the caller reports back.
pub async fn clear(db: &Database, pseud_id: Uuid) -> Result<u64> {
    // `db.sql`, not one literal for both arms. A bare `?` reaches PostgreSQL as the literal token
    // `?` — sqlx does not translate it — and the server answers "syntax error at end of input",
    // a message that names neither the placeholder nor the engine. The other two functions here
    // go through `db.sql` for exactly this reason; this one did not, and only the PostgreSQL arm
    // of `clearing_removes_only_this_pseud_and_reports_the_count` could tell.
    let sql = db.sql(
        "DELETE FROM search_history WHERE pseud_id = ?",
        "DELETE FROM search_history WHERE pseud_id = ?",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(pseud_id.to_string())
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(pseud_id.to_string())
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };

    Ok(affected)
}

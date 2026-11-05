//! M70-07 — Marked for Later: a private queue, not a private bookmark.
//!
//! See `migrations/*/0158_marked_for_later.sql` for why this is its own table. The short version: a bookmark is a
//! record and this is a queue. A queue entry disappears when the work is read; a bookmark stays. A queue entry must
//! never reach a count or a taste vector, and it must be unreachable from any surface that lists bookmarks — both
//! true here because nothing else reads this table.
//!
//! # Every function takes the account
//!
//! There is no "all entries" call and deliberately none: the only way to read this table is scoped to one account,
//! which is what makes it private by construction rather than by a filter somebody has to remember.

use anyhow::Result;
use serde::Serialize;

use crate::{Backend, Database};

/// One queued work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The work waiting to be read.
    pub work_id: String,
    /// When it was queued, RFC 3339 UTC.
    pub marked_at: String,
}

/// Queue a work. Idempotent: queueing twice is not two entries and not an error.
///
/// Returns `true` when this call added the entry, `false` when it was already there — the difference matters to a
/// caller that wants to say "already on your list" rather than "added".
pub async fn mark(db: &Database, account_id: &str, work_id: &str) -> Result<bool> {
    let statement = db.sql(
        "INSERT INTO marked_for_later (account_id, work_id, marked_at) VALUES (?, ?, ?) \
         ON CONFLICT (account_id, work_id) DO NOTHING",
        "INSERT INTO marked_for_later (account_id, work_id, marked_at) VALUES ($1::uuid, $2::uuid, $3) \
         ON CONFLICT (account_id, work_id) DO NOTHING",
    );
    let now = now_rfc3339();
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(account_id)
            .bind(work_id)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(account_id)
            .bind(work_id)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// Drop a work from the queue. Returns whether there was an entry to drop.
pub async fn unmark(db: &Database, account_id: &str, work_id: &str) -> Result<bool> {
    let statement = db.sql(
        "DELETE FROM marked_for_later WHERE account_id = ? AND work_id = ?",
        "DELETE FROM marked_for_later WHERE account_id = $1::uuid AND work_id = $2::uuid",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(account_id)
            .bind(work_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(account_id)
            .bind(work_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// Whether one work is queued.
pub async fn is_marked(db: &Database, account_id: &str, work_id: &str) -> Result<bool> {
    let statement = db.sql(
        "SELECT count(*) FROM marked_for_later WHERE account_id = ? AND work_id = ?",
        "SELECT count(*) FROM marked_for_later WHERE account_id = $1::uuid AND work_id = $2::uuid",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&statement)
                .bind(account_id)
                .bind(work_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&statement)
                .bind(account_id)
                .bind(work_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(count > 0)
}

/// The queue, most recently queued first.
pub async fn queue(db: &Database, account_id: &str, limit: i64) -> Result<Vec<Entry>> {
    let statement = db.sql(
        "SELECT work_id, marked_at FROM marked_for_later WHERE account_id = ? \
          ORDER BY marked_at DESC, work_id DESC LIMIT ?",
        "SELECT work_id::text AS work_id, marked_at FROM marked_for_later WHERE account_id = $1::uuid \
          ORDER BY marked_at DESC, work_id DESC LIMIT $2",
    );
    let rows: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(account_id)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(account_id)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(work_id, marked_at)| Entry { work_id, marked_at })
        .collect())
}

fn now_rfc3339() -> String {
    // The same clock the rest of the crate uses, so a sort by `marked_at` cannot disagree with a sort by any other
    // timestamp in the product.
    crate::identity::now_rfc3339()
}

//! Reads and writes the outbound ATProto bridge needs (`docs/FEDERATION.md` §2, W6 §5).
//!
//! Two functions, both deliberately dull. `work_for_record` returns a work **only when it is public and published** — a
//! draft or a private work must never become a record, and that filter is the whole reason publication is a worker step
//! rather than a trigger on insert. `set_at_uri` writes the `at://` URI back on the row, which is what makes the purge in
//! `docs/FEDERATION.md` §9 a walk over a column instead of a re-derivation.

use sqlx::Row;

use crate::{Backend, Database};

/// A published work, in the shape the record builder takes.
#[derive(Debug, Clone)]
pub struct WorkForRecord {
    /// The work id — the record key is derived from it, so a replayed event rewrites one record rather than adding one.
    pub id: String,
    /// What the card and the post text carry.
    pub title: String,
    /// The card description, truncated by the caller.
    pub summary: String,
    /// BCP 47, from the row.
    pub language: String,
    /// `published_at` when it exists, `created_at` otherwise — a record must have a creation time that is stable across
    /// replays, and "now" is not.
    pub created_at: String,
}

fn pool_err() -> sqlx::Error {
    sqlx::Error::PoolClosed
}

/// The work to publish, or `None` when it is not public, not published, or deleted.
///
/// `None` is not an error: a `work.published` event for a work that has since been withdrawn is a normal race, and the
/// worker treats it as "nothing to do" rather than as a delivery failure that would retry for twelve hours.
pub async fn work_for_record(
    db: &Database,
    work_id: &str,
) -> Result<Option<WorkForRecord>, sqlx::Error> {
    let sql = db.sql(
        "SELECT id, title, summary, language, COALESCE(published_at, created_at) AS created_at
           FROM works
          WHERE id = ?
            AND visibility = 'public'
            AND lifecycle = 'published'
            AND deleted_at IS NULL",
        "SELECT id, title, summary, language, COALESCE(published_at, created_at) AS created_at
           FROM works
          WHERE id = $1::uuid
            AND visibility = 'public'
            AND lifecycle = 'published'
            AND deleted_at IS NULL",
    );

    // The mapping happens inside each arm on purpose: `SqliteRow` and `PgRow` are different types, so a `match` that
    // returns the *row* cannot type-check — the arms must be reduced to a common type before they meet. Measured as
    // `error[E0308]: match arms have incompatible types` on the first version of this function.
    let work = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(work_id)
                .fetch_optional(db.sqlite_pool().ok_or_else(pool_err)?)
                .await?
                .map(|r| WorkForRecord {
                    id: r.get("id"),
                    title: r.get("title"),
                    summary: r.get("summary"),
                    language: r.get("language"),
                    created_at: r.get("created_at"),
                })
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(work_id)
                .fetch_optional(db.postgres_pool().ok_or_else(pool_err)?)
                .await?
                .map(|r| WorkForRecord {
                    id: r.get("id"),
                    title: r.get("title"),
                    summary: r.get("summary"),
                    language: r.get("language"),
                    created_at: r.get("created_at"),
                })
        }
    };

    Ok(work)
}

/// Record where the work was published. Idempotent by construction: it overwrites with the same value on a replay.
pub async fn set_at_uri(db: &Database, work_id: &str, at_uri: &str) -> Result<(), sqlx::Error> {
    let sql = db.sql(
        "UPDATE works SET at_uri = ?, updated_at = ? WHERE id = ?",
        "UPDATE works SET at_uri = $1, updated_at = $2 WHERE id = $3::uuid",
    );
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(at_uri)
                .bind(&now)
                .bind(work_id)
                .execute(db.sqlite_pool().ok_or_else(pool_err)?)
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(at_uri)
                .bind(&now)
                .bind(work_id)
                .execute(db.postgres_pool().ok_or_else(pool_err)?)
                .await?;
        }
    }
    Ok(())
}

/// Where this work was published, if anywhere. `None` is the ordinary case: most works have never been bridged.
pub async fn at_uri_for(db: &Database, work_id: &str) -> Result<Option<String>, sqlx::Error> {
    let sql = db.sql(
        "SELECT at_uri FROM works WHERE id = ? AND at_uri IS NOT NULL",
        "SELECT at_uri FROM works WHERE id = $1::uuid AND at_uri IS NOT NULL",
    );
    let uri = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(work_id)
                .fetch_optional(db.sqlite_pool().ok_or_else(pool_err)?)
                .await?
                .map(|r| r.get::<String, _>("at_uri"))
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(work_id)
                .fetch_optional(db.postgres_pool().ok_or_else(pool_err)?)
                .await?
                .map(|r| r.get::<String, _>("at_uri"))
        }
    };
    Ok(uri)
}

/// Forget a record that no longer exists. Called only after the PDS has confirmed the delete, so the column never claims a
/// record that is still live — the failure mode that makes a purge impossible to run twice.
pub async fn clear_at_uri(db: &Database, work_id: &str) -> Result<(), sqlx::Error> {
    let sql = db.sql(
        "UPDATE works SET at_uri = NULL, updated_at = ? WHERE id = ?",
        "UPDATE works SET at_uri = NULL, updated_at = $1 WHERE id = $2::uuid",
    );
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql).bind(&now).bind(work_id).execute(db.sqlite_pool().ok_or_else(pool_err)?).await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql).bind(&now).bind(work_id).execute(db.postgres_pool().ok_or_else(pool_err)?).await?;
        }
    }
    Ok(())
}

/// The `at://` URIs already published, for the purge in `docs/FEDERATION.md` §9 and for the gate that counts them.
pub async fn published_uris(db: &Database, limit: i64) -> Result<Vec<(String, String)>, sqlx::Error> {
    let sql = db.sql(
        "SELECT id, at_uri FROM works WHERE at_uri IS NOT NULL ORDER BY updated_at DESC LIMIT ?",
        "SELECT id, at_uri FROM works WHERE at_uri IS NOT NULL ORDER BY updated_at DESC LIMIT $1",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(limit)
                .fetch_all(db.sqlite_pool().ok_or_else(pool_err)?)
                .await?
                .into_iter()
                .map(|r| (r.get::<String, _>("id"), r.get::<String, _>("at_uri")))
                .collect::<Vec<_>>()
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(limit)
                .fetch_all(db.postgres_pool().ok_or_else(pool_err)?)
                .await?
                .into_iter()
                .map(|r| (r.get::<String, _>("id"), r.get::<String, _>("at_uri")))
                .collect::<Vec<_>>()
        }
    };
    Ok(rows)
}

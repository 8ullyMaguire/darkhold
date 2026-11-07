//! M69-05 — the record of which peers hold a copy of this instance's work.
//!
//! # The store does not decide
//!
//! Whether a peer *may* hold a copy is `darkhold_domain::mirroring::eligible`'s question, answered against documents the
//! domain crate can compare without a database. This module only records the answer, which is why `similarity_bp` is a
//! parameter rather than something derived here: the decision and the record must be the same number, and the way to
//! guarantee that is to pass it.
//!
//! # One row per (work, peer)
//!
//! Re-mirroring to the same peer replaces the earlier row rather than adding a second. Two rows for one fact make "is this
//! work mirrored?" a sum, and the second row is always the one nobody reads. A *retired* row that is recorded again is
//! re-armed, with `retired_at` cleared — the mirror exists again, and a page that kept saying "withdrawn" after it was
//! restored would be wrong in the direction that looks like a bug.

use anyhow::Result;
use sqlx::Row as _;

use crate::{Backend, Database};

/// One mirror of one work.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkMirror {
    /// Its id.
    pub id: String,
    /// The work it mirrors.
    pub work_id: String,
    /// The peer holding the copy, by its canonical base URL.
    pub peer_instance: String,
    /// Where a reader finds the copy there. Empty when the peer gave no address.
    pub peer_url: String,
    /// Taste similarity at the moment of the decision, in basis points.
    pub similarity_bp: i64,
    /// `active`, `lost` or `withdrawn`.
    pub state: String,
    /// When the copy was made.
    pub mirrored_at: String,
    /// When the peer was last seen serving it.
    pub last_seen_at: Option<String>,
    /// When it stopped being active.
    pub retired_at: Option<String>,
    /// Why it stopped.
    pub retire_reason: Option<String>,
}

/// Record that a peer holds a copy of a work.
///
/// Returns the row as stored, so the caller reports the similarity the record carries rather than the one it hoped to store.
pub async fn record(
    db: &Database,
    work_id: &str,
    peer_instance: &str,
    peer_url: &str,
    similarity_bp: i64,
    now: &str,
) -> Result<WorkMirror> {
    if !(0..=10_000).contains(&similarity_bp) {
        return Err(anyhow::anyhow!(
            "similarity must be basis points in 0..=10000, got {similarity_bp}"
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO work_mirrors
                    (id, work_id, peer_instance, peer_url, similarity_bp, state, mirrored_at)
                 VALUES (?, ?, ?, ?, ?, 'active', ?)
                 ON CONFLICT(work_id, peer_instance) DO UPDATE SET
                    peer_url = excluded.peer_url,
                    similarity_bp = excluded.similarity_bp,
                    state = 'active',
                    mirrored_at = excluded.mirrored_at,
                    last_seen_at = excluded.mirrored_at,
                    retired_at = NULL,
                    retire_reason = NULL",
            )
            .bind(&id)
            .bind(work_id)
            .bind(peer_instance)
            .bind(peer_url)
            .bind(similarity_bp)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO work_mirrors
                    (id, work_id, peer_instance, peer_url, similarity_bp, state, mirrored_at)
                 VALUES ($1, $2, $3, $4, $5::bigint, 'active', $6)
                 ON CONFLICT(work_id, peer_instance) DO UPDATE SET
                    peer_url = EXCLUDED.peer_url,
                    similarity_bp = EXCLUDED.similarity_bp,
                    state = 'active',
                    mirrored_at = EXCLUDED.mirrored_at,
                    last_seen_at = EXCLUDED.mirrored_at,
                    retired_at = NULL,
                    retire_reason = NULL",
            )
            .bind(&id)
            .bind(work_id)
            .bind(peer_instance)
            .bind(peer_url)
            .bind(similarity_bp)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }

    find(db, work_id, peer_instance)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the mirror was written and cannot be read back"))
}

fn row_to_mirror_sqlite(row: &sqlx::sqlite::SqliteRow) -> WorkMirror {
    WorkMirror {
        id: row.get("id"),
        work_id: row.get("work_id"),
        peer_instance: row.get("peer_instance"),
        peer_url: row.get("peer_url"),
        similarity_bp: row.get("similarity_bp"),
        state: row.get("state"),
        mirrored_at: row.get("mirrored_at"),
        last_seen_at: row.get("last_seen_at"),
        retired_at: row.get("retired_at"),
        retire_reason: row.get("retire_reason"),
    }
}

fn row_to_mirror_postgres(row: &sqlx::postgres::PgRow) -> WorkMirror {
    WorkMirror {
        id: row.get("id"),
        work_id: row.get("work_id"),
        peer_instance: row.get("peer_instance"),
        peer_url: row.get("peer_url"),
        similarity_bp: row.get("similarity_bp"),
        state: row.get("state"),
        mirrored_at: row.get("mirrored_at"),
        last_seen_at: row.get("last_seen_at"),
        retired_at: row.get("retired_at"),
        retire_reason: row.get("retire_reason"),
    }
}

/// One work's mirror to one peer, in any state.
pub async fn find(db: &Database, work_id: &str, peer_instance: &str) -> Result<Option<WorkMirror>> {
    const COLUMNS: &str =
        "id, work_id, peer_instance, peer_url, CAST(similarity_bp AS BIGINT) AS similarity_bp, \
                           state, mirrored_at, last_seen_at, retired_at, retire_reason";
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&format!(
                "SELECT {} FROM work_mirrors WHERE work_id = ? AND peer_instance = ?",
                COLUMNS.replace(
                    "CAST(similarity_bp AS BIGINT) AS similarity_bp",
                    "similarity_bp"
                )
            ))
            .bind(work_id)
            .bind(peer_instance)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.as_ref().map(row_to_mirror_sqlite))
        }
        Backend::Postgres => {
            let row = sqlx::query(&format!(
                "SELECT {} FROM work_mirrors WHERE work_id = $1 AND peer_instance = $2",
                COLUMNS
            ))
            .bind(work_id)
            .bind(peer_instance)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.as_ref().map(row_to_mirror_postgres))
        }
    }
}

/// Every mirror of one work, active first, then newest.
///
/// Retired rows are included on purpose: the attribution line has to be able to say "this copy was withdrawn", which it can
/// only do if the row is still there.
pub async fn for_work(db: &Database, work_id: &str) -> Result<Vec<WorkMirror>> {
    const COLUMNS: &str =
        "id, work_id, peer_instance, peer_url, CAST(similarity_bp AS BIGINT) AS similarity_bp, \
                           state, mirrored_at, last_seen_at, retired_at, retire_reason";
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&format!(
                "SELECT {} FROM work_mirrors WHERE work_id = ? \
                 ORDER BY CASE state WHEN 'active' THEN 0 WHEN 'lost' THEN 1 ELSE 2 END, mirrored_at DESC",
                COLUMNS.replace("CAST(similarity_bp AS BIGINT) AS similarity_bp", "similarity_bp")
            ))
            .bind(work_id)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows.iter().map(row_to_mirror_sqlite).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(&format!(
                "SELECT {} FROM work_mirrors WHERE work_id = $1 \
                 ORDER BY CASE state WHEN 'active' THEN 0 WHEN 'lost' THEN 1 ELSE 2 END, mirrored_at DESC",
                COLUMNS
            ))
            .bind(work_id)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows.iter().map(row_to_mirror_postgres).collect())
        }
    }
}

/// How many active mirrors one work has.
pub async fn active_count(db: &Database, work_id: &str) -> Result<i64> {
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM work_mirrors WHERE work_id = ? AND state = 'active'",
            )
            .bind(work_id)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM work_mirrors WHERE work_id = $1 AND state = 'active'",
            )
            .bind(work_id)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await?
        }
    };
    Ok(count)
}

/// Retire a mirror: it exists but no longer serves. Returns whether anything changed.
pub async fn retire(db: &Database, id: &str, state: &str, reason: &str, now: &str) -> Result<bool> {
    if !matches!(state, "lost" | "withdrawn") {
        return Err(anyhow::anyhow!(
            "a mirror retires as 'lost' or 'withdrawn', not '{state}'"
        ));
    }
    if reason.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "a retirement needs a reason: the attribution line has to be able to explain itself"
        ));
    }
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "UPDATE work_mirrors SET state = ?, retired_at = ?, retire_reason = ?
                  WHERE id = ? AND state = 'active'",
        )
        .bind(state)
        .bind(now)
        .bind(reason)
        .bind(id)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await?
        .rows_affected(),
        Backend::Postgres => sqlx::query(
            "UPDATE work_mirrors SET state = $1, retired_at = $2, retire_reason = $3
                  WHERE id = $4 AND state = 'active'",
        )
        .bind(state)
        .bind(now)
        .bind(reason)
        .bind(id)
        .execute(db.postgres_pool().expect("postgres"))
        .await?
        .rows_affected(),
    };
    Ok(affected > 0)
}

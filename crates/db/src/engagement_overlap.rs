//! M64-13 — engagement overlap between two works.
//!
//! The cluster is the set of distinct readers with a real session on a work in the window. This module returns only the size of
//! the intersection; no reader id leaves the database. The caller applies the k-anonymity floor to the count before publishing it.

use sqlx::Row;

use crate::{Backend, Database, Result};

#[derive(Debug, Clone)]
pub struct Options {
    /// RFC 3339 lower bound.
    pub since: String,
    /// RFC 3339 upper bound.
    pub until: String,
}

/// Number of readers with a qualifying session on both works.
pub async fn reader_overlap(
    db: &Database,
    left: &str,
    right: &str,
    options: &Options,
) -> Result<i64> {
    let sqlite = "SELECT COUNT(*) AS n FROM ( \
                    SELECT DISTINCT account_id FROM reading_sessions \
                     WHERE work_id = ? AND seconds >= 30 AND ended_at >= ? AND ended_at <= ? \
                    INTERSECT \
                    SELECT DISTINCT account_id FROM reading_sessions \
                     WHERE work_id = ? AND seconds >= 30 AND ended_at >= ? AND ended_at <= ? \
                  )";
    let postgres = "SELECT COUNT(*)::bigint AS n FROM ( \
                      SELECT DISTINCT account_id FROM reading_sessions \
                       WHERE work_id = $1::uuid AND seconds >= 30 AND ended_at >= $2 AND ended_at <= $3 \
                      INTERSECT \
                      SELECT DISTINCT account_id FROM reading_sessions \
                       WHERE work_id = $4::uuid AND seconds >= 30 AND ended_at >= $5 AND ended_at <= $6 \
                    ) AS overlap";
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(sqlite)
                .bind(left)
                .bind(&options.since)
                .bind(&options.until)
                .bind(right)
                .bind(&options.since)
                .bind(&options.until)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?;
            row.get::<i64, _>("n")
        }
        Backend::Postgres => {
            let row = sqlx::query(postgres)
                .bind(left)
                .bind(&options.since)
                .bind(&options.until)
                .bind(right)
                .bind(&options.since)
                .bind(&options.until)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?;
            row.get::<i64, _>("n")
        }
    };
    Ok(count)
}

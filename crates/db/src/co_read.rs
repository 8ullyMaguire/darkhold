//! M64-12 — co-read transitions from signed-in reading sessions.
//!
//! The only writer of `reading_sessions` is the browser-only `record_reading_session` door, which requires a
//! `RequirePseud` session. Token-driven bot traffic therefore cannot enter this table; a future bot writer would require an
//! explicit exclusion here. A short session is treated as a single click, not an engagement.

use sqlx::Row;

use crate::{Backend, Database, Result};

#[derive(Debug, Clone)]
pub struct Options {
    /// RFC 3339 lower bound, normalised by the caller.
    pub since: String,
    /// RFC 3339 upper bound.
    pub until: String,
    /// Minimum distinct readers for an edge to be reported.
    pub min_readers: i64,
    /// Maximum rows.
    pub limit: i64,
}

/// Works read after `work_id` by readers who spent real time on both legs.
///
/// Returns `(work_id, distinct_readers)` ordered by readers descending. A below-floor edge is absent, not zero, so an empty
/// answer means "unmeasured" rather than "nobody enjoyed this".
pub async fn also_enjoyed(
    db: &Database,
    work_id: &str,
    options: &Options,
) -> Result<Vec<(String, i64)>> {
    let min = options.min_readers.max(1);
    let limit = options.limit.clamp(1, 100);
    let sqlite = "SELECT rs2.work_id AS work_id, COUNT(DISTINCT rs2.account_id) AS readers \
                    FROM reading_sessions rs1 \
                    JOIN reading_sessions rs2 ON rs2.account_id = rs1.account_id \
                   WHERE rs1.work_id = ? AND rs2.work_id <> rs1.work_id \
                     AND rs1.seconds >= 30 AND rs2.seconds >= 30 \
                     AND rs1.ended_at >= ? AND rs2.started_at > rs1.ended_at AND rs2.started_at <= ? \
                   GROUP BY rs2.work_id HAVING readers >= ? \
                   ORDER BY readers DESC, work_id ASC LIMIT ?";
    let postgres = "SELECT rs2.work_id::text AS work_id, COUNT(DISTINCT rs2.account_id)::bigint AS readers \
                      FROM reading_sessions rs1 \
                      JOIN reading_sessions rs2 ON rs2.account_id = rs1.account_id \
                     WHERE rs1.work_id = $1::uuid AND rs2.work_id <> rs1.work_id \
                       AND rs1.seconds >= 30 AND rs2.seconds >= 30 \
                       AND rs1.ended_at >= $2 AND rs2.started_at > rs1.ended_at AND rs2.started_at <= $3 \
                     GROUP BY rs2.work_id HAVING COUNT(DISTINCT rs2.account_id) >= $4 \
                     ORDER BY readers DESC, work_id ASC LIMIT $5";
    let rows: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => {
            let rows: Vec<sqlx::sqlite::SqliteRow> = sqlx::query(sqlite)
                .bind(work_id)
                .bind(&options.since)
                .bind(&options.until)
                .bind(min)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?;
            rows.into_iter()
                .map(|row| {
                    (
                        row.get::<String, _>("work_id"),
                        row.get::<i64, _>("readers"),
                    )
                })
                .collect()
        }
        Backend::Postgres => {
            let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(postgres)
                .bind(work_id)
                .bind(&options.since)
                .bind(&options.until)
                .bind(min)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?;
            rows.into_iter()
                .map(|row| {
                    (
                        row.get::<String, _>("work_id"),
                        row.get::<i64, _>("readers"),
                    )
                })
                .collect()
        }
    };
    Ok(rows)
}

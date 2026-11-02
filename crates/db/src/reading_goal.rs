//! §9.7.9 — storage for reading goals (M47-13).
//!
//! One row per pseud, and the absence of a row is the absence of a goal. Progress is derived from
//! `reading_progress`, `chapter_revisions` and `reading_status` on every read rather than accumulated
//! in a counter, so a reader who clears their history (§9.6) sees their progress go with it.

use anyhow::Result;
use time::OffsetDateTime;

use darkhold_domain::reading_goal::{Goal, Metric, Period, Progress};

use crate::{Backend, Database};

/// Record that a reader has reached a chapter.
///
/// `reading_progress` cannot answer "which chapters has this reader read" — it is unique per
/// (account, pseud, subject), so it keeps the chapter the reader is *on*. This is the record that
/// makes the `chapters` and `words` goals countable, and `reading::save_progress` writes it inside
/// the same transaction as the progress upsert.
///
/// Upsert moves `last_read_at` forward, so re-reading a chapter in a later window counts it in that
/// window without ever counting it twice in one.
pub async fn record_unit_read(
    db: &Database,
    pseud_id: &str,
    work_id: &str,
    unit_id: &str,
    now: &str,
) -> Result<()> {
    let sql = db.sql(
        "INSERT INTO reading_unit_read (id, pseud_id, work_id, unit_id, first_read_at, last_read_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (pseud_id, unit_id) DO UPDATE SET last_read_at = excluded.last_read_at",
        "INSERT INTO reading_unit_read (id, pseud_id, work_id, unit_id, first_read_at, last_read_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (pseud_id, unit_id) DO UPDATE SET last_read_at = excluded.last_read_at",
    );
    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(pseud_id)
                .bind(work_id)
                .bind(unit_id)
                .bind(now)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(pseud_id)
                .bind(work_id)
                .bind(unit_id)
                .bind(now)
                .bind(now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// The reader's goal, if they have set one.
pub async fn get(db: &Database, pseud_id: &str) -> Result<Option<Goal>> {
    let sql = db.sql(
        "SELECT period, metric, target FROM reading_goal WHERE pseud_id = ?",
        "SELECT period, metric, target FROM reading_goal WHERE pseud_id = ?",
    );
    let row: Option<(String, String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(pseud_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(pseud_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let Some((period, metric, target)) = row else {
        return Ok(None);
    };
    // A row that fails to parse is not a goal this build can render; treat it as absent rather than
    // failing the reader's dashboard over a value a repair script wrote.
    Ok(Period::parse(&period)
        .zip(Metric::parse(&metric))
        .map(|(period, metric)| Goal {
            period,
            metric,
            target,
        }))
}

/// Write a goal, replacing any existing one.
pub async fn set(db: &Database, pseud_id: &str, goal: Goal, now: &str) -> Result<()> {
    let sql = db.sql(
        "INSERT INTO reading_goal (pseud_id, period, metric, target, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (pseud_id) DO UPDATE SET
              period = excluded.period,
              metric = excluded.metric,
              target = excluded.target,
              updated_at = excluded.updated_at",
        "INSERT INTO reading_goal (pseud_id, period, metric, target, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (pseud_id) DO UPDATE SET
              period = excluded.period,
              metric = excluded.metric,
              target = excluded.target,
              updated_at = excluded.updated_at",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(pseud_id)
                .bind(goal.period.as_str())
                .bind(goal.metric.as_str())
                .bind(goal.target)
                .bind(now)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(pseud_id)
                .bind(goal.period.as_str())
                .bind(goal.metric.as_str())
                .bind(goal.target)
                .bind(now)
                .bind(now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Remove the goal. Returns whether there was one.
pub async fn clear(db: &Database, pseud_id: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM reading_goal WHERE pseud_id = ?",
        "DELETE FROM reading_goal WHERE pseud_id = ?",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(pseud_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(pseud_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// A goal measured over the window containing `now`.
pub async fn progress(
    db: &Database,
    goal: Goal,
    pseud_id: &str,
    account_id: &str,
    now: OffsetDateTime,
) -> Result<Progress> {
    let (start, end) = darkhold_domain::reading_goal::window(goal.period, now);
    // `format_rfc3339` is the schema's canonical form (M63-31): second precision, `Z` suffix.
    let start = crate::identity::format_rfc3339(start);
    let end = crate::identity::format_rfc3339(end);
    let current = match goal.metric {
        Metric::Chapters => chapters_read(db, pseud_id, &start, &end).await?,
        Metric::Words => words_read(db, pseud_id, &start, &end).await?,
        Metric::Works => works_finished(db, account_id, &start, &end).await?,
    };
    Ok(Progress {
        goal,
        current,
        window_start: start,
        window_end: end,
    })
}

async fn scalar(db: &Database, sql: &str, binds: &[&str]) -> Result<i64> {
    match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_scalar(sql);
            for bind in binds {
                query = query.bind(*bind);
            }
            Ok(query
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
                .unwrap_or(0))
        }
        Backend::Postgres => {
            let mut query = sqlx::query_scalar(sql);
            for bind in binds {
                query = query.bind(*bind);
            }
            Ok(query
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
                .unwrap_or(0))
        }
    }
}

/// Distinct chapters read in the window.
///
/// From `reading_unit_read`, not `reading_progress`: the latter is unique per (account, pseud,
/// subject), so it holds the chapter the reader is *on* and cannot count the chapters they have
/// read. And the column is `unit_id`, not `chapter_id` — the schema renamed `chapters` → `units`
/// while the reader-facing word stayed "chapter", which is how a query written from the 0004
/// migration alone compiles and then fails at runtime with "no such table: chapters".
///
/// Distinct because `reading_progress` is keyed per subject: re-opening the same chapter five times
/// is one chapter read, which is also how §9.7.2 pays its reading credit ("once per unique chapter
/// per day").
async fn chapters_read(db: &Database, pseud_id: &str, start: &str, end: &str) -> Result<i64> {
    let sql = db.sql(
        "SELECT COUNT(*) FROM reading_unit_read
          WHERE pseud_id = ? AND last_read_at >= ? AND last_read_at < ?",
        "SELECT COUNT(*)::bigint FROM reading_unit_read
          WHERE pseud_id = ? AND last_read_at >= ? AND last_read_at < ?",
    );
    scalar(db, &sql, &[pseud_id, start, end]).await
}

/// Words across those distinct chapters, from each chapter's CURRENT revision.
///
/// The current revision rather than the revision that was read: `reading_progress.content_revision`
/// records what the reader saw, but joining on it would under-count a chapter the author edited
/// after the reader left, and over-count nothing a reader cares about. Documented as an estimate —
/// §9.6 is explicit that estimates are labelled, never presented as measurement.
async fn words_read(db: &Database, pseud_id: &str, start: &str, end: &str) -> Result<i64> {
    let sql = db.sql(
        "SELECT COALESCE(SUM(r.word_count), 0)
           FROM reading_unit_read read
           JOIN units u ON u.id = read.unit_id
           JOIN revisions r ON r.id = u.current_revision_id
          WHERE read.pseud_id = ? AND read.last_read_at >= ? AND read.last_read_at < ?",
        "SELECT COALESCE(SUM(r.word_count), 0)::bigint
           FROM reading_unit_read read
           -- `units.id` and `revisions.id` are UUID on this engine and `reading_unit_read.unit_id`
           -- is TEXT, so the cast belongs on the column; `revisions.id =
           -- units.current_revision_id` is UUID-to-UUID and must NOT be cast.
           JOIN units u ON u.id::text = read.unit_id
           JOIN revisions r ON r.id = u.current_revision_id
          WHERE read.pseud_id = ? AND read.last_read_at >= ? AND read.last_read_at < ?",
    );
    scalar(db, &sql, &[pseud_id, start, end]).await
}

/// Works marked finished in the window, from the ACCOUNT's shelf.
async fn works_finished(db: &Database, account_id: &str, start: &str, end: &str) -> Result<i64> {
    let sql = db.sql(
        "SELECT COUNT(*) FROM reading_status
          WHERE account_id = ? AND status = 'finished'
            AND finished_at IS NOT NULL AND finished_at >= ? AND finished_at < ?",
        "SELECT COUNT(*)::bigint FROM reading_status
          WHERE account_id::text = ? AND status = 'finished'
            AND finished_at IS NOT NULL AND finished_at >= ? AND finished_at < ?",
    );
    scalar(db, &sql, &[account_id, start, end]).await
}

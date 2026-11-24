//! Store side of the automatic WIP/abandoned lifecycle (M45-76).
//!
//! The pure decision lives in `darkhold_domain::work_lifecycle`; this module
//! supplies the clocks and the current state. Keeping the clock selection here
//! means an imported work's `publication_events.publish` — an import artifact —
//! cannot be mistaken for the author's last update.

use crate::Database;

/// One row the sweep is allowed to judge.
#[derive(Debug)]
pub struct Candidate {
    pub work_id: String,
    pub completion: String,
    pub completion_source: String,
    pub completion_marked_at: Option<String>,
    pub published_at: Option<String>,
    pub last_event: Option<String>,
    pub last_source: Option<String>,
    pub source_says_complete: bool,
}

const CANDIDATES_SQLITE: &str = "
WITH ev AS (
  SELECT work_id, MAX(occurred_at) AS last_event
    FROM publication_events
   WHERE action IN ('publish', 'update', 'restore_revision')
   GROUP BY work_id
),
src AS (
  SELECT work_id, MAX(source_updated_at) AS last_source
    FROM library_items
   WHERE source_updated_at IS NOT NULL
   GROUP BY work_id
),
has_lib AS (
  SELECT DISTINCT work_id FROM library_items
),
done AS (
  SELECT DISTINCT work_id FROM library_items WHERE status = 'complete'
)
SELECT w.id,
       w.completion,
       w.completion_source,
       w.completion_marked_at,
       w.published_at,
       CASE WHEN hl.work_id IS NOT NULL THEN NULL ELSE ev.last_event END AS last_event,
       src.last_source,
       CAST(CASE WHEN done.work_id IS NULL THEN 0 ELSE 1 END AS INTEGER) AS source_says_complete
  FROM works w
  LEFT JOIN ev      ON ev.work_id = w.id
  LEFT JOIN src     ON src.work_id = w.id
  LEFT JOIN has_lib hl ON hl.work_id = w.id
  LEFT JOIN done    ON done.work_id = w.id
 WHERE w.lifecycle = 'published'
   AND w.deleted_at IS NULL
   AND w.completion IN ('in_progress', 'hiatus', 'abandoned')
   AND w.id > COALESCE(?, '')
 ORDER BY w.id
 LIMIT ?";

const CANDIDATES_POSTGRES: &str = "
WITH ev AS (
  SELECT work_id, MAX(occurred_at) AS last_event
    FROM publication_events
   WHERE action IN ('publish', 'update', 'restore_revision')
   GROUP BY work_id
),
src AS (
  SELECT work_id, MAX(source_updated_at) AS last_source
    FROM library_items
   WHERE source_updated_at IS NOT NULL
   GROUP BY work_id
),
has_lib AS (
  SELECT DISTINCT work_id FROM library_items
),
done AS (
  SELECT DISTINCT work_id FROM library_items WHERE status = 'complete'
)
SELECT w.id::text,
       w.completion,
       w.completion_source,
       w.completion_marked_at,
       w.published_at,
       CASE WHEN hl.work_id IS NOT NULL THEN NULL ELSE ev.last_event END AS last_event,
       src.last_source,
       CAST(CASE WHEN done.work_id IS NULL THEN 0 ELSE 1 END AS BIGINT) AS source_says_complete
  FROM works w
  LEFT JOIN ev      ON ev.work_id = w.id
  LEFT JOIN src     ON src.work_id = w.id
  LEFT JOIN has_lib hl ON hl.work_id = w.id
  LEFT JOIN done    ON done.work_id = w.id
 WHERE w.lifecycle = 'published'
   AND w.deleted_at IS NULL
   AND w.completion IN ('in_progress', 'hiatus', 'abandoned')
   AND w.id > COALESCE(?::uuid, '00000000-0000-0000-0000-000000000000'::uuid)
 ORDER BY w.id
 LIMIT ?";

/// The maximum of each clock is taken separately and combined in Rust.
/// `published_at` is naive, `occurred_at` has nanoseconds, and a SQL
/// `GREATEST` would order them by shape rather than by instant.
pub async fn candidates(
    db: &Database,
    after: Option<&str>,
    limit: i64,
) -> Result<Vec<Candidate>, sqlx::Error> {
    let sql = db.sql(CANDIDATES_SQLITE, CANDIDATES_POSTGRES);
    let rows: Vec<(
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
    )> = match db.backend() {
        crate::Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(after)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        crate::Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(after)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(
                work_id,
                completion,
                completion_source,
                completion_marked_at,
                published_at,
                last_event,
                last_source,
                source_says_complete,
            )| Candidate {
                work_id,
                completion,
                completion_source,
                completion_marked_at,
                published_at,
                last_event,
                last_source,
                source_says_complete: source_says_complete != 0,
            },
        )
        .collect())
}

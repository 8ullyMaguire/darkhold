//! M45-38 — the store half of author-note link monitoring (§32.7, gaps review D3).
//!
//! `darkhold_domain::note_links` decides what counts as a link; this module stores the decision and
//! gives the checker the same three verbs it has for availability links: what is due, what happened,
//! and what a work is currently pointing at.
//!
//! **The row is written with the revision, not when someone reads the note.** Extraction at read time
//! would re-scan every revision on every request, and — worse — a link discovered on read could never
//! have been checked, so the surface would show "pending" forever and be right to.
//!
//! **Re-recording a revision is idempotent.** `UNIQUE (revision_id, url)` with `ON CONFLICT DO
//! NOTHING` means a retried save, or a note edited to say the same thing, does not reset a link's
//! health. A link that has been dead for a month must not look fresh because the author fixed a typo
//! in the sentence around it.

use anyhow::Result;

use darkhold_domain::media_resilience::LinkStatus;
use darkhold_domain::note_links;

use crate::{Backend, Database};

/// One monitored note link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteLink {
    /// Identifier.
    pub id: String,
    /// The work whose note carries it.
    pub work_id: String,
    /// The unit (chapter) whose revision carries it.
    pub unit_id: String,
    /// The revision whose note carried it — the claim, not the work.
    pub revision_id: String,
    /// The URL as extracted.
    pub url: String,
    /// Its host, without scheme or port.
    pub host: String,
    /// Who publishes it, in the §32.7 vocabulary.
    pub provider: String,
    /// Healthy, degraded, dead, quarantined or pending.
    pub status: String,
    /// How many checks in a row have failed.
    pub consecutive_failures: i64,
    /// When it was last checked, RFC 3339, `None` for never.
    pub last_checked_at: Option<String>,
    /// When it last answered, RFC 3339.
    pub last_healthy_at: Option<String>,
    /// Why the last check failed, when it did.
    pub failure_reason: Option<String>,
}

impl NoteLink {
    /// Whether this link is currently believed broken.
    #[must_use]
    pub fn is_dead(&self) -> bool {
        self.status == LinkStatus::Dead.as_str()
    }
}

/// Record the links a revision's note carries. Returns how many rows this call created.
///
/// Zero is the common answer: most notes have no links, and a note whose links were already recorded
/// (a retried save) creates nothing. The count is returned rather than a bool because the caller logs
/// it, and "6 links registered" is a fact a bool cannot carry.
pub async fn record_for_revision(
    db: &Database,
    work_id: &str,
    unit_id: &str,
    revision_id: &str,
    note: Option<&str>,
) -> Result<usize> {
    let links = note_links::links_in(note);
    if links.is_empty() {
        return Ok(0);
    }
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO note_links
             (id, work_id, unit_id, revision_id, url, host, provider, status,
              consecutive_failures, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'pending_verification', 0, ?, ?)
         ON CONFLICT (revision_id, url) DO NOTHING",
        "INSERT INTO note_links
             (id, work_id, unit_id, revision_id, url, host, provider, status,
              consecutive_failures, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'pending_verification', 0, ?, ?)
         ON CONFLICT (revision_id, url) DO NOTHING",
    );
    let mut created = 0usize;
    for url in links {
        // A link with no host cannot be checked; `links_in` already refuses one, and this is the
        // belt to that brace rather than a second rule.
        let Some(host) = note_links::host_of(&url) else {
            continue;
        };
        let provider = note_links::provider_of(host).as_str();
        let affected = match db.backend() {
            Backend::Sqlite => sqlx::query(&sql)
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(work_id)
                .bind(unit_id)
                .bind(revision_id)
                .bind(&url)
                .bind(host)
                .bind(provider)
                .bind(&now)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?
                .rows_affected(),
            Backend::Postgres => sqlx::query(&sql)
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(work_id)
                .bind(unit_id)
                .bind(revision_id)
                .bind(&url)
                .bind(host)
                .bind(provider)
                .bind(&now)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?
                .rows_affected(),
        };
        created += usize::try_from(affected).unwrap_or(0);
    }
    Ok(created)
}

/// One link by id, for the checker.
pub async fn find_by_id(db: &Database, id: &str) -> Result<Option<NoteLink>> {
    let sql = db.sql(
        "SELECT id, work_id, unit_id, revision_id, url, host, provider, status,
                consecutive_failures, last_checked_at, last_healthy_at, failure_reason
           FROM note_links WHERE id = ?",
        "SELECT id, work_id, unit_id, revision_id, url, host, provider, status,
                consecutive_failures, last_checked_at, last_healthy_at, failure_reason
           FROM note_links WHERE id = $1",
    );
    let row = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, NoteLinkRow>(&sql)
                .bind(id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, NoteLinkRow>(&sql)
                .bind(id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(NoteLinkRow::into_link))
}

/// Every link a work's notes carry, dead ones last so a reader sees the healthy ones first.
pub async fn list_for_work(db: &Database, work_id: &str) -> Result<Vec<NoteLink>> {
    let sql = db.sql(
        "SELECT id, work_id, unit_id, revision_id, url, host, provider, status,
                consecutive_failures, last_checked_at, last_healthy_at, failure_reason
           FROM note_links
          WHERE work_id = ?
          ORDER BY CASE status WHEN 'dead' THEN 1 ELSE 0 END, created_at ASC",
        "SELECT id, work_id, unit_id, revision_id, url, host, provider, status,
                consecutive_failures, last_checked_at, last_healthy_at, failure_reason
           FROM note_links
          WHERE work_id = $1
          ORDER BY CASE status WHEN 'dead' THEN 1 ELSE 0 END, created_at ASC",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, NoteLinkRow>(&sql)
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, NoteLinkRow>(&sql)
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(NoteLinkRow::into_link).collect())
}

/// Every link on the live revision of each of a work's units, which is what a reader is shown.
///
/// A superseded revision's links are still monitored — the row is not deleted — but they are not what
/// the work says today, and listing them beside the current ones would report a link the author
/// already fixed as if it were still there.
pub async fn list_for_live_revisions(db: &Database, work_id: &str) -> Result<Vec<NoteLink>> {
    let sql = db.sql(
        "SELECT nl.id, nl.work_id, nl.unit_id, nl.revision_id, nl.url, nl.host, nl.provider,
                nl.status, nl.consecutive_failures, nl.last_checked_at, nl.last_healthy_at,
                nl.failure_reason
           FROM note_links nl
          WHERE nl.work_id = ?
            AND nl.revision_id IN (SELECT current_revision_id FROM units WHERE work_id = ?)
          ORDER BY CASE nl.status WHEN 'dead' THEN 1 ELSE 0 END, nl.created_at ASC",
        "SELECT nl.id, nl.work_id, nl.unit_id, nl.revision_id, nl.url, nl.host, nl.provider,
                nl.status, nl.consecutive_failures, nl.last_checked_at,
                nl.last_healthy_at, nl.failure_reason
           FROM note_links nl
          WHERE nl.work_id = $1
            AND nl.revision_id IN (SELECT current_revision_id::text FROM units WHERE work_id::text = $1)
          ORDER BY CASE nl.status WHEN 'dead' THEN 1 ELSE 0 END, nl.created_at ASC",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            // The SQLite text names the work twice (`?`) and PostgreSQL once (`$1`), so the bind
            // count differs by dialect. One bind here fails with "column index out of bounds", which
            // is what this comment exists to stop the next reader from re-introducing.
            sqlx::query_as::<_, NoteLinkRow>(&sql)
                .bind(work_id)
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, NoteLinkRow>(&sql)
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(NoteLinkRow::into_link).collect())
}

/// The links the checker should look at next: never-checked first, then oldest.
pub async fn find_needing_check(db: &Database, limit: i64) -> Result<Vec<NoteLink>> {
    let sql = db.sql(
        "SELECT id, work_id, unit_id, revision_id, url, host, provider, status,
                consecutive_failures, last_checked_at, last_healthy_at, failure_reason
           FROM note_links
          ORDER BY last_checked_at IS NOT NULL, last_checked_at ASC
          LIMIT ?",
        "SELECT id, work_id, unit_id, revision_id, url, host, provider, status,
                consecutive_failures, last_checked_at, last_healthy_at, failure_reason
           FROM note_links
          ORDER BY last_checked_at IS NOT NULL, last_checked_at ASC
          LIMIT $1",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, NoteLinkRow>(&sql)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, NoteLinkRow>(&sql)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(NoteLinkRow::into_link).collect())
}

/// Record the result of one check.
///
/// `consecutive_failures` is passed in rather than incremented here so that a caller replaying a
/// batch of checks cannot double-count one result, and so the transition rule lives in one place:
/// three failures is dead (§32.7's threshold for availability links, reused rather than re-chosen).
pub async fn mark_checked(
    db: &Database,
    id: &str,
    status: LinkStatus,
    consecutive_failures: i64,
    failure_reason: Option<&str>,
) -> Result<()> {
    let now = crate::identity::now_rfc3339();
    // A healthy check stamps `last_healthy_at`; a failing one does not, and the column keeps the last
    // time it answered rather than being cleared — "dead since Tuesday, last answered in March" is
    // the sentence a reader needs.
    let sql = db.sql(
        "UPDATE note_links
            SET status = ?, consecutive_failures = ?, failure_reason = ?,
                last_checked_at = ?,
                last_healthy_at = CASE WHEN ? = 'healthy' THEN ? ELSE last_healthy_at END,
                updated_at = ?
          WHERE id = ?",
        "UPDATE note_links
            SET status = $1, consecutive_failures = $2, failure_reason = $3,
                last_checked_at = $4,
                last_healthy_at = CASE WHEN $5 = 'healthy' THEN $6 ELSE last_healthy_at END,
                updated_at = $7
          WHERE id = $8",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(status.as_str())
                .bind(consecutive_failures)
                .bind(failure_reason)
                .bind(&now)
                .bind(status.as_str())
                .bind(&now)
                .bind(&now)
                .bind(id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(status.as_str())
                .bind(consecutive_failures)
                .bind(failure_reason)
                .bind(&now)
                .bind(status.as_str())
                .bind(&now)
                .bind(&now)
                .bind(id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// The row shape, so `query_as` can build it on both engines.
#[derive(sqlx::FromRow)]
struct NoteLinkRow {
    id: String,
    work_id: String,
    unit_id: String,
    revision_id: String,
    url: String,
    host: String,
    provider: String,
    status: String,
    consecutive_failures: i64,
    last_checked_at: Option<String>,
    last_healthy_at: Option<String>,
    failure_reason: Option<String>,
}

impl NoteLinkRow {
    fn into_link(self) -> NoteLink {
        NoteLink {
            id: self.id,
            work_id: self.work_id,
            unit_id: self.unit_id,
            revision_id: self.revision_id,
            url: self.url,
            host: self.host,
            provider: self.provider,
            status: self.status,
            consecutive_failures: self.consecutive_failures,
            last_checked_at: self.last_checked_at,
            last_healthy_at: self.last_healthy_at,
            failure_reason: self.failure_reason,
        }
    }
}

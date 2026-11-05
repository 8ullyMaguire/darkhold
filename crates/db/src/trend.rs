//! M45-32 — the trend radar.
//!
//! The signal is `work_tags.added_at`: when this instance first learned that a work carried a fandom tag.
//! It is not "when the author published" — a sources' own dates are patchy, and a fandom that was imported
//! late would look like it exploded. **The radar measures our arrivals, and says so.**
//!
//! Two windows, both counted in one query: the recent window and the baseline before it. The split is done
//! in SQL (`CASE WHEN`) so a fandom with works in both windows is one row rather than two that have to be
//! joined in code.
//!
//! **Text timestamps compare correctly here** because every writer stores RFC 3339 UTC with a `Z`, which is
//! the storage contract in `darkhold_app::format_rfc3339`; a lexicographic comparison of that format is a
//! chronological one. A dialect that stored local offsets would make this wrong, which is why the contract
//! exists.

use sqlx::Row;

use crate::{Database, Result};

/// One fandom's arrivals in the two windows.
#[derive(Debug, Clone, PartialEq)]
pub struct FandomCounts {
    /// The fandom's canonical name.
    pub fandom: String,
    /// Works that arrived inside the recent window.
    pub recent: i64,
    /// Works that arrived inside the baseline window, before it.
    pub baseline: i64,
}

/// Count arrivals per fandom over two windows ending at `now`.
///
/// `recent_since` and `baseline_since` are RFC 3339 UTC bounds: a row counts as recent when
/// `added_at >= recent_since`, and as baseline when `baseline_since <= added_at < recent_since`.
pub async fn counts(
    db: &Database,
    baseline_since: &str,
    recent_since: &str,
    min_recent: i64,
) -> Result<Vec<FandomCounts>> {
    let sql = db.sql(
        "SELECT n.canonical AS fandom, \
                SUM(CASE WHEN wt.added_at >= ?1 THEN 1 ELSE 0 END) AS recent, \
                SUM(CASE WHEN wt.added_at < ?1 THEN 1 ELSE 0 END) AS baseline \
           FROM work_tags wt \
           JOIN taxonomy_nodes n ON n.id = wt.node_id \
          WHERE n.kind = 'fandom' AND wt.added_at >= ?2 \
          GROUP BY n.canonical \
         HAVING SUM(CASE WHEN wt.added_at >= ?1 THEN 1 ELSE 0 END) >= ?3 \
          ORDER BY recent DESC, n.canonical",
        "SELECT n.canonical AS fandom, \
                SUM(CASE WHEN wt.added_at >= $1 THEN 1 ELSE 0 END)::bigint AS recent, \
                SUM(CASE WHEN wt.added_at < $1 THEN 1 ELSE 0 END)::bigint AS baseline \
           FROM work_tags wt \
           JOIN taxonomy_nodes n ON n.id::text = wt.node_id::text \
          WHERE n.kind = 'fandom' AND wt.added_at >= $2 \
          GROUP BY n.canonical \
         HAVING SUM(CASE WHEN wt.added_at >= $1 THEN 1 ELSE 0 END) >= $3 \
          ORDER BY recent DESC, n.canonical",
    );
    // Each arm maps its own row type: SQLite and PostgreSQL rows are different types, so the conversion
    // cannot be hoisted out of the match. The column names are the same on both sides by construction.
    Ok(match db.backend() {
        crate::Backend::Sqlite => {
            let rows = sqlx::query(&sql)
                .bind(recent_since)
                .bind(baseline_since)
                .bind(min_recent)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?;
            rows.into_iter()
                .map(|row| FandomCounts {
                    fandom: row.get("fandom"),
                    recent: row.get("recent"),
                    baseline: row.get("baseline"),
                })
                .collect()
        }
        crate::Backend::Postgres => {
            let rows = sqlx::query(&sql)
                .bind(recent_since)
                .bind(baseline_since)
                .bind(min_recent)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?;
            rows.into_iter()
                .map(|row| FandomCounts {
                    fandom: row.get("fandom"),
                    recent: row.get("recent"),
                    baseline: row.get("baseline"),
                })
                .collect()
        }
    })
}

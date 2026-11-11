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
        Backend::Sqlite => sqlx::query(&sql)
            .bind(work_id)
            .fetch_optional(db.sqlite_pool().ok_or_else(pool_err)?)
            .await?
            .map(|r| WorkForRecord {
                id: r.get("id"),
                title: r.get("title"),
                summary: r.get("summary"),
                language: r.get("language"),
                created_at: r.get("created_at"),
            }),
        Backend::Postgres => sqlx::query(&sql)
            .bind(work_id)
            .fetch_optional(db.postgres_pool().ok_or_else(pool_err)?)
            .await?
            .map(|r| WorkForRecord {
                id: r.get("id"),
                title: r.get("title"),
                summary: r.get("summary"),
                language: r.get("language"),
                created_at: r.get("created_at"),
            }),
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
        Backend::Sqlite => sqlx::query(&sql)
            .bind(work_id)
            .fetch_optional(db.sqlite_pool().ok_or_else(pool_err)?)
            .await?
            .map(|r| r.get::<String, _>("at_uri")),
        Backend::Postgres => sqlx::query(&sql)
            .bind(work_id)
            .fetch_optional(db.postgres_pool().ok_or_else(pool_err)?)
            .await?
            .map(|r| r.get::<String, _>("at_uri")),
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
            sqlx::query(&sql)
                .bind(&now)
                .bind(work_id)
                .execute(db.sqlite_pool().ok_or_else(pool_err)?)
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&now)
                .bind(work_id)
                .execute(db.postgres_pool().ok_or_else(pool_err)?)
                .await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The feed generators (W6 §7.5, slice B)
// ---------------------------------------------------------------------------

/// Which skeleton is being asked for.
///
/// Three feeds, and none of them invents a ranking: each is an ordering the site already produces. There is deliberately
/// no `top` — measured, the product's only ranked listing is `library_items ORDER BY likes DESC`, which is **per reader**,
/// and `work_metric_aggregates` is counted but never ordered by. A `top` feed would have to invent that rule for Bluesky,
/// which is a second ranking wearing a feed's clothes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Feed {
    /// Newest publications.
    Recent,
    /// Most recently touched works — the ordering `chapter.revised` already implies.
    Updated,
    /// Works that carry at least one tag, by the same recency.
    Tagged,
}

impl Feed {
    /// The column the ordering and the cursor both live in.
    ///
    /// A static string chosen from a closed enum, so it is safe to interpolate — and it must be *one* column, because a
    /// cursor that pages on one column while the sort uses another skips rows with no error.
    fn column(self) -> &'static str {
        match self {
            Feed::Recent => "w.published_at",
            Feed::Updated | Feed::Tagged => "w.updated_at",
        }
    }

    /// The extra predicate, if the feed has one.
    fn predicate(self) -> &'static str {
        match self {
            Feed::Recent | Feed::Updated => "",
            Feed::Tagged => " AND EXISTS (SELECT 1 FROM work_tags wt WHERE wt.work_id = w.id)",
        }
    }
}

/// One entry in a skeleton: the record to show, and the value a cursor resumes from.
#[derive(Debug, Clone)]
pub struct SkeletonItem {
    /// The `at://` URI the record bridge stored — never re-derived, or the skeleton would point at a record that may not
    /// exist.
    pub at_uri: String,
    /// The ordering value, in the exact shape the column stores (`YYYY-MM-DDTHH:MM:SS`), so a lexicographic comparison
    /// against the text column is the same comparison the database makes.
    pub cursor: String,
}

/// One page of a skeleton, newest first.
///
/// `before` is the previous page's last cursor. Rows without an `at_uri` are excluded by the query rather than filtered
/// here: a skeleton entry that does not resolve is worse than a shorter feed, and the database can say that with an index.
pub async fn feed_page(
    db: &Database,
    feed: Feed,
    limit: i64,
    before: Option<&str>,
) -> Result<Vec<SkeletonItem>, sqlx::Error> {
    let column = feed.column();
    let predicate = feed.predicate();
    // `<> ''` as well as `IS NOT NULL`: measured on a scratch instance, a work inserted with an empty string published
    // `{"post": ""}` to the skeleton — an entry that resolves to nothing, which is worse than a shorter feed. The column
    // is TEXT, so "no record" has two spellings and both must be excluded.
    let filter = "FROM works w
         WHERE w.visibility = 'public' AND w.lifecycle = 'published' AND w.deleted_at IS NULL
           AND w.at_uri IS NOT NULL AND w.at_uri <> ''";
    // Bound to locals before `db.sql`, which borrows both strings: a `&format!()` temporary would be dropped while the
    // returned `Cow` still pointed at it.
    let (sqlite_sql, postgres_sql) = match before {
        None => (
            format!("SELECT w.at_uri, {column} {filter}{predicate} ORDER BY {column} DESC LIMIT ?"),
            format!("SELECT w.at_uri, {column} {filter}{predicate} ORDER BY {column} DESC LIMIT $1"),
        ),
        Some(_) => (
            format!("SELECT w.at_uri, {column} {filter}{predicate} AND {column} < ? ORDER BY {column} DESC LIMIT ?"),
            format!("SELECT w.at_uri, {column} {filter}{predicate} AND {column} < $1 ORDER BY {column} DESC LIMIT $2"),
        ),
    };
    let sql = db.sql(&sqlite_sql, &postgres_sql);
    let limit = limit.clamp(1, 100);

    // `SqliteRow` and `PgRow` are distinct types, so the mapping happens INSIDE each arm — a `match` cannot return one
    // row type, and a `Row::get` on the wrong pool is a runtime panic rather than a compile error.
    let rows: Vec<SkeletonItem> = match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(&sql);
            if let Some(before) = before {
                q = q.bind(before);
            }
            q.bind(limit)
                .fetch_all(db.sqlite_pool().ok_or_else(pool_err)?)
                .await?
                .into_iter()
                .map(|r| SkeletonItem {
                    at_uri: r.get::<String, _>("at_uri"),
                    cursor: r.get::<String, _>(column.trim_start_matches("w.")),
                })
                .collect()
        }
        Backend::Postgres => {
            let mut q = sqlx::query(&sql);
            if let Some(before) = before {
                q = q.bind(before);
            }
            q.bind(limit)
                .fetch_all(db.postgres_pool().ok_or_else(pool_err)?)
                .await?
                .into_iter()
                .map(|r| SkeletonItem {
                    at_uri: r.get::<String, _>("at_uri"),
                    cursor: r.get::<String, _>(column.trim_start_matches("w.")),
                })
                .collect()
        }
    };
    Ok(rows)
}

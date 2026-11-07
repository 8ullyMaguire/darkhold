//! Crosspost union: one story's engagement summed across every archive it appears on (M64-07).
//!
//! The catalogue already displays `total_likes` and `source_count` (M63-04's normalised title+author resolution). This is the same
//! rule used as a **ranking input**: group the imported items by normalised title and author, sum the endorsement counts across the
//! group, and rank the groups.
//!
//! Two decisions the shape makes visible:
//!
//! * **The group is the story, not the copy.** A work that exists on three archives is one story with three audiences, and ranking the
//!   copies separately would put the same story in the feed three times with a third of its evidence each.
//! * **One representative work id, and it is a work that can be read.** `MIN(work_id)` — but only over items whose work is published
//!   and public, so an archived private draft cannot become the face of a popular story. A story with no readable copy is not ranked at
//!   all: it has engagement and nowhere to send the reader.
//!
//! Normalisation matches `catalogue.rs` exactly (`lower(trim(...))` / `lower(btrim(...))`) because two spellings of the same rule is how
//! a "total" and a "group" start disagreeing about which works are one story.

use crate::{Backend, Database, Result};

/// One story, as the union of its copies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosspostGroup {
    /// The representative work: the lowest-id readable copy of the story.
    pub work_id: String,
    /// The title as the catalogue spells it.
    pub title: String,
    /// The author as the catalogue spells it.
    pub author_text: String,
    /// Endorsements summed across every copy.
    pub total_likes: i64,
    /// How many distinct archives carry it.
    pub sources: i64,
}

/// Stories ranked by their summed engagement. `limit` bounds the answer, not the sum: the total is over every copy.
pub async fn union_ranking(db: &Database, limit: i64) -> Result<Vec<CrosspostGroup>> {
    let sql = db.sql(
        "SELECT MIN(li.work_id) AS work_id,
                MIN(li.title) AS title,
                MIN(li.author_text) AS author_text,
                SUM(li.likes) AS total_likes,
                COUNT(DISTINCT li.source_key) AS sources
           FROM library_items li
           JOIN works w ON w.id = li.work_id
          WHERE w.lifecycle = 'published' AND w.visibility = 'public'
            AND li.likes > 0
          GROUP BY lower(trim(li.title)), lower(trim(li.author_text))
          ORDER BY total_likes DESC, work_id
          LIMIT ?1",
        "SELECT MIN(li.work_id)::text AS work_id,
                MIN(li.title) AS title,
                MIN(li.author_text) AS author_text,
                SUM(li.likes)::bigint AS total_likes,
                COUNT(DISTINCT li.source_key)::bigint AS sources
           FROM library_items li
           JOIN works w ON w.id = li.work_id
          WHERE w.lifecycle = 'published' AND w.visibility = 'public'
            AND li.likes > 0
          GROUP BY lower(btrim(li.title)), lower(btrim(li.author_text))
          ORDER BY total_likes DESC, work_id
          LIMIT $1",
    );
    let rows: Vec<(String, String, String, i64, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(work_id, title, author_text, total_likes, sources)| CrosspostGroup {
                work_id,
                title,
                author_text,
                total_likes,
                sources,
            },
        )
        .collect())
}

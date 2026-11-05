//! M70-02 — the chapter spans of a work, in reading order, with their lengths.
//!
//! Whole-work reading needs one thing from the database that per-chapter reading never asks for: **how long each
//! chapter is**, in a unit the reader can be placed into. The unit is `word_count` on the chapter's current
//! revision, because that is the number the product already shows and derives once (ADR 0002) — a byte offset into
//! `sanitized_html` would mean two renderings of the same work disagree about where the reader was.
//!
//! The tables are `units` and `revisions` (renamed from `chapters` in migration 0122); the domain still calls
//! them chapters, which is the reader's word for them.
//!
//! Ordering is `order_key`, not creation time or revision number: chapters are spaced by ten so an insert between
//! two of them is one write, and a reader's place depends on that order being the author's.

use crate::{Backend, Database, Result};
use serde::Serialize;

/// One chapter's place in the whole-work document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    /// The chapter this span belongs to.
    pub chapter_id: String,
    /// Its title, empty when the author never gave one.
    pub title: String,
    /// Words before it in the work.
    pub start: u64,
    /// Words in it. Zero for a chapter with no saved revision yet — a real state, not an error.
    pub words: u64,
}

/// The chapter spans of `work_id`, in reading order. Empty when the work has no chapters, which is also a real
/// state (a draft), not an error.
pub async fn spans(db: &Database, work_id: &str) -> Result<Vec<Span>> {
    let statement = db.sql(
        "SELECT c.id, c.title, coalesce(r.word_count, 0) \
           FROM units c LEFT JOIN revisions r ON r.id = c.current_revision_id \
          WHERE c.work_id = ? AND c.deleted_at IS NULL \
          ORDER BY c.order_key",
        "SELECT c.id::text, c.title, coalesce(r.word_count, 0)::bigint \
           FROM units c LEFT JOIN revisions r ON r.id = c.current_revision_id \
          WHERE c.work_id = $1::uuid AND c.deleted_at IS NULL \
          ORDER BY c.order_key",
    );
    let rows: Vec<(String, String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let mut start = 0u64;
    Ok(rows
        .into_iter()
        .map(|(chapter_id, title, words)| {
            let words = u64::try_from(words).unwrap_or(0);
            let span = Span {
                chapter_id,
                title,
                start,
                words,
            };
            start += words;
            span
        })
        .collect())
}

/// The whole work's length in words.
pub async fn total_words(db: &Database, work_id: &str) -> Result<u64> {
    Ok(spans(db, work_id).await?.iter().map(|s| s.words).sum())
}

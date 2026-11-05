//! M45-71 — the facts a brief is built from, read once per work.
//!
//! One query per work rather than one with a variable `IN` list: the list needs a placeholder per
//! element, which means building SQL per request and two spellings of it (one per engine). Twenty works is
//! the cap on a shelf, so twenty small reads are the cheaper thing to keep correct.

use crate::{Backend, Database};
use anyhow::Result;

/// `(fandom, tag)` names a work carries, split by node kind.
pub struct WorkFacts {
    /// Fandom nodes.
    pub fandoms: Vec<String>,
    /// Every other tag-like node: tropes, characters, relationships, warnings, moods.
    pub tags: Vec<String>,
}

/// What one catalogued work carries.
///
/// A row with no `work_id` — an imported shelf entry never linked to a work — has no facts at all, which is
/// the honest answer: the instance has not read the work.
pub async fn facts_for_item(db: &Database, item_id: &str) -> Result<WorkFacts> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT ce.kind, ce.canonical
             FROM library_items li
             JOIN work_tags wt ON wt.work_id = li.work_id
             JOIN canonical_entities ce ON ce.id = wt.node_id
             WHERE li.id = ?
             ORDER BY ce.canonical"
        }
        Backend::Postgres => {
            "SELECT ce.kind, ce.canonical
             FROM library_items li
             JOIN work_tags wt ON wt.work_id = li.work_id
             JOIN canonical_entities ce ON ce.id = wt.node_id
             WHERE li.id = $1
             ORDER BY ce.canonical"
        }
    };
    let rows: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(item_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(item_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let mut fandoms = Vec::new();
    let mut tags = Vec::new();
    for (kind, canonical) in rows {
        if kind == "fandom" {
            fandoms.push(canonical);
        } else {
            tags.push(canonical);
        }
    }
    Ok(WorkFacts { fandoms, tags })
}

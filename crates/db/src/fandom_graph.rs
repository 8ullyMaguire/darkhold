//! M64-08's query half — works in the fandoms an operator declared adjacent.
//!
//! The map itself lives in `darkhold_domain::fandom_graph` and is configuration, not data this crate infers. What lives here is the
//! question a surface asks of it: *given these neighbouring fandoms and how strongly each is related, which works should a reader see?*
//!
//! Two properties matter more than the ranking:
//!
//! * **Published and public only.** An adjacency is a reason to look at a work, not a permission to see it.
//! * **The weight is carried through, not multiplied into a popularity count.** A work with 40 likes in a fandom the operator called
//!   `0.9` related and one with 400 likes in a fandom they called `0.4` related are different claims, and adding them into one number
//!   would erase which claim was made. The caller sees `(work, fandom, weight, likes)` and decides — the surfaces blend by weight, the
//!   page prints the fandom.
//!
//! An unmapped or unmatched fandom yields nothing rather than an error: most fandoms are not adjacent to anything, and a surface asking
//! about one should serve its other engines, not fail.

use std::collections::HashMap;

use serde::Serialize;

use crate::{Backend, Database, Result};

/// One work reached through a declared adjacency.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RelatedWork {
    /// The work's id.
    pub work_id: String,
    /// The neighbouring fandom that reached it, as the operator spelled it.
    pub fandom: String,
    /// The edge's weight, `0 < weight <= 1`.
    pub weight: f64,
    /// The work's accumulated endorsement count, for the caller's own ordering.
    pub likes: i64,
    /// The work's title, so a caller can render a row without a second query.
    pub title: String,
}

/// Works in the given fandoms, strongest edge first, then most liked.
///
/// `fandoms` is `(name, weight)` — the operator's names, matched case-insensitively against the catalogue's fandom nodes. Works in
/// `exclude` are omitted (the seed work, the reader's own library). `limit` bounds the answer after ordering, never the population.
pub async fn related_works(
    db: &Database,
    fandoms: &[(String, f64)],
    exclude: &[String],
    limit: i64,
) -> Result<Vec<RelatedWork>> {
    if fandoms.is_empty() || limit <= 0 {
        return Ok(Vec::new());
    }

    // The names travel as binds rather than being spliced into the SQL: an operator's fandom name is data, and the one thing a query
    // built by `format!` cannot survive is a name with a quote in it.
    let placeholders = match db.backend() {
        Backend::Sqlite => (1..=fandoms.len())
            .map(|n| format!("?{n}"))
            .collect::<Vec<_>>()
            .join(", "),
        Backend::Postgres => (1..=fandoms.len())
            .map(|n| format!("${n}"))
            .collect::<Vec<_>>()
            .join(", "),
    };
    let limit_bind = match db.backend() {
        Backend::Sqlite => format!("?{}", fandoms.len() + 1),
        Backend::Postgres => format!("${}", fandoms.len() + 1),
    };

    let sqlite = format!(
            "SELECT w.id, lower(ce.canonical) AS fandom, w.title,
                    COALESCE((SELECT wma.kudos FROM work_metric_aggregates wma WHERE wma.work_id = w.id), 0) AS likes
               FROM work_tags wt
               JOIN canonical_entities ce ON ce.id = wt.node_id AND ce.kind = 'fandom'
               JOIN works w ON w.id = wt.work_id
              WHERE w.lifecycle = 'published'
                AND w.visibility = 'public'
                AND w.deleted_at IS NULL
                AND lower(ce.canonical) IN ({placeholders})
              GROUP BY w.id, lower(ce.canonical), w.title
              LIMIT {limit_bind}"
        );
    let postgres = format!(
            "SELECT w.id::text AS id, lower(ce.canonical) AS fandom, w.title,
                    COALESCE((SELECT wma.kudos FROM work_metric_aggregates wma WHERE wma.work_id = w.id), 0)::bigint AS likes
               FROM work_tags wt
               JOIN canonical_entities ce ON ce.id = wt.node_id AND ce.kind = 'fandom'
               JOIN works w ON w.id = wt.work_id
              WHERE w.lifecycle = 'published'
                AND w.visibility = 'public'
                AND w.deleted_at IS NULL
                AND lower(ce.canonical) IN ({placeholders})
              GROUP BY w.id, lower(ce.canonical), w.title
              LIMIT {limit_bind}"
        );
    let sql = db.sql(&sqlite, &postgres);

    let weights: HashMap<String, f64> = fandoms
        .iter()
        .map(|(name, weight)| (name.trim().to_lowercase(), *weight))
        .collect();
    let display: HashMap<String, String> = fandoms
        .iter()
        .map(|(name, _)| (name.trim().to_lowercase(), name.trim().to_owned()))
        .collect();
    let excluded: std::collections::HashSet<&str> = exclude.iter().map(String::as_str).collect();

    let rows: Vec<(String, String, String, i64)> = match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as(&sql);
            for (name, _) in fandoms {
                query = query.bind(name.trim().to_lowercase());
            }
            query
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as(&sql);
            for (name, _) in fandoms {
                query = query.bind(name.trim().to_lowercase());
            }
            query
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    let mut out: Vec<RelatedWork> = rows
        .into_iter()
        .filter(|(work_id, _, _, _)| !excluded.contains(work_id.as_str()))
        .filter_map(|(work_id, fandom, title, likes)| {
            let weight = *weights.get(&fandom)?;
            Some(RelatedWork {
                work_id,
                fandom: display.get(&fandom).cloned().unwrap_or(fandom),
                weight,
                likes,
                title,
            })
        })
        .collect();

    out.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.likes.cmp(&a.likes))
            .then_with(|| a.work_id.cmp(&b.work_id))
    });
    out.truncate(limit.max(0) as usize);
    Ok(out)
}

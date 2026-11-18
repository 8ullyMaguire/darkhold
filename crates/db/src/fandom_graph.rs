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

/// One author reached through a declared adjacency, with the operator's edge weight intact.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarAuthor {
    pub author: String,
    pub fandom: String,
    pub weight: f64,
    pub works: i64,
}

fn bind_names(count: usize, backend: &Backend) -> String {
    match backend {
        Backend::Sqlite => (1..=count)
            .map(|n| format!("?{n}"))
            .collect::<Vec<_>>()
            .join(", "),
        Backend::Postgres => (1..=count)
            .map(|n| format!("${n}"))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Works carrying `tag` in the declared neighbouring fandoms. The tag and the fandom are separate canonical
/// nodes on the same work; an adjacency is still a reason to look, never a permission to see a private work.
pub async fn tag_cross_fandom_works(
    db: &Database,
    tag: &str,
    fandoms: &[(String, f64)],
    exclude: &[String],
    limit: i64,
) -> Result<Vec<RelatedWork>> {
    if fandoms.is_empty() || limit <= 0 || tag.trim().is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = bind_names(fandoms.len() + 1, &db.backend());
    let tag_bind = match db.backend() {
        Backend::Sqlite => format!("?{}", fandoms.len() + 1),
        Backend::Postgres => format!("${}", fandoms.len() + 1),
    };
    let limit_bind = match db.backend() {
        Backend::Sqlite => format!("?{}", fandoms.len() + 2),
        Backend::Postgres => format!("${}", fandoms.len() + 2),
    };
    let common = format!(
        "SELECT w.id, lower(cf.canonical) AS fandom, w.title,
                COALESCE((SELECT wma.kudos FROM work_metric_aggregates wma WHERE wma.work_id = w.id), 0) AS likes
           FROM works w
           JOIN work_tags wf ON wf.work_id = w.id
           JOIN canonical_entities cf ON cf.id = wf.node_id AND cf.kind = 'fandom'
           JOIN work_tags wt ON wt.work_id = w.id
           JOIN canonical_entities ct ON ct.id = wt.node_id AND ct.kind = 'trope'
          WHERE w.lifecycle = 'published' AND w.visibility = 'public' AND w.deleted_at IS NULL
            AND lower(cf.canonical) IN ({placeholders})
            AND lower(ct.canonical) = lower({tag_bind})
          GROUP BY w.id, lower(cf.canonical), w.title
          LIMIT {limit_bind}"
    );
    let sql = match db.backend() {
        Backend::Sqlite => common.clone(),
        Backend::Postgres => common
            .replacen("w.id,", "w.id::text,", 1)
            .replace(", 0) AS likes", ", 0)::bigint AS likes"),
    };
    let rows: Vec<(String, String, String, i64)> = match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as::<sqlx::Sqlite, _>(&sql);
            for (name, _) in fandoms {
                query = query.bind(name.trim().to_lowercase());
            }
            query
                .bind(tag.trim())
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as::<sqlx::Postgres, _>(&sql);
            for (name, _) in fandoms {
                query = query.bind(name.trim().to_lowercase());
            }
            query
                .bind(tag.trim())
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let weights: HashMap<String, f64> = fandoms
        .iter()
        .map(|(n, w)| (n.trim().to_lowercase(), *w))
        .collect();
    let display: HashMap<String, String> = fandoms
        .iter()
        .map(|(n, _)| (n.trim().to_lowercase(), n.trim().to_owned()))
        .collect();
    let excluded: std::collections::HashSet<&str> = exclude.iter().map(String::as_str).collect();
    let mut out: Vec<RelatedWork> = rows
        .into_iter()
        .filter(|(id, _, _, _)| !excluded.contains(id.as_str()))
        .filter_map(|(work_id, fandom, title, likes)| {
            Some(RelatedWork {
                work_id,
                fandom: display.get(&fandom).cloned().unwrap_or(fandom.clone()),
                weight: *weights.get(&fandom)?,
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

/// Fandoms an author actually writes in, from published/public works only.
pub async fn author_fandoms(db: &Database, author: &str) -> Result<Vec<String>> {
    if author.trim().is_empty() {
        return Ok(Vec::new());
    }
    let sql = match db.backend() {
        Backend::Sqlite => "SELECT DISTINCT lower(cf.canonical) AS fandom
              FROM works w
              JOIN work_tags wf ON wf.work_id = w.id
              JOIN canonical_entities cf ON cf.id = wf.node_id AND cf.kind = 'fandom'
              JOIN work_tags wa ON wa.work_id = w.id
              JOIN canonical_entities ca ON ca.id = wa.node_id AND ca.kind = 'author'
             WHERE w.lifecycle = 'published' AND w.visibility = 'public' AND w.deleted_at IS NULL
               AND lower(ca.canonical) = lower(?)"
            .to_owned(),
        Backend::Postgres => "SELECT DISTINCT lower(cf.canonical) AS fandom
              FROM works w
              JOIN work_tags wf ON wf.work_id = w.id
              JOIN canonical_entities cf ON cf.id = wf.node_id AND cf.kind = 'fandom'
              JOIN work_tags wa ON wa.work_id = w.id
              JOIN canonical_entities ca ON ca.id = wa.node_id AND ca.kind = 'author'
             WHERE w.lifecycle = 'published' AND w.visibility = 'public' AND w.deleted_at IS NULL
               AND lower(ca.canonical) = lower($1)"
            .to_owned(),
    };
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<sqlx::Sqlite, _>(&sql)
                .bind(author.trim())
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<sqlx::Postgres, _>(&sql)
                .bind(author.trim())
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows.into_iter().map(|(name,)| name).collect())
}

/// Other authors with works in the given neighbouring fandoms, strongest edge first.
pub async fn similar_authors(
    db: &Database,
    exclude_author: &str,
    fandoms: &[(String, f64)],
    limit: i64,
) -> Result<Vec<SimilarAuthor>> {
    if fandoms.is_empty() || limit <= 0 {
        return Ok(Vec::new());
    }
    let placeholders = bind_names(fandoms.len() + 1, &db.backend());
    let author_bind = match db.backend() {
        Backend::Sqlite => format!("?{}", fandoms.len() + 1),
        Backend::Postgres => format!("${}", fandoms.len() + 1),
    };
    let limit_bind = match db.backend() {
        Backend::Sqlite => format!("?{}", fandoms.len() + 2),
        Backend::Postgres => format!("${}", fandoms.len() + 2),
    };
    let sql = format!(
        "SELECT ca.canonical AS author, lower(cf.canonical) AS fandom, COUNT(DISTINCT w.id) AS works
           FROM works w
           JOIN work_tags wf ON wf.work_id = w.id
           JOIN canonical_entities cf ON cf.id = wf.node_id AND cf.kind = 'fandom'
           JOIN work_tags wa ON wa.work_id = w.id
           JOIN canonical_entities ca ON ca.id = wa.node_id AND ca.kind = 'author'
          WHERE w.lifecycle = 'published' AND w.visibility = 'public' AND w.deleted_at IS NULL
            AND lower(cf.canonical) IN ({placeholders})
            AND lower(ca.canonical) <> lower({author_bind})
          GROUP BY ca.canonical, lower(cf.canonical)
          ORDER BY lower(cf.canonical), ca.canonical
          LIMIT {limit_bind}"
    );
    let rows: Vec<(String, String, i64)> = match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as::<sqlx::Sqlite, _>(&sql);
            for (name, _) in fandoms {
                query = query.bind(name.trim().to_lowercase());
            }
            query
                .bind(exclude_author.trim())
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as::<sqlx::Postgres, _>(&sql);
            for (name, _) in fandoms {
                query = query.bind(name.trim().to_lowercase());
            }
            query
                .bind(exclude_author.trim())
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let weights: HashMap<String, f64> = fandoms
        .iter()
        .map(|(n, w)| (n.trim().to_lowercase(), *w))
        .collect();
    let display: HashMap<String, String> = fandoms
        .iter()
        .map(|(n, _)| (n.trim().to_lowercase(), n.trim().to_owned()))
        .collect();
    let mut out: Vec<SimilarAuthor> = rows
        .into_iter()
        .filter_map(|(author, fandom, works)| {
            Some(SimilarAuthor {
                author,
                fandom: display.get(&fandom).cloned().unwrap_or(fandom.clone()),
                weight: *weights.get(&fandom)?,
                works,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.works.cmp(&a.works))
            .then_with(|| a.author.cmp(&b.author))
    });
    out.truncate(limit.max(0) as usize);
    Ok(out)
}

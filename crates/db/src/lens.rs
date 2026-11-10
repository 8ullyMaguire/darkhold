//! Curator lenses — §0.3's user-facing counterpart to the operator's own lens (M45-40).
//!
//! A lens is one curator's published view of the catalogue: the same works everybody can see, in the order that curator's
//! own kudos argue for. The operator's lens is deliberately absent from every door here — §0.3 forbids it being inferable
//! through any user-facing label, and "browse the archive ordered by it" is the strongest label there is.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::search::{search_works_ast_scoped, SearchFilters};
use crate::{Backend, Database};

/// One published lens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lens {
    /// The account whose view it is.
    pub account_id: String,
    /// The handle the view is published under.
    pub handle: String,
    /// One sentence from the curator about what the view is.
    pub blurb: String,
    /// When it was first published, RFC 3339.
    pub published_at: String,
}

/// What a lens decided about its own ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LensBasis {
    /// The curator's own kudos overlap with the works' tags, so the order is theirs.
    Taste,
    /// The curator has left no kudos that touch any candidate tag, so the order is the catalogue's and the document says so.
    Catalogue,
}

/// Publish, or edit, the caller's lens.
///
/// Upsert on the account: a person has one view, and a second `PUT` is that view edited — not a second lens.
pub async fn publish(db: &Database, account_id: &str, blurb: &str) -> Result<()> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO curator_lens (account_id, blurb, published_at, updated_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (account_id) DO UPDATE SET blurb = excluded.blurb, updated_at = excluded.updated_at",
        "INSERT INTO curator_lens (account_id, blurb, published_at, updated_at) VALUES ($1::uuid, $2, $3, $4)
         ON CONFLICT (account_id) DO UPDATE SET blurb = excluded.blurb, updated_at = excluded.updated_at",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(account_id)
                .bind(blurb)
                .bind(&now)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(account_id)
                .bind(blurb)
                .bind(&now)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(())
}

/// Withdraw the caller's lens. `false` when there was none.
pub async fn withdraw(db: &Database, account_id: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM curator_lens WHERE account_id = ?",
        "DELETE FROM curator_lens WHERE account_id = $1::uuid",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(account_id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(account_id)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// Every published lens, ordered by handle so the list is stable between reads.
pub async fn published(db: &Database) -> Result<Vec<Lens>> {
    let sql = db.sql(
        "SELECT l.account_id, p.handle, l.blurb, l.published_at
         FROM curator_lens l
         JOIN pseuds p ON p.account_id = l.account_id
         GROUP BY l.account_id, l.blurb, l.published_at
         ORDER BY lower(p.handle)",
        "SELECT l.account_id::text AS account_id, p.handle, l.blurb, l.published_at
         FROM curator_lens l
         JOIN pseuds p ON p.account_id = l.account_id
         GROUP BY l.account_id, l.blurb, l.published_at, p.handle
         ORDER BY lower(p.handle)",
    );
    let rows: Vec<(String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(account_id, handle, blurb, published_at)| Lens {
            account_id,
            handle,
            blurb,
            published_at,
        })
        .collect())
}

/// One lens by handle, case-insensitively — handles are unique per account but not globally, and a lens is one per account,
/// so the lowest handle wins when an account has several pseuds.
pub async fn find(db: &Database, handle: &str) -> Result<Option<Lens>> {
    let sql = db.sql(
        "SELECT l.account_id, p.handle, l.blurb, l.published_at
         FROM curator_lens l
         JOIN pseuds p ON p.account_id = l.account_id
         WHERE lower(p.handle) = lower(?)
         ORDER BY p.handle
         LIMIT 1",
        "SELECT l.account_id::text AS account_id, p.handle, l.blurb, l.published_at
         FROM curator_lens l
         JOIN pseuds p ON p.account_id = l.account_id
         WHERE lower(p.handle) = lower($1)
         ORDER BY p.handle
         LIMIT 1",
    );
    let row: Option<(String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(handle)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(handle)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(row.map(|(account_id, handle, blurb, published_at)| Lens {
        account_id,
        handle,
        blurb,
        published_at,
    }))
}

/// How many of the curator's kudos each tag accounts for — their taste, counted rather than inferred.
async fn tag_affinity(db: &Database, account_id: &str) -> Result<Vec<(String, i64)>> {
    let sql = db.sql(
        "SELECT wt.node_id AS node_id, COUNT(*) AS weight
         FROM work_kudos k JOIN work_tags wt ON wt.work_id = k.work_id
         WHERE k.account_id = ?
         GROUP BY wt.node_id",
        "SELECT wt.node_id AS node_id, CAST(COUNT(*) AS BIGINT) AS weight
         FROM work_kudos k JOIN work_tags wt ON wt.work_id = k.work_id
         WHERE k.account_id = $1::uuid
         GROUP BY wt.node_id",
    );
    let rows: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows)
}

/// The lens's order over the catalogue the reader may already see.
///
/// The candidate set comes from `search_works_ast_scoped` — the real entry point — so visibility, blocked tags and
/// restricted audiences stay the search's business and this function only re-orders what search was willing to show. A
/// second visibility rule here would be a narrower twin of the one readers already meet, and the pair would drift.
pub async fn ranked_for(
    db: &Database,
    curator_account: &str,
    viewer_id: Option<&str>,
    limit: i64,
) -> Result<(Vec<crate::search::SearchResult>, LensBasis)> {
    let limit = limit.clamp(1, 100);
    // A wider candidate window than the page: a work the curator liked can be anywhere in the catalogue's order, and a
    // window of exactly `limit` would only ever re-shuffle the first page.
    let candidates = search_works_ast_scoped(
        db,
        "",
        viewer_id,
        (limit * 3).clamp(limit, 300),
        &[],
        &SearchFilters::default(),
    )
    .await?;
    if candidates.is_empty() {
        return Ok((candidates, LensBasis::Catalogue));
    }
    let affinity: std::collections::HashMap<String, i64> = tag_affinity(db, curator_account)
        .await?
        .into_iter()
        .collect();
    if affinity.is_empty() {
        let mut page = candidates;
        page.truncate(limit as usize);
        return Ok((page, LensBasis::Catalogue));
    }
    let ids: Vec<String> = candidates.iter().map(|c| c.work_id.clone()).collect();
    let weights = tags_of(db, &ids).await?;
    let mut scored: Vec<(usize, i64, crate::search::SearchResult)> =
        Vec::with_capacity(candidates.len());
    for (position, candidate) in candidates.into_iter().enumerate() {
        let score = weights
            .get(&candidate.work_id)
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(|node| affinity.get(node))
                    .sum::<i64>()
            })
            .unwrap_or(0);
        scored.push((position, score, candidate));
    }
    // Ties keep the catalogue's order (`position`), so a lens with no signal is the catalogue rather than a shuffle.
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let any_signal = scored.first().is_some_and(|(_, score, _)| *score > 0);
    let page: Vec<_> = scored
        .into_iter()
        .take(limit as usize)
        .map(|(_, _, c)| c)
        .collect();
    Ok((
        page,
        if any_signal {
            LensBasis::Taste
        } else {
            LensBasis::Catalogue
        },
    ))
}

/// The tags of the candidate works, fetched in one query rather than per row.
async fn tags_of(
    db: &Database,
    work_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<String>>> {
    let mut out: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    if work_ids.is_empty() {
        return Ok(out);
    }
    // Chunked so the placeholder list stays well inside every engine's parameter cap.
    for chunk in work_ids.chunks(200) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let placeholders_pg = (1..=chunk.len())
            .map(|n| format!("${n}::uuid"))
            .collect::<Vec<_>>()
            .join(",");
        let sqlite_sql =
            format!("SELECT work_id, node_id FROM work_tags WHERE work_id IN ({placeholders})");
        let pg_sql = format!(
            "SELECT work_id::text AS work_id, node_id FROM work_tags WHERE work_id IN ({placeholders_pg})"
        );
        let sql = db.sql(&sqlite_sql, &pg_sql);
        let pairs: Vec<(String, String)> = match db.backend() {
            Backend::Sqlite => {
                let mut query = sqlx::query_as(&sql);
                for id in chunk {
                    query = query.bind(id);
                }
                query.fetch_all(db.sqlite_pool().expect("sqlite")).await?
            }
            Backend::Postgres => {
                let mut query = sqlx::query_as(&sql);
                for id in chunk {
                    query = query.bind(id);
                }
                query
                    .fetch_all(db.postgres_pool().expect("postgres"))
                    .await?
            }
        };
        for (work_id, node_id) in pairs {
            out.entry(work_id).or_default().push(node_id);
        }
    }
    Ok(out)
}

/// Every handle on an account, so a door can check that the handle in the path is the caller's own.
pub async fn handles_for(db: &Database, account_id: &str) -> Result<Vec<String>> {
    let sql = db.sql(
        "SELECT handle FROM pseuds WHERE account_id = ?",
        "SELECT handle FROM pseuds WHERE account_id = $1::uuid",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows)
}

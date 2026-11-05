//! M45-72 — the shelves a reader keeps, with the query that makes each one (M45-72).
//!
//! Reads `saved_views` and, where one exists, its last run. The run is a `LEFT JOIN`, not a second query per
//! row: a page that showed a shelf with no run as "not loaded" would be wrong, and one query is honest about
//! which shelves have never run.

use crate::{Backend, Database};
use anyhow::Result;

/// The seven columns the page needs, in the order the two SQL statements select them.
type Row = (
    String,
    String,
    i64,
    String,
    String,
    Option<String>,
    Option<i64>,
);

/// A shelf row, before the domain renders it.
pub struct ShelfRow {
    pub name: String,
    pub scope: String,
    pub pinned: bool,
    pub document: String,
    pub last_changed: String,
    pub last_run: Option<(String, i64)>,
}

/// The account's saved views, pinned first, then by name.
///
/// Ordered in SQL rather than in the caller: the page is a document, and which shelf opens it is an
/// editorial decision (`pinned`) rather than an accident of row order.
pub async fn shelves_for_account(db: &Database, account_id: &str) -> Result<Vec<ShelfRow>> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT v.name, v.scope, v.pinned, v.query_json, v.updated_at, r.ran_at, r.seen_count
             FROM saved_views v
             LEFT JOIN saved_query_runs r ON r.saved_view_id = v.id
             WHERE v.account_id = ?
             ORDER BY v.pinned DESC, v.name"
        }
        Backend::Postgres => {
            // `pinned` is BOOLEAN on PostgreSQL and INTEGER on SQLite; the cast makes one decoder serve
            // both, which is the same trick the library store uses for its own view rows.
            "SELECT v.name, v.scope, v.pinned::int::bigint AS pinned, v.query_json, v.updated_at,
                    r.ran_at, r.seen_count::bigint AS seen_count
             FROM saved_views v
             LEFT JOIN saved_query_runs r ON r.saved_view_id = v.id
             WHERE v.account_id = $1::uuid
             ORDER BY v.pinned DESC, v.name"
        }
    };
    let rows: Vec<Row> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(name, scope, pinned, document, last_changed, ran_at, seen)| ShelfRow {
                name,
                scope,
                pinned: pinned != 0,
                document,
                last_changed,
                last_run: ran_at.map(|ran_at| (ran_at, seen.unwrap_or(0))),
            },
        )
        .collect())
}

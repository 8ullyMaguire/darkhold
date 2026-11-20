//! Read-only candidate feed for private taste ranking (TA1).
//!
//! One query shape for SQLite and PostgreSQL. The caller does the scoring in
//! `darkhold-taste`, so this module never sees the private preference file.

use crate::{Backend, Database, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TasteCandidate {
    pub library_item_id: String,
    pub source_key: String,
    pub source_work_key: String,
    pub source_url: String,
    pub title: String,
    pub summary: String,
    pub provenance_json: String,
}

pub async fn candidates(
    db: &Database,
    account_id: &str,
    limit: i64,
) -> Result<Vec<TasteCandidate>> {
    let sql = db.sql(
        "SELECT id, source_key, source_work_key, source_url, title, summary, provenance_json
           FROM library_items
          WHERE account_id = ?1 AND source_url <> ''
          ORDER BY created_at DESC, id DESC
          LIMIT ?2",
        "SELECT id::text, source_key, source_work_key, source_url, title, summary, provenance_json
           FROM library_items
          WHERE account_id = $1::uuid AND source_url <> ''
          ORDER BY created_at DESC, id DESC
          LIMIT $2",
    );
    let rows: Vec<(String, String, String, String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite pool"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres pool"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(
                library_item_id,
                source_key,
                source_work_key,
                source_url,
                title,
                summary,
                provenance_json,
            )| TasteCandidate {
                library_item_id,
                source_key,
                source_work_key,
                source_url,
                title,
                summary,
                provenance_json,
            },
        )
        .collect())
}

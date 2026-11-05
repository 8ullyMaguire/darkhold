//! M70-05 — collection tags: a fact about the collection, never about its works.
//!
//! See `migrations/*/0159_collection_tags.sql`. The rule that matters is a *non*-rule: nothing here writes
//! `work_tags`, and no work-facing query reads `collection_tags`. That is what keeps a challenge tag from dragging
//! every work inside the challenge into that tag's search results and into readers' taste vectors.
//!
//! Tags are `canonical_entities` rows, so canonicalisation (and the wrangling that produces it) is shared with every
//! other tag in the product rather than reimplemented for collections.

use anyhow::Result;
use serde::Serialize;

use crate::{Backend, Database};

/// A tag attached to a collection, with the words it displays as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectionTag {
    /// The taxonomy node, which is what a filter would compare.
    pub node_id: String,
    /// The node's kind (`fandom`, `relationship`, `freeform`, …), so a client can group without a second call.
    pub kind: String,
    /// The canonical text.
    pub canonical: String,
    /// When it was attached.
    pub added_at: String,
}

/// Whether `account_id` owns this collection — the only permission this module needs.
pub async fn owned_by(db: &Database, collection_id: &str, account_id: &str) -> Result<bool> {
    let statement = db.sql(
        "SELECT count(*) FROM collections WHERE id = ? AND owner = ?",
        "SELECT count(*) FROM collections WHERE id::text = $1 AND owner = $2",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&statement)
                .bind(collection_id)
                .bind(account_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&statement)
                .bind(collection_id)
                .bind(account_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(count > 0)
}

/// Attach a tag to a collection. Idempotent: attaching twice is one row and not an error.
pub async fn add(db: &Database, collection_id: &str, node_id: &str) -> Result<bool> {
    let statement = db.sql(
        "INSERT INTO collection_tags (collection_id, node_id, added_at) VALUES (?, ?, ?) \
         ON CONFLICT (collection_id, node_id) DO NOTHING",
        "INSERT INTO collection_tags (collection_id, node_id, added_at) VALUES ($1::uuid, $2, $3) \
         ON CONFLICT (collection_id, node_id) DO NOTHING",
    );
    let now = crate::identity::now_rfc3339();
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(collection_id)
            .bind(node_id)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(collection_id)
            .bind(node_id)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// Detach a tag. Returns whether a row was removed.
pub async fn remove(db: &Database, collection_id: &str, node_id: &str) -> Result<bool> {
    let statement = db.sql(
        "DELETE FROM collection_tags WHERE collection_id = ? AND node_id = ?",
        "DELETE FROM collection_tags WHERE collection_id::text = $1 AND node_id = $2",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(collection_id)
            .bind(node_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(collection_id)
            .bind(node_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// The tags of a collection, in attachment order.
pub async fn list(db: &Database, collection_id: &str) -> Result<Vec<CollectionTag>> {
    let statement = db.sql(
        "SELECT t.node_id, n.kind, n.canonical, t.added_at \
           FROM collection_tags t JOIN canonical_entities n ON n.id = t.node_id \
          WHERE t.collection_id = ? ORDER BY t.added_at, t.node_id",
        "SELECT t.node_id, n.kind, n.canonical, t.added_at \
           FROM collection_tags t JOIN canonical_entities n ON n.id = t.node_id \
          WHERE t.collection_id::text = $1 ORDER BY t.added_at, t.node_id",
    );
    let rows: Vec<(String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(collection_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(collection_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(node_id, kind, canonical, added_at)| CollectionTag {
            node_id,
            kind,
            canonical,
            added_at,
        })
        .collect())
}

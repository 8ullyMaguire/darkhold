//! M70-06 — the reader's own stylesheet, stored and served to them and to nobody else.
//!
//! Every function takes an account, and there is no lookup by anything else: a skin has no id, no slug and no
//! public flag, so "somebody else's skin" is not a URL that can be constructed.

use anyhow::Result;

use crate::{Backend, Database};

/// A stored skin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSkin {
    /// The stylesheet, as it will be served.
    pub css: String,
    /// When it was last written.
    pub updated_at: String,
}

/// The reader's skin, if they have one.
pub async fn get(db: &Database, account_id: &str) -> Result<Option<StoredSkin>> {
    let statement = db.sql(
        "SELECT css, updated_at FROM reader_skins WHERE account = ?",
        "SELECT css, updated_at FROM reader_skins WHERE account::text = $1",
    );
    let row: Option<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(account_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(account_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(|(css, updated_at)| StoredSkin { css, updated_at }))
}

/// Write (or replace) the reader's skin.
///
/// The replacement is unconditional: a reader has one skin, and "which one wins" is not a question worth a version
/// number. The caller has already validated the CSS.
pub async fn put(db: &Database, account_id: &str, css: &str) -> Result<()> {
    let statement = db.sql(
        "INSERT INTO reader_skins (account, css, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT (account) DO UPDATE SET css = excluded.css, updated_at = excluded.updated_at",
        "INSERT INTO reader_skins (account, css, updated_at) VALUES ($1::uuid, $2, $3) \
         ON CONFLICT (account) DO UPDATE SET css = excluded.css, updated_at = excluded.updated_at",
    );
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&statement)
                .bind(account_id)
                .bind(css)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&statement)
                .bind(account_id)
                .bind(css)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Remove the reader's skin. Returns whether there was one.
pub async fn remove(db: &Database, account_id: &str) -> Result<bool> {
    let statement = db.sql(
        "DELETE FROM reader_skins WHERE account = ?",
        "DELETE FROM reader_skins WHERE account::text = $1",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(account_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(account_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

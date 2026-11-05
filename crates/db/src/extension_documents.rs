//! M70-09 — storage for extension documents, where the canonical text is the record.
//!
//! Nothing here parses. `darkhold_domain::extension_document::parse` is the only entry point in the product, and it
//! runs on both paths: a document a reader wrote and a document they imported are validated by the same code, so an
//! import cannot reach a state the API refuses.
//!
//! Every function takes an account. A document is private until its author publishes it, and even then the
//! reader-side listing is a separate call (M70-10) rather than a flag this module quietly honours.

use anyhow::Result;

use crate::{Backend, Database};

/// A stored document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDocument {
    /// Row id.
    pub id: String,
    /// Owning account.
    pub account: String,
    /// The document's own name, taken from the parsed document.
    pub name: String,
    /// The document's kind slug, taken from the parsed document.
    pub kind: String,
    /// The canonical document text: what export returns, byte for byte.
    pub document: String,
    /// Whether other readers may see it.
    pub is_public: bool,
    /// When it was stored.
    pub created_at: String,
}

/// Store a validated document. Returns the new id.
pub async fn create(
    db: &Database,
    account_id: &str,
    document: &str,
    name: &str,
    kind: &str,
    is_public: bool,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();
    let statement = db.sql(
        "INSERT INTO extension_documents (id, account, name, kind, document, is_public, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        "INSERT INTO extension_documents (id, account, name, kind, document, is_public, created_at) \
         VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6, $7)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&statement)
                .bind(&id)
                .bind(account_id)
                .bind(name)
                .bind(kind)
                .bind(document)
                .bind(i64::from(is_public))
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&statement)
                .bind(&id)
                .bind(account_id)
                .bind(name)
                .bind(kind)
                .bind(document)
                .bind(i64::from(is_public))
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(id)
}

fn row_to_stored(row: (String, String, String, String, String, i64, String)) -> StoredDocument {
    let (id, account, name, kind, document, is_public, created_at) = row;
    StoredDocument {
        id,
        account,
        name,
        kind,
        document,
        is_public: is_public != 0,
        created_at,
    }
}

/// The selected columns, with `account` cast to text on PostgreSQL.
///
/// Two casts, both because sqlx refuses to decode a differently-typed column rather than guessing: `account` is
/// `UUID` where the struct field is a `String`, and `is_public` is `INT4` where the field is an `i64`. Each one is a
/// loud error instead of a silent wrong value, which is the right way round.
fn select_columns(postgres: bool) -> &'static str {
    if postgres {
        "id, account::text, name, kind, document, is_public::bigint, created_at"
    } else {
        "id, account, name, kind, document, is_public, created_at"
    }
}

/// One document, if the viewer may see it: their own, or somebody's public one.
pub async fn get(db: &Database, id: &str, viewer: &str) -> Result<Option<StoredDocument>> {
    let sqlite = format!(
        "SELECT {} FROM extension_documents WHERE id = ? AND (account = ? OR is_public = 1)",
        select_columns(false)
    );
    let postgres = format!(
        "SELECT {} FROM extension_documents WHERE id = $1 AND (account::text = $2 OR is_public = 1)",
        select_columns(true)
    );
    let statement = db.sql(&sqlite, &postgres);
    let row: Option<(String, String, String, String, String, i64, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(statement.as_ref())
                .bind(id)
                .bind(viewer)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(statement.as_ref())
                .bind(id)
                .bind(viewer)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(row_to_stored))
}

/// The viewer's own documents, newest first. Public ones do not appear here: a list of "mine" that included somebody
/// else's would make the word do two jobs.
pub async fn list_mine(db: &Database, account_id: &str) -> Result<Vec<StoredDocument>> {
    let sqlite = format!(
        "SELECT {} FROM extension_documents WHERE account = ? ORDER BY created_at DESC, id",
        select_columns(false)
    );
    let postgres = format!(
        "SELECT {} FROM extension_documents WHERE account::text = $1 ORDER BY created_at DESC, id",
        select_columns(true)
    );
    let statement = db.sql(&sqlite, &postgres);
    let rows: Vec<(String, String, String, String, String, i64, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(statement.as_ref())
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(statement.as_ref())
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(row_to_stored).collect())
}

/// One row of the public browse list: the document plus the pseud it is attributed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicDocument {
    /// Row id.
    pub id: String,
    /// The document's name.
    pub name: String,
    /// The kind slug.
    pub kind: String,
    /// The canonical document text, so a caller can render a preview without a second read.
    pub document: String,
    /// The publishing pseud's handle.
    pub pseud_handle: String,
    /// The publishing pseud's display name.
    pub pseud_display_name: String,
    /// When it was stored.
    pub created_at: String,
}

/// Publish one of the viewer's documents, attributed to `pseud_id`.
///
/// The pseud must belong to the account: a document published as somebody else's pseud would be impersonation with
/// extra steps, and it is cheap to refuse here.
pub async fn publish(db: &Database, id: &str, account_id: &str, pseud_id: &str) -> Result<bool> {
    let owned = db.sql(
        "SELECT count(*) FROM pseuds WHERE id = ? AND account_id = ?",
        "SELECT count(*) FROM pseuds WHERE id::text = $1 AND account_id::text = $2",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&owned)
                .bind(pseud_id)
                .bind(account_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&owned)
                .bind(pseud_id)
                .bind(account_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    if count == 0 {
        return Ok(false);
    }
    let statement = db.sql(
        "UPDATE extension_documents SET is_public = 1, pseud_id = ? WHERE id = ? AND account = ?",
        "UPDATE extension_documents SET is_public = 1, pseud_id = $1::uuid WHERE id = $2 AND account::text = $3",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(pseud_id)
            .bind(id)
            .bind(account_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(pseud_id)
            .bind(id)
            .bind(account_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// Withdraw a document from the browse list. The row stays — a reader may have imported it, and deleting their copy
/// is not this instance's business — but it stops being offered.
pub async fn unpublish(db: &Database, id: &str, account_id: &str) -> Result<bool> {
    let statement = db.sql(
        "UPDATE extension_documents SET is_public = 0, pseud_id = NULL WHERE id = ? AND account = ?",
        "UPDATE extension_documents SET is_public = 0, pseud_id = NULL WHERE id = $1 AND account::text = $2",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(id)
            .bind(account_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(id)
            .bind(account_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// The browse list: published documents, newest first, optionally narrowed by kind.
///
/// Newest first and nothing else. There is no install count, no rating and no popularity ordering — on a corpus of
/// four, ranking by popularity is a way to make the first uploader permanently first, and a count nobody can audit
/// is worse than no count.
pub async fn list_public(
    db: &Database,
    kind: Option<&str>,
    limit: i64,
) -> Result<Vec<PublicDocument>> {
    let limit = limit.clamp(1, 100);
    let columns = "d.id, d.name, d.kind, d.document, p.handle, p.display_name, d.created_at";
    let (sqlite, postgres) = match kind {
        Some(_) => (
            format!(
                "SELECT {columns} FROM extension_documents d JOIN pseuds p ON p.id = d.pseud_id \
                  WHERE d.is_public = 1 AND d.pseud_id IS NOT NULL AND d.kind = ? \
                  ORDER BY d.created_at DESC, d.id LIMIT ?"
            ),
            format!(
                "SELECT {columns} FROM extension_documents d JOIN pseuds p ON p.id = d.pseud_id \
                  WHERE d.is_public = 1 AND d.pseud_id IS NOT NULL AND d.kind = $1 \
                  ORDER BY d.created_at DESC, d.id LIMIT $2"
            ),
        ),
        None => (
            format!(
                "SELECT {columns} FROM extension_documents d JOIN pseuds p ON p.id = d.pseud_id \
                  WHERE d.is_public = 1 AND d.pseud_id IS NOT NULL \
                  ORDER BY d.created_at DESC, d.id LIMIT ?"
            ),
            format!(
                "SELECT {columns} FROM extension_documents d JOIN pseuds p ON p.id = d.pseud_id \
                  WHERE d.is_public = 1 AND d.pseud_id IS NOT NULL \
                  ORDER BY d.created_at DESC, d.id LIMIT $1"
            ),
        ),
    };
    let statement = db.sql(&sqlite, &postgres);
    let rows: Vec<(String, String, String, String, String, String, String)> =
        match (db.backend(), kind) {
            (Backend::Sqlite, Some(kind)) => {
                sqlx::query_as(statement.as_ref())
                    .bind(kind)
                    .bind(limit)
                    .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                    .await?
            }
            (Backend::Sqlite, None) => {
                sqlx::query_as(statement.as_ref())
                    .bind(limit)
                    .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                    .await?
            }
            (Backend::Postgres, Some(kind)) => {
                sqlx::query_as(statement.as_ref())
                    .bind(kind)
                    .bind(limit)
                    .fetch_all(db.postgres_pool().expect("postgres handle"))
                    .await?
            }
            (Backend::Postgres, None) => {
                sqlx::query_as(statement.as_ref())
                    .bind(limit)
                    .fetch_all(db.postgres_pool().expect("postgres handle"))
                    .await?
            }
        };
    Ok(rows
        .into_iter()
        .map(
            |(id, name, kind, document, pseud_handle, pseud_display_name, created_at)| {
                PublicDocument {
                    id,
                    name,
                    kind,
                    document,
                    pseud_handle,
                    pseud_display_name,
                    created_at,
                }
            },
        )
        .collect())
}

/// Delete one of the viewer's own documents. Returns whether a row went.
pub async fn delete(db: &Database, id: &str, account_id: &str) -> Result<bool> {
    let statement = db.sql(
        "DELETE FROM extension_documents WHERE id = ? AND account = ?",
        "DELETE FROM extension_documents WHERE id = $1 AND account::text = $2",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(id)
            .bind(account_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(id)
            .bind(account_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

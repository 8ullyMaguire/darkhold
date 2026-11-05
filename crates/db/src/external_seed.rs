//! M45-67 — external seeds: taste evidence the instance cannot verify, kept honestly.
//!
//! The rule this module exists to hold: **an external seed is never a catalogue row.** A title pasted
//! from Webnovel has no word count, no kudos, no author id — anything this instance reported about it
//! would be invented. So it is stored in `external_seeds` with the words the reader typed, and it can
//! be used as a *seed* (its title's vocabulary ranks the catalogue) without ever being counted,
//! exported or erased as a work.
//!
//! Re-pasting the same list must not accumulate duplicates: `(account_id, spec)` is unique and the
//! insert is idempotent, because a reader who re-runs their list is confirming it, not adding to it.

use crate::{Backend, Database};
use anyhow::Result;
use serde_json::Value as JsonValue;

/// One stored external seed (`(id, account_id, spec, title, source_key, source_url,
/// provenance_json, created_at)`).
type ExternalRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
);

/// A taste claim that is not a catalogue row.
#[derive(Debug, Clone)]
pub struct ExternalSeed {
    pub id: String,
    pub account_id: String,
    /// Exactly what the reader pasted, kept verbatim so the row can explain itself.
    pub spec: String,
    /// The words to rank by: the title, or a URL's last path segment.
    pub title: String,
    /// The archive named by the line, or `unknown` — never a guess.
    pub source_key: String,
    pub source_url: Option<String>,
    /// When it was stored, plus whatever the caller wants to remember about why.
    pub provenance_json: JsonValue,
    pub created_at: String,
}

impl From<ExternalRow> for ExternalSeed {
    fn from(
        (id, account_id, spec, title, source_key, source_url, provenance, created_at): ExternalRow,
    ) -> Self {
        Self {
            id,
            account_id,
            spec,
            title,
            source_key,
            source_url,
            provenance_json: serde_json::from_str(&provenance).unwrap_or(JsonValue::Null),
            created_at,
        }
    }
}

/// One line as the reader gave it, before storage.
///
/// A struct rather than five positional arguments: `remember(db, account, spec, title, source_key,
/// url, provenance, now)` is a call where four of the eight arguments are strings, and swapping two
/// of them compiles silently. The provenance is what makes the row explain itself, so it travels with
/// the claim instead of beside it.
#[derive(Debug, Clone)]
pub struct Claim<'a> {
    /// Exactly what was pasted.
    pub spec: &'a str,
    /// The words to rank by.
    pub title: &'a str,
    /// The archive the line names, or `unknown`.
    pub source_key: &'a str,
    /// The URL, when the line was one.
    pub source_url: Option<&'a str>,
    /// Why this claim exists: what was typed, and why it was not resolved.
    pub provenance: &'a JsonValue,
}

/// Store one line as evidence, returning the row (existing one when the spec was already stored).
///
/// Idempotent by `(account_id, spec)`: the caller may re-run a list without growing the table, and
/// gets back the same id, so a seed reference the reader has already used keeps working.
pub async fn remember(
    db: &Database,
    account_id: &str,
    claim: &Claim<'_>,
    now: &str,
) -> Result<ExternalSeed> {
    if let Some(found) = by_spec(db, account_id, claim.spec).await? {
        return Ok(found);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let provenance = serde_json::to_string(claim.provenance)?;
    let sql = match db.backend() {
        Backend::Sqlite => {
            "INSERT INTO external_seeds (id, account_id, spec, title, source_key, source_url, provenance_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        }
        Backend::Postgres => {
            "INSERT INTO external_seeds (id, account_id, spec, title, source_key, source_url, provenance_json, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
        }
    };
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(sql)
                .bind(&id)
                .bind(account_id)
                .bind(claim.spec)
                .bind(claim.title)
                .bind(claim.source_key)
                .bind(claim.source_url)
                .bind(&provenance)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(sql)
                .bind(&id)
                .bind(account_id)
                .bind(claim.spec)
                .bind(claim.title)
                .bind(claim.source_key)
                .bind(claim.source_url)
                .bind(&provenance)
                .bind(now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    by_spec(db, account_id, claim.spec)
        .await?
        .ok_or_else(|| anyhow::anyhow!("external seed vanished after insert"))
}

/// One row by id, scoped to its owner: evidence belongs to the reader who supplied it.
pub async fn by_id(db: &Database, account_id: &str, id: &str) -> Result<Option<ExternalSeed>> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT id, account_id, spec, title, source_key, source_url, provenance_json, created_at
             FROM external_seeds WHERE id = ? AND account_id = ?"
        }
        Backend::Postgres => {
            "SELECT id, account_id, spec, title, source_key, source_url, provenance_json, created_at
             FROM external_seeds WHERE id = $1 AND account_id = $2"
        }
    };
    let row: Option<ExternalRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(id)
                .bind(account_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(id)
                .bind(account_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(ExternalSeed::from))
}

/// One row by the exact spec that produced it — what makes `remember` idempotent.
pub async fn by_spec(db: &Database, account_id: &str, spec: &str) -> Result<Option<ExternalSeed>> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT id, account_id, spec, title, source_key, source_url, provenance_json, created_at
             FROM external_seeds WHERE account_id = ? AND spec = ?"
        }
        Backend::Postgres => {
            "SELECT id, account_id, spec, title, source_key, source_url, provenance_json, created_at
             FROM external_seeds WHERE account_id = $1 AND spec = $2"
        }
    };
    let row: Option<ExternalRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(account_id)
                .bind(spec)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(account_id)
                .bind(spec)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(ExternalSeed::from))
}

/// Every stored claim this reader has made, newest first.
pub async fn list_for_account(db: &Database, account_id: &str) -> Result<Vec<ExternalSeed>> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT id, account_id, spec, title, source_key, source_url, provenance_json, created_at
             FROM external_seeds WHERE account_id = ? ORDER BY created_at DESC, id DESC"
        }
        Backend::Postgres => {
            "SELECT id, account_id, spec, title, source_key, source_url, provenance_json, created_at
             FROM external_seeds WHERE account_id = $1 ORDER BY created_at DESC, id DESC"
        }
    };
    let rows: Vec<ExternalRow> = match db.backend() {
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
    Ok(rows.into_iter().map(ExternalSeed::from).collect())
}

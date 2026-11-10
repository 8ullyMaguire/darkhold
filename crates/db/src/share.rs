//! Share attribution — a link carries its sharer's code and both sides earn once (M45-41).

use anyhow::Result;
use serde::Serialize;

use crate::{Backend, Database};

/// A minted share link.
#[derive(Debug, Clone, Serialize)]
pub struct ShareCode {
    /// The code the link carries.
    pub code: String,
    /// The work the code names. A code used on another work attributes nothing.
    pub work_id: String,
}

/// The code for one (account, work) pair.
///
/// Derived, not random: two `POST`s that race must not mint two codes for one work, because the same reader's visit to the
/// second link would be a second credit. Deriving it makes minting idempotent without a lock, and the row is still written
/// so the code can be looked up and the pair can be audited.
fn code_for(account: &str, work: &str) -> String {
    let digest = digest_hex(format!("{account}\u{1f}{work}").as_bytes());
    format!("sh_{}", &digest[..24])
}

fn digest_hex(bytes: &[u8]) -> String {
    // No key: this is an identifier, not a secret. A share code is published in a URL and its only job is to be distinct per
    // (account, work); anyone who can read the URL already knows both halves, so a MAC would protect nothing.
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Mint, or re-read, the caller's code for a work.
pub async fn mint(db: &Database, account: &str, work: &str) -> Result<ShareCode> {
    let code = code_for(account, work);
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO share_code (code, account_id, work_id, created_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (account_id, work_id) DO NOTHING",
        "INSERT INTO share_code (code, account_id, work_id, created_at) VALUES ($1, $2::uuid, $3::uuid, $4)
         ON CONFLICT (account_id, work_id) DO NOTHING",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&code)
                .bind(account)
                .bind(work)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&code)
                .bind(account)
                .bind(work)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    // The row that exists is authoritative: a code minted before this build may not be the derived one.
    let stored: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar("SELECT code FROM share_code WHERE account_id = ? AND work_id = ?")
                .bind(account)
                .bind(work)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(
                "SELECT code FROM share_code WHERE account_id = $1::uuid AND work_id = $2::uuid",
            )
            .bind(account)
            .bind(work)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
        }
    };
    Ok(ShareCode {
        code: stored.unwrap_or(code),
        work_id: work.to_owned(),
    })
}

/// Who a code belongs to, and which work it names.
pub async fn owner_of(db: &Database, code: &str) -> Result<Option<(String, String)>> {
    let sql = db.sql(
        "SELECT account_id, work_id FROM share_code WHERE code = ?",
        "SELECT account_id::text AS account_id, work_id::text AS work_id FROM share_code WHERE code = $1",
    );
    let row: Option<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(code)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(code)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(row)
}

/// Record that this visitor arrived through this code. `true` only for the first time.
///
/// The insert is the decision: `ON CONFLICT DO NOTHING` plus `rows_affected` answers "was this visit new?" in one
/// statement, where a `SELECT` then an `INSERT` would let two concurrent visits to a shared link both read "new" and both
/// pay.
pub async fn record_visit(db: &Database, code: &str, visitor: &str) -> Result<bool> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO share_visit (code, visitor_account, credited_at) VALUES (?, ?, ?)
         ON CONFLICT (code, visitor_account) DO NOTHING",
        "INSERT INTO share_visit (code, visitor_account, credited_at) VALUES ($1, $2::uuid, $3)
         ON CONFLICT (code, visitor_account) DO NOTHING",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(code)
            .bind(visitor)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(code)
            .bind(visitor)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

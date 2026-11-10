//! Claims over imported metadata — §E4 (M45-42).

use anyhow::Result;
use serde::Serialize;

use crate::{Backend, Database};

/// A claim on one or more metadata works.
#[derive(Debug, Clone, Serialize)]
pub struct Claim {
    /// The claim's id, which is a capability for its owner only.
    pub id: String,
    /// The string the author pastes into their profile.
    pub token: String,
    /// The URL the token must appear on.
    pub profile_url: String,
    /// `pending` or `verified`.
    pub state: String,
}

/// One work's fate under a claim.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClaimDecision {
    /// The work the decision is about.
    pub work: String,
    /// `claimed`, `already_claimed` or `excluded_by_author`.
    pub outcome: String,
}

fn token_for(account: &str, url: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(format!("{account}\u{1f}{url}").as_bytes());
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    // Derived, not random: the same author claiming the same profile keeps one token, so a page that carries it keeps
    // carrying it. A random token per request would make the author re-paste on every visit and let an old paste look stale.
    format!("dh-claim-{}", &digest[..24])
}

/// Create the claim, or return the one this author already has for this profile, and remember the works it is about.
pub async fn create_or_get(
    db: &Database,
    account: &str,
    profile_url: &str,
    works: &[String],
) -> Result<Claim> {
    let token = token_for(account, profile_url);
    let existing = find_by_profile(db, account, profile_url).await?;
    let claim = match existing {
        Some(claim) => claim,
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            let now = crate::identity::now_rfc3339();
            let sql = db.sql(
                "INSERT INTO work_claim (id, account_id, profile_url, token, state, created_at)
                 VALUES (?, ?, ?, ?, 'pending', ?)",
                "INSERT INTO work_claim (id, account_id, profile_url, token, state, created_at)
                 VALUES ($1::uuid, $2::uuid, $3, $4, 'pending', $5)",
            );
            match db.backend() {
                Backend::Sqlite => {
                    sqlx::query(&sql)
                        .bind(&id)
                        .bind(account)
                        .bind(profile_url)
                        .bind(&token)
                        .bind(&now)
                        .execute(db.sqlite_pool().expect("sqlite"))
                        .await?;
                }
                Backend::Postgres => {
                    sqlx::query(&sql)
                        .bind(&id)
                        .bind(account)
                        .bind(profile_url)
                        .bind(&token)
                        .bind(&now)
                        .execute(db.postgres_pool().expect("postgres"))
                        .await?;
                }
            }
            Claim {
                id,
                token,
                profile_url: profile_url.to_owned(),
                state: "pending".to_owned(),
            }
        }
    };
    let now = crate::identity::now_rfc3339();
    for work in works {
        let sql = db.sql(
            "INSERT INTO work_claim_work (claim_id, work_id, outcome, decided_at) VALUES (?, ?, NULL, NULL)
             ON CONFLICT (claim_id, work_id) DO NOTHING",
            "INSERT INTO work_claim_work (claim_id, work_id, outcome, decided_at) VALUES ($1::uuid, $2::uuid, NULL, NULL)
             ON CONFLICT (claim_id, work_id) DO NOTHING",
        );
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(&sql)
                    .bind(&claim.id)
                    .bind(work)
                    .execute(db.sqlite_pool().expect("sqlite"))
                    .await?;
            }
            Backend::Postgres => {
                sqlx::query(&sql)
                    .bind(&claim.id)
                    .bind(work)
                    .execute(db.postgres_pool().expect("postgres"))
                    .await?;
            }
        }
    }
    let _ = now;
    Ok(claim)
}

async fn find_by_profile(db: &Database, account: &str, url: &str) -> Result<Option<Claim>> {
    let sql = db.sql(
        "SELECT id, token, profile_url, state FROM work_claim WHERE account_id = ? AND profile_url = ?",
        "SELECT id::text AS id, token, profile_url, state FROM work_claim WHERE account_id = $1::uuid AND profile_url = $2",
    );
    let row: Option<(String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account)
                .bind(url)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account)
                .bind(url)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(row.map(|(id, token, profile_url, state)| Claim {
        id,
        token,
        profile_url,
        state,
    }))
}

/// The claim, if it is this account's. Someone else's claim is `None`, not a refusal with a different code.
pub async fn find_owned(db: &Database, id: &str, account: &str) -> Result<Option<Claim>> {
    let sql = db.sql(
        "SELECT id, token, profile_url, state FROM work_claim WHERE id = ? AND account_id = ?",
        "SELECT id::text AS id, token, profile_url, state FROM work_claim WHERE id = $1::uuid AND account_id = $2::uuid",
    );
    let row: Option<(String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(id)
                .bind(account)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(id)
                .bind(account)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(row.map(|(id, token, profile_url, state)| Claim {
        id,
        token,
        profile_url,
        state,
    }))
}

/// The works this claim is about and what has been decided about each, in a stable order.
pub async fn works_of(db: &Database, claim_id: &str) -> Result<Vec<ClaimDecision>> {
    let sql = db.sql(
        "SELECT work_id, outcome FROM work_claim_work WHERE claim_id = ? ORDER BY work_id",
        "SELECT work_id::text AS work_id, outcome FROM work_claim_work WHERE claim_id = $1::uuid ORDER BY work_id",
    );
    let rows: Vec<(String, Option<String>)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(claim_id)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(claim_id)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(work, outcome)| ClaimDecision {
            work,
            outcome: outcome.unwrap_or_else(|| "pending".to_owned()),
        })
        .collect())
}

/// Record what happened to one work under this claim.
pub async fn set_outcome(db: &Database, claim_id: &str, work: &str, outcome: &str) -> Result<()> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "UPDATE work_claim_work SET outcome = ?, decided_at = ? WHERE claim_id = ? AND work_id = ?",
        "UPDATE work_claim_work SET outcome = $1, decided_at = $2 WHERE claim_id = $3::uuid AND work_id = $4::uuid",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(outcome)
                .bind(&now)
                .bind(claim_id)
                .bind(work)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(outcome)
                .bind(&now)
                .bind(claim_id)
                .bind(work)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(())
}

/// Hand the work to this pseud. One statement, so a work cannot be marked claimed without a new owner.
pub async fn attach(db: &Database, work: &str, pseud: &str) -> Result<()> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "UPDATE works SET owner_pseud_id = ?, updated_at = ?, version = version + 1 WHERE id = ?",
        "UPDATE works SET owner_pseud_id = $1::uuid, updated_at = $2, version = version + 1 WHERE id = $3::uuid",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(pseud)
                .bind(&now)
                .bind(work)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(pseud)
                .bind(&now)
                .bind(work)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(())
}

/// Mark the claim verified.
pub async fn mark_verified(db: &Database, claim_id: &str) -> Result<()> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "UPDATE work_claim SET state = 'verified', verified_at = ? WHERE id = ?",
        "UPDATE work_claim SET state = 'verified', verified_at = $1 WHERE id = $2::uuid",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&now)
                .bind(claim_id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&now)
                .bind(claim_id)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(())
}

/// Whether the work is still a metadata row — owned by the instance's system account — rather than somebody's.
pub async fn is_metadata_work(db: &Database, work: &str) -> Result<bool> {
    let system = crate::system_account();
    let sql = db.sql(
        "SELECT COUNT(*) FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id
          WHERE w.id = ? AND p.account_id = ?",
        "SELECT CAST(COUNT(*) AS BIGINT) FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id
          WHERE w.id = $1::uuid AND p.account_id = $2::uuid",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(work)
                .bind(&system)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(work)
                .bind(&system)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(count > 0)
}

/// The pseud a claim attaches works to: the account's first pseud by handle, so the choice is deterministic.
pub async fn primary_pseud(db: &Database, account: &str) -> Result<Option<String>> {
    let sql = db.sql(
        "SELECT id FROM pseuds WHERE account_id = ? ORDER BY handle LIMIT 1",
        "SELECT id::text FROM pseuds WHERE account_id = $1::uuid ORDER BY handle LIMIT 1",
    );
    let pseud: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(account)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(account)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(pseud)
}

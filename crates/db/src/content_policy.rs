//! The instance-level content policy: M69-16's middle layer.
//!
//! Content filtering has three layers, and they are different kinds of thing, which is why only one of them is a table:
//!
//! 1. **hardcoded** — `lifecycle = 'published' AND visibility = 'public' AND deleted_at IS NULL`, carried by every read in this
//!    tree. No configuration can reach it. That is what makes it hardcoded, and it is why there is no code in *this* module for
//!    it: a "hardcoded blocklist" in a table would be a list an operator could edit, which is the opposite of hardcoded.
//! 2. **policy** — this module. An operator blocks a taxonomy value for the whole instance and says why. Read by
//!    `search::content_filter_sql::for_pseud`, so it reaches every query, feed, recommendation and notification pipeline the way
//!    the reader's own filters do: one 'somebody decided this' rather than one per surface.
//! 3. **taste** — `content_filters`, per pseud. Untouched here.
//!
//! A block carries a **reason** and is **revoked**, never deleted. The reason is required because a policy block is an instance
//! taking a decision about what its readers may see, and a row that cannot say why cannot be defended when someone asks. Revoking
//! rather than deleting keeps the record: the audit trail for "we hid this for a month" is the row itself.

use crate::{identity::now_rfc3339, Backend, Database, Result};

/// One live policy block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyBlock {
    /// Taxonomy kind, e.g. `tag` or `fandom`.
    pub filter_type: String,
    /// The value blocked, in the taxonomy node's canonical form.
    pub value: String,
    /// Why the instance blocks it. Required.
    pub reason: String,
}

/// Why a block was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockRefusal {
    /// The reason was blank or missing.
    EmptyReason,
    /// There is no live block for the rule an override would exempt.
    NotFound,
    /// The type or value was blank.
    EmptyTarget,
    /// A live block already covers this (type, value).
    AlreadyBlocked,
}

/// Add a policy block. Returns the refusal rather than a database error, because each refusal is something an operator can fix.
pub async fn block(
    db: &Database,
    filter_type: &str,
    value: &str,
    reason: &str,
    created_by: &str,
) -> Result<std::result::Result<(), BlockRefusal>> {
    let filter_type = filter_type.trim();
    let value = value.trim();
    let reason = reason.trim();
    if filter_type.is_empty() || value.is_empty() {
        return Ok(Err(BlockRefusal::EmptyTarget));
    }
    if reason.is_empty() {
        return Ok(Err(BlockRefusal::EmptyReason));
    }

    let exists = db.sql(
        "SELECT COUNT(*) FROM content_policy_blocks
          WHERE filter_type = ? AND value = ? AND revoked_at IS NULL",
        "SELECT COUNT(*) FROM content_policy_blocks
          WHERE filter_type = $1 AND value = $2 AND revoked_at IS NULL",
    );
    let live: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&exists)
                .bind(filter_type)
                .bind(value)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&exists)
                .bind(filter_type)
                .bind(value)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    if live > 0 {
        return Ok(Err(BlockRefusal::AlreadyBlocked));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let sql = db.sql(
        "INSERT INTO content_policy_blocks (id, filter_type, value, reason, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
        "INSERT INTO content_policy_blocks (id, filter_type, value, reason, created_by, created_at)
         VALUES (?::uuid, ?, ?, ?, ?::uuid, ?)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(filter_type)
                .bind(value)
                .bind(reason)
                .bind(created_by)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(filter_type)
                .bind(value)
                .bind(reason)
                .bind(created_by)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(Ok(()))
}

/// Revoke a live block, recording who and why. Returns whether a live block matched.
///
/// The revocation reason is required for the same reason the block's is: the record of *why we stopped hiding this* is as
/// load-bearing as the record of why we started, and a bare timestamp answers neither question.
pub async fn revoke(
    db: &Database,
    filter_type: &str,
    value: &str,
    reason: &str,
    revoked_by: &str,
) -> Result<std::result::Result<bool, BlockRefusal>> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Ok(Err(BlockRefusal::EmptyReason));
    }
    let now = now_rfc3339();
    let sql = db.sql(
        "UPDATE content_policy_blocks
            SET revoked_at = ?, revoked_by = ?, revoked_reason = ?
          WHERE filter_type = ? AND value = ? AND revoked_at IS NULL",
        "UPDATE content_policy_blocks
            SET revoked_at = $1, revoked_by = $2::uuid, revoked_reason = $3
          WHERE filter_type = $4 AND value = $5 AND revoked_at IS NULL",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(&now)
            .bind(revoked_by)
            .bind(reason)
            .bind(filter_type)
            .bind(value)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(&now)
            .bind(revoked_by)
            .bind(reason)
            .bind(filter_type)
            .bind(value)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(Ok(affected > 0))
}

/// Every live block, ordered so a page is stable.
pub async fn live(db: &Database) -> Result<Vec<PolicyBlock>> {
    let sql = db.sql(
        "SELECT filter_type, value, reason FROM content_policy_blocks
          WHERE revoked_at IS NULL ORDER BY filter_type, value",
        "SELECT filter_type, value, reason FROM content_policy_blocks
          WHERE revoked_at IS NULL ORDER BY filter_type, value",
    );
    let rows: Vec<(String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(filter_type, value, reason)| PolicyBlock {
            filter_type,
            value,
            reason,
        })
        .collect())
}

/// One line of the policy trail: what was blocked, why, and whether and why it was lifted.
pub type TrailRow = (String, String, String, Option<String>, Option<String>);

/// The full audit trail, revoked blocks included, newest first.
pub async fn history(db: &Database) -> Result<Vec<TrailRow>> {
    let sql = db.sql(
        "SELECT filter_type, value, reason, revoked_at, revoked_reason
           FROM content_policy_blocks ORDER BY created_at DESC, filter_type, value",
        "SELECT filter_type, value, reason, revoked_at::text, revoked_reason
           FROM content_policy_blocks ORDER BY created_at DESC, filter_type, value",
    );
    let rows: Vec<TrailRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

/// Grant a work an exception to one live policy block (M69-16's last clause).
///
/// The override names the block it exempts, so an exception for one rule cannot silently cover a later block on the same work.
/// A reason is required for the same reason the block's is; and there must **be** a live block to exempt, otherwise the row would
/// authorise something nobody blocked and would sit in the trail looking like a decision.
pub async fn grant_override(
    db: &Database,
    work_id: &str,
    filter_type: &str,
    value: &str,
    reason: &str,
    granted_by: &str,
) -> Result<std::result::Result<(), BlockRefusal>> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Ok(Err(BlockRefusal::EmptyReason));
    }
    if work_id.trim().is_empty() || filter_type.trim().is_empty() || value.trim().is_empty() {
        return Ok(Err(BlockRefusal::EmptyTarget));
    }
    let live_check = db.sql(
        "SELECT COUNT(*) FROM content_policy_blocks
          WHERE filter_type = ? AND value = ? AND revoked_at IS NULL",
        "SELECT COUNT(*) FROM content_policy_blocks
          WHERE filter_type = $1 AND value = $2 AND revoked_at IS NULL",
    );
    let blocks: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&live_check)
                .bind(filter_type)
                .bind(value)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&live_check)
                .bind(filter_type)
                .bind(value)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    if blocks == 0 {
        return Ok(Err(BlockRefusal::NotFound));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let sql = db.sql(
        "INSERT INTO content_policy_overrides (id, work_id, filter_type, value, reason, granted_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (work_id, filter_type, value) WHERE revoked_at IS NULL DO NOTHING",
        "INSERT INTO content_policy_overrides (id, work_id, filter_type, value, reason, granted_by, created_at)
         VALUES (?::uuid, ?::uuid, ?, ?, ?, ?::uuid, ?)
         ON CONFLICT (work_id, filter_type, value) WHERE revoked_at IS NULL DO NOTHING",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(&id)
            .bind(work_id)
            .bind(filter_type)
            .bind(value)
            .bind(reason)
            .bind(granted_by)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(&id)
            .bind(work_id)
            .bind(filter_type)
            .bind(value)
            .bind(reason)
            .bind(granted_by)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    if affected == 0 {
        return Ok(Err(BlockRefusal::AlreadyBlocked));
    }
    Ok(Ok(()))
}

/// Revoke a work's exception. Returns whether a live one matched.
pub async fn revoke_override(
    db: &Database,
    work_id: &str,
    filter_type: &str,
    value: &str,
    reason: &str,
    revoked_by: &str,
) -> Result<std::result::Result<bool, BlockRefusal>> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Ok(Err(BlockRefusal::EmptyReason));
    }
    let now = now_rfc3339();
    let sql = db.sql(
        "UPDATE content_policy_overrides
            SET revoked_at = ?, revoked_by = ?, revoked_reason = ?
          WHERE work_id = ? AND filter_type = ? AND value = ? AND revoked_at IS NULL",
        "UPDATE content_policy_overrides
            SET revoked_at = $1, revoked_by = $2::uuid, revoked_reason = $3
          WHERE work_id = $4::uuid AND filter_type = $5 AND value = $6 AND revoked_at IS NULL",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(&now)
            .bind(revoked_by)
            .bind(reason)
            .bind(work_id)
            .bind(filter_type)
            .bind(value)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(&now)
            .bind(revoked_by)
            .bind(reason)
            .bind(work_id)
            .bind(filter_type)
            .bind(value)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(Ok(affected > 0))
}

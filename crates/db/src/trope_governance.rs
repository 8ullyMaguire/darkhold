//! Community trope proposals (M45-77). Both dialects.
//! SQL is written with `?` and rewritten by [`Database::sql`].

use anyhow::{anyhow, Result};
use darkhold_domain::trope_governance::{proposal_decided, valid_slug, TropeAction};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{Backend, Database};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TropeProposal {
    pub id: String,
    pub slug: String,
    pub action: String,
    pub payload: String,
    pub status: String,
    pub yes_votes: i64,
    pub no_votes: i64,
    pub quorum_needed: i64,
    pub closes_at: String,
    pub proposed_by: String,
    pub decided_by: Option<String>,
    pub decision_reason: Option<String>,
    pub created_at: String,
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalPayload {
    pub label: String,
    pub summary: String,
    pub members: Vec<String>,
    /// An update proposal has its own slug; this names the group it edits.
    pub target_slug: Option<String>,
    /// The active row's `created_at`, used as an optimistic version token.
    pub target_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SettleReport {
    pub scanned: i64,
    pub expired: i64,
    pub applied: i64,
    pub remaining: i64,
}

fn parse_payload(raw: &str) -> Result<ProposalPayload> {
    serde_json::from_str(raw).map_err(|e| anyhow!("malformed trope proposal payload: {e}"))
}

/// Apply an agreed proposal. Called inside the caller's transaction so a vote
/// that passes can never leave a decision without its active group.
macro_rules! apply_in_tx {
    ($db:expr, $tx:expr, $proposal:expr, $now:expr) => {{
        let proposal = $proposal;
        let payload = parse_payload(&proposal.payload)?;
        if !valid_slug(&proposal.slug) || payload.members.is_empty() || payload.members.len() > 10 {
            return Err(anyhow!("a trope group needs 1..10 members and a closed slug"));
        }
        let action = TropeAction::parse(&proposal.action)
            .ok_or_else(|| anyhow!("unknown trope proposal action: {}", proposal.action))?;
        let target = payload.target_slug.as_deref().unwrap_or(&proposal.slug);
        let active: Option<TropeProposal> = sqlx::query_as(&$db.sql(
            "SELECT * FROM trope_proposals WHERE slug = ? AND status = 'active'",
            "SELECT id::text AS id, slug, action, payload, status, yes_votes::bigint AS yes_votes, no_votes::bigint AS no_votes,
                quorum_needed::bigint AS quorum_needed, closes_at::text AS closes_at, proposed_by::text AS proposed_by,
                decided_by::text AS decided_by, decision_reason, created_at::text AS created_at,
                decided_at::text AS decided_at
            FROM trope_proposals WHERE slug = ? AND status = 'active'",
        ))
        .bind(target)
        .fetch_optional(&mut *$tx)
        .await?;
        match action {
            TropeAction::Create => {
                if active.is_some() {
                    return Err(anyhow!("a proposal or active group already uses this slug"));
                }
                let id = uuid::Uuid::new_v4().to_string();
                sqlx::query(&$db.sql(
                    "INSERT INTO trope_proposals
                     (id, slug, action, payload, status, quorum_needed, closes_at, proposed_by, created_at)
                     VALUES (?, ?, ?, ?, 'active', ?, ?, ?, ?)",
                    "INSERT INTO trope_proposals
                     (id, slug, action, payload, status, quorum_needed, closes_at, proposed_by, created_at)
                     VALUES (?::uuid, ?, ?, ?, 'active', ?, ?::timestamptz, ?::uuid, ?::timestamptz)",
                ))
                .bind(&id)
                .bind(target)
                .bind(action.as_str())
                .bind(&proposal.payload)
                .bind(proposal.quorum_needed)
                .bind(&proposal.closes_at)
                .bind(&proposal.proposed_by)
                .bind($now)
                .execute(&mut *$tx)
                .await?;
            }
            TropeAction::Update => {
                let current = active.ok_or_else(|| anyhow!("no active group to update"))?;
                if payload.target_version.as_deref() != Some(current.created_at.as_str()) {
                    return Err(anyhow!("target version does not match the active group"));
                }
                sqlx::query(&$db.sql(
                    "UPDATE trope_proposals SET payload = ?, action = 'update' WHERE id = ?",
                    "UPDATE trope_proposals SET payload = ?, action = 'update' WHERE id = ?::uuid",
                ))
                .bind(&proposal.payload)
                .bind(&current.id)
                .execute(&mut *$tx)
                .await?;
            }
            TropeAction::Retire => {
                let current = active.ok_or_else(|| anyhow!("no active group to retire"))?;
                sqlx::query(&$db.sql(
                    "UPDATE trope_proposals SET status = 'retired', decided_at = ? WHERE id = ?",
                    "UPDATE trope_proposals SET status = 'retired', decided_at = ?::timestamptz WHERE id = ?::uuid",
                ))
                .bind($now)
                .bind(&current.id)
                .execute(&mut *$tx)
                .await?;
            }
        }
    }};
}

pub async fn insert_proposal(
    db: &Database,
    slug: &str,
    action: &str,
    payload: &str,
    quorum: i64,
    closes_at: &str,
    proposed_by: &str,
    now: &str,
) -> Result<String> {
    if !valid_slug(slug) || TropeAction::parse(action).is_none() {
        return Err(anyhow!("invalid trope slug or action"));
    }
    parse_payload(payload)?;
    let id = uuid::Uuid::new_v4().to_string();
    let sql = db.sql(
        "INSERT INTO trope_proposals
         (id, slug, action, payload, status, quorum_needed, closes_at, proposed_by, created_at)
         VALUES (?, ?, ?, ?, 'proposed', ?, ?, ?, ?)",
        "INSERT INTO trope_proposals
         (id, slug, action, payload, status, quorum_needed, closes_at, proposed_by, created_at)
         VALUES (?::uuid, ?, ?, ?, 'proposed', ?, ?::timestamptz, ?::uuid, ?::timestamptz)",
    );
    // The two drivers' QueryResult types differ, so each arm classifies its
    // own error instead of trying to unify them in one match expression.
    fn classify(e: sqlx::Error) -> Result<String> {
        let text = e.to_string().to_lowercase();
        if text.contains("unique") || text.contains("constraint") {
            Err(anyhow!("a proposal or active group already uses this slug"))
        } else {
            Err(e.into())
        }
    }
    match db.backend() {
        Backend::Sqlite => match sqlx::query(&sql)
            .bind(&id)
            .bind(slug)
            .bind(action)
            .bind(payload)
            .bind(quorum)
            .bind(closes_at)
            .bind(proposed_by)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await
        {
            Ok(_) => Ok(id),
            Err(e) => classify(e),
        },
        Backend::Postgres => match sqlx::query(&sql)
            .bind(&id)
            .bind(slug)
            .bind(action)
            .bind(payload)
            .bind(quorum)
            .bind(closes_at)
            .bind(proposed_by)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await
        {
            Ok(_) => Ok(id),
            Err(e) => classify(e),
        },
    }
}

pub async fn get_proposal(db: &Database, id: &str) -> Result<Option<TropeProposal>> {
    let sql = db.sql(
        "SELECT * FROM trope_proposals WHERE id = ?",
        "SELECT id::text AS id, slug, action, payload, status, yes_votes::bigint AS yes_votes, no_votes::bigint AS no_votes,
                quorum_needed::bigint AS quorum_needed, closes_at::text AS closes_at, proposed_by::text AS proposed_by,
                decided_by::text AS decided_by, decision_reason, created_at::text AS created_at,
                decided_at::text AS decided_at
            FROM trope_proposals WHERE id = ?::uuid",
    );
    Ok(match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, TropeProposal>(&sql)
                .bind(id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, TropeProposal>(&sql)
                .bind(id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    })
}

pub async fn list_proposals(
    db: &Database,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<TropeProposal>> {
    let (sql, with_status) = if status.is_some() {
        (
            db.sql(
                "SELECT * FROM trope_proposals WHERE status = ? ORDER BY created_at DESC LIMIT ?",
                "SELECT id::text AS id, slug, action, payload, status, yes_votes::bigint AS yes_votes, no_votes::bigint AS no_votes,
                quorum_needed::bigint AS quorum_needed, closes_at::text AS closes_at, proposed_by::text AS proposed_by,
                decided_by::text AS decided_by, decision_reason, created_at::text AS created_at,
                decided_at::text AS decided_at
            FROM trope_proposals WHERE status = ? ORDER BY created_at DESC LIMIT ?",
            ),
            true,
        )
    } else {
        (
            db.sql(
                "SELECT * FROM trope_proposals ORDER BY created_at DESC LIMIT ?",
                "SELECT id::text AS id, slug, action, payload, status, yes_votes::bigint AS yes_votes, no_votes::bigint AS no_votes,
                quorum_needed::bigint AS quorum_needed, closes_at::text AS closes_at, proposed_by::text AS proposed_by,
                decided_by::text AS decided_by, decision_reason, created_at::text AS created_at,
                decided_at::text AS decided_at
            FROM trope_proposals ORDER BY created_at DESC LIMIT ?",
            ),
            false,
        )
    };
    Ok(match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as::<_, TropeProposal>(&sql);
            if with_status {
                query = query.bind(status.expect("checked"));
            }
            query
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as::<_, TropeProposal>(&sql);
            if with_status {
                query = query.bind(status.expect("checked"));
            }
            query
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    })
}

pub async fn insert_vote(
    db: &Database,
    proposal_id: &str,
    account_id: &str,
    value: &str,
    now: &str,
) -> Result<bool> {
    if value != "yes" && value != "no" {
        return Err(anyhow!("a trope vote is yes or no"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let sql = db.sql(
        "INSERT INTO trope_votes (id, proposal_id, account_id, value, created_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(proposal_id, account_id) DO NOTHING",
        "INSERT INTO trope_votes (id, proposal_id, account_id, value, created_at)
         VALUES (?::uuid, ?::uuid, ?::uuid, ?, ?::timestamptz)
         ON CONFLICT(proposal_id, account_id) DO NOTHING",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(&id)
            .bind(proposal_id)
            .bind(account_id)
            .bind(value)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(&id)
            .bind(proposal_id)
            .bind(account_id)
            .bind(value)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected == 1)
}

/// Recount the votes, decide once, and apply an approved proposal in the same
/// transaction. A stale or malformed application rolls the whole decision back.
pub async fn recount_and_decide(
    db: &Database,
    proposal_id: &str,
    now: &str,
    decider: &str,
) -> Result<Option<bool>> {
    let sql = db.sql(
        "UPDATE trope_proposals
         SET yes_votes = (SELECT COUNT(*) FROM trope_votes WHERE proposal_id = ? AND value = 'yes'),
             no_votes = (SELECT COUNT(*) FROM trope_votes WHERE proposal_id = ? AND value = 'no')
         WHERE id = ?",
        "UPDATE trope_proposals
         SET yes_votes = (SELECT COUNT(*) FROM trope_votes WHERE proposal_id = ?::uuid AND value = 'yes'),
             no_votes = (SELECT COUNT(*) FROM trope_votes WHERE proposal_id = ?::uuid AND value = 'no')
         WHERE id = ?::uuid",
    );
    let read = db.sql(
        "SELECT * FROM trope_proposals WHERE id = ?",
        "SELECT id::text AS id, slug, action, payload, status, yes_votes::bigint AS yes_votes, no_votes::bigint AS no_votes,
                quorum_needed::bigint AS quorum_needed, closes_at::text AS closes_at, proposed_by::text AS proposed_by,
                decided_by::text AS decided_by, decision_reason, created_at::text AS created_at,
                decided_at::text AS decided_at
            FROM trope_proposals WHERE id = ?::uuid",
    );
    let decide_sql = db.sql(
        "UPDATE trope_proposals SET status = ?, decided_by = ?, decided_at = ? WHERE id = ?",
        "UPDATE trope_proposals SET status = ?, decided_by = ?::uuid, decided_at = ?::timestamptz WHERE id = ?::uuid",
    );
    match db.backend() {
        Backend::Sqlite => {
            let mut tx = db.sqlite_pool().expect("sqlite handle").begin().await?;
            sqlx::query(&sql)
                .bind(proposal_id)
                .bind(proposal_id)
                .bind(proposal_id)
                .execute(&mut *tx)
                .await?;
            let proposal: TropeProposal = sqlx::query_as::<_, TropeProposal>(&read)
                .bind(proposal_id)
                .fetch_one(&mut *tx)
                .await?;
            let decided = proposal_decided(
                proposal.yes_votes as u32,
                proposal.no_votes as u32,
                proposal.quorum_needed as u32,
            );
            if let Some(passed) = decided {
                let status = if passed { "passed" } else { "failed" };
                sqlx::query(&decide_sql)
                    .bind(status)
                    .bind(decider)
                    .bind(now)
                    .bind(proposal_id)
                    .execute(&mut *tx)
                    .await?;
                if passed {
                    apply_in_tx!(db, tx, proposal, now);
                }
            }
            tx.commit().await?;
            Ok(decided)
        }
        Backend::Postgres => {
            let mut tx = db.postgres_pool().expect("postgres handle").begin().await?;
            sqlx::query(&sql)
                .bind(proposal_id)
                .bind(proposal_id)
                .bind(proposal_id)
                .execute(&mut *tx)
                .await?;
            let proposal: TropeProposal = sqlx::query_as::<_, TropeProposal>(&read)
                .bind(proposal_id)
                .fetch_one(&mut *tx)
                .await?;
            let decided = proposal_decided(
                proposal.yes_votes as u32,
                proposal.no_votes as u32,
                proposal.quorum_needed as u32,
            );
            if let Some(passed) = decided {
                let status = if passed { "passed" } else { "failed" };
                sqlx::query(&decide_sql)
                    .bind(status)
                    .bind(decider)
                    .bind(now)
                    .bind(proposal_id)
                    .execute(&mut *tx)
                    .await?;
                if passed {
                    apply_in_tx!(db, tx, proposal, now);
                }
            }
            tx.commit().await?;
            Ok(decided)
        }
    }
}

/// The groups the search resolver consumes. Members come from the same JSON
/// payload the route wrote; there is no second group table.
pub async fn active_community_groups(db: &Database) -> Result<Vec<(String, Vec<String>)>> {
    let sql = db.sql(
        "SELECT slug, payload FROM trope_proposals WHERE status = 'active' ORDER BY slug",
        "SELECT slug, payload FROM trope_proposals WHERE status = 'active' ORDER BY slug",
    );
    let rows: Vec<(String, String)> = match db.backend() {
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
    rows.into_iter()
        .map(|(slug, raw)| {
            let payload = parse_payload(&raw)?;
            Ok((slug, payload.members))
        })
        .collect()
}

macro_rules! settle_in_backend {
    ($db:expr, $pool:expr, $limit:expr, $now:expr) => {{
        let old: Vec<TropeProposal> = sqlx::query_as(&$db.sql(
            "SELECT * FROM trope_proposals WHERE status = 'proposed' AND closes_at < ? ORDER BY closes_at LIMIT ?",
            "SELECT id::text AS id, slug, action, payload, status, yes_votes::bigint AS yes_votes, no_votes::bigint AS no_votes,
                quorum_needed::bigint AS quorum_needed, closes_at::text AS closes_at, proposed_by::text AS proposed_by,
                decided_by::text AS decided_by, decision_reason, created_at::text AS created_at,
                decided_at::text AS decided_at
            FROM trope_proposals WHERE status = 'proposed' AND closes_at < ?::timestamptz ORDER BY closes_at LIMIT ?",
        ))
        .bind($now)
        .bind($limit)
        .fetch_all($pool)
        .await?;
        let scanned = old.len() as i64;
        let mut expired = 0_i64;
        let mut applied = 0_i64;
        for row in old {
            sqlx::query(&$db.sql(
                "UPDATE trope_proposals SET status = 'expired', decided_at = ?, decision_reason = 'expired' WHERE id = ?",
                "UPDATE trope_proposals SET status = 'expired', decided_at = ?::timestamptz, decision_reason = 'expired' WHERE id = ?::uuid",
            ))
            .bind($now)
            .bind(&row.id)
            .execute($pool)
            .await?;
            expired += 1;
        }
        let passed: Vec<TropeProposal> = sqlx::query_as(&$db.sql(
            "SELECT * FROM trope_proposals WHERE status = 'passed' ORDER BY created_at",
            "SELECT id::text AS id, slug, action, payload, status, yes_votes::bigint AS yes_votes, no_votes::bigint AS no_votes,
                quorum_needed::bigint AS quorum_needed, closes_at::text AS closes_at, proposed_by::text AS proposed_by,
                decided_by::text AS decided_by, decision_reason, created_at::text AS created_at,
                decided_at::text AS decided_at
            FROM trope_proposals WHERE status = 'passed' ORDER BY created_at",
        ))
        .fetch_all($pool)
        .await?;
        for row in passed {
            let active: Option<TropeProposal> = sqlx::query_as(&$db.sql(
                "SELECT * FROM trope_proposals WHERE slug = ? AND status = 'active'",
                "SELECT id::text AS id, slug, action, payload, status, yes_votes::bigint AS yes_votes, no_votes::bigint AS no_votes,
                quorum_needed::bigint AS quorum_needed, closes_at::text AS closes_at, proposed_by::text AS proposed_by,
                decided_by::text AS decided_by, decision_reason, created_at::text AS created_at,
                decided_at::text AS decided_at
            FROM trope_proposals WHERE slug = ? AND status = 'active'",
            ))
            .bind(&row.slug)
            .fetch_optional($pool)
            .await?;
            if active.is_none() {
                let mut tx = $pool.begin().await?;
                apply_in_tx!($db, tx, row, $now);
                tx.commit().await?;
                applied += 1;
            }
        }
        (scanned, expired, applied)
    }};
}

pub async fn settle_expired(db: &Database, limit: i64, now: &str) -> Result<SettleReport> {
    let (scanned, expired, applied) = match db.backend() {
        Backend::Sqlite => {
            let pool = db.sqlite_pool().expect("sqlite handle");
            settle_in_backend!(db, pool, limit, now)
        }
        Backend::Postgres => {
            let pool = db.postgres_pool().expect("postgres handle");
            settle_in_backend!(db, pool, limit, now)
        }
    };
    let sql = db.sql(
        "SELECT COUNT(*) FROM trope_proposals WHERE status = 'proposed'",
        "SELECT COUNT(*) FROM trope_proposals WHERE status = 'proposed'",
    );
    let remaining: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(SettleReport {
        scanned,
        expired,
        applied,
        remaining,
    })
}

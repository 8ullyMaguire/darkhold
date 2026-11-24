//! Trope proposal authority (M45-77 Step 7).
//!
//! The caller supplies the account id from the session; a request body cannot
//! claim someone else's role. Curator status is checked before trust so a
//! TL2 curator stays a curator, and an operator is only labelled as one — it
//! grants no extra vote.

use darkhold_db::{media_resilience, Backend, Database};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TropeRole {
    Curator,
    Trusted,
    Operator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct TropeAuthority {
    pub allowed: bool,
    pub role: Option<TropeRole>,
    pub level: i64,
}

pub async fn for_account(
    db: &Database,
    cfg: &crate::config::TropesConfig,
    account_id: &str,
) -> Result<TropeAuthority, sqlx::Error> {
    let level = darkhold_db::governance::trust_for(db, account_id).await?;
    let sql = db.sql(
        "SELECT coalesce(is_curator, false) AS is_curator FROM accounts WHERE id = ?",
        "SELECT coalesce(is_curator, false) AS is_curator FROM accounts WHERE id = ?::uuid",
    );
    let is_curator = match db.backend() {
        Backend::Sqlite => sqlx::query_scalar::<_, bool>(&sql)
            .bind(account_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .unwrap_or(false),
        Backend::Postgres => sqlx::query_scalar::<_, bool>(&sql)
            .bind(account_id)
            .fetch_optional(db.postgres_pool().expect("postgres handle"))
            .await?
            .unwrap_or(false),
    };
    let active = media_resilience::is_active_curator(db, account_id).await?;
    let role = if is_curator || active {
        Some(TropeRole::Curator)
    } else if level >= 5 {
        Some(TropeRole::Operator)
    } else if level >= cfg.proposal_min_trust {
        Some(TropeRole::Trusted)
    } else {
        None
    };
    Ok(TropeAuthority {
        allowed: role.is_some(),
        role,
        level,
    })
}

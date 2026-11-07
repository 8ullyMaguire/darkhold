//! Counting what the moderation ladder measures (W7 task 4, step 2).
//!
//! Deliberately conservative: **a contribution counts only if it happened after the ladder's activation**. Measured on
//! 2026-11-07 this instance holds 58,042 published works owned by three pseuds — 58,035 by `devwriter`, the import corpus —
//! and `works` carries no provenance column separating an imported work from an authored one (`import_units` links to
//! `library_item_id`, not `work_id`). Counting published works without a cutover would mint `role:moderator:<site>` to
//! whoever ran the importer on the first sweep: authority granted by an import, one query away.
//!
//! The cutoff is therefore the safety property, not a refinement. History is excluded because it cannot be attributed; the
//! ladder starts empty and earns from actions taken while it was running.
//!
//! Named `moderation_ladder` and not `ladder`, because `crate::ladder` already means the *taste* ladder (Glicko-2 eligibility)
//! in this crate. Two ladders, one word, and the qualified name is cheaper than the confusion.

use darkhold_domain::moderation_ladder::Counts;

use crate::{Backend, Database};

/// This account's contributions since `cutoff` (RFC 3339), and the reports against them that were upheld.
pub async fn counts_for(db: &Database, account: &str, cutoff: &str) -> Result<Counts, sqlx::Error> {
    let (accepted, upheld): (i64, i64) = match db.backend() {
        Backend::Sqlite => {
            let accepted = sqlx::query_scalar(
                "SELECT COUNT(*) FROM works w
                   JOIN pseuds p ON p.id = w.owner_pseud_id
                  WHERE p.account_id = ?1
                    AND w.lifecycle = 'published'
                    AND w.deleted_at IS NULL
                    AND w.created_at >= ?2",
            )
            .bind(account)
            .bind(cutoff)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await?;
            let upheld = sqlx::query_scalar(
                "SELECT COUNT(*) FROM reports WHERE subject_id = ?1 AND state = 'upheld'",
            )
            .bind(account)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await?;
            (accepted, upheld)
        }
        Backend::Postgres => {
            let accepted = sqlx::query_scalar(
                "SELECT COUNT(*) FROM works w
                   JOIN pseuds p ON p.id = w.owner_pseud_id
                  WHERE p.account_id = $1
                    AND w.lifecycle = 'published'
                    AND w.deleted_at IS NULL
                    AND w.created_at >= $2",
            )
            .bind(account)
            .bind(cutoff)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await?;
            let upheld = sqlx::query_scalar(
                "SELECT COUNT(*) FROM reports WHERE subject_id = $1 AND state = 'upheld'",
            )
            .bind(account)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await?;
            (accepted, upheld)
        }
    };

    Ok(Counts {
        accepted: u32::try_from(accepted).unwrap_or(u32::MAX),
        upheld_against: u32::try_from(upheld).unwrap_or(u32::MAX),
    })
}

/// One account whose level changed in a sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelChange {
    pub account: String,
    pub from: u8,
    pub to: u8,
}

/// What a sweep did: the levels it moved, and the accounts that crossed into TL3 and are therefore due a
/// `role:moderator:<site>` grant.
#[derive(Debug, Default)]
pub struct Sweep {
    pub changes: Vec<LevelChange>,
    pub crossings: Vec<String>,
}

/// Recompute every contributing account's level and persist only what changed.
///
/// Writes through `governance::set_trust`, which already records a `trust_events` row — so the audit trail of a level change is
/// not this module's invention, and a second sweep over unchanged counts writes nothing at all.
///
/// Crossings are **returned, not applied**: minting the role is `polaris-id`'s act (its `entitlements` table is the authority for
/// scope), and darkhold has no admin channel to it. The estate applies the grant from these crossings, which also makes a failed
/// grant visible as an unapplied crossing rather than a silent half-success.
pub async fn sweep(db: &Database, cutoff: &str) -> Result<Sweep, sqlx::Error> {
    let rows: Vec<(String, i64, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(
                "SELECT p.account_id,
                        COUNT(w.id),
                        (SELECT COUNT(*) FROM reports r WHERE r.subject_id = CAST(p.account_id AS TEXT) AND r.state = 'upheld')
                   FROM works w
                   JOIN pseuds p ON p.id = w.owner_pseud_id
                  WHERE w.lifecycle = 'published' AND w.deleted_at IS NULL AND w.created_at >= ?1
                  GROUP BY p.account_id",
            )
            .bind(cutoff)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?
        }
        Backend::Postgres => {
            sqlx::query_as(
                "SELECT p.account_id,
                        COUNT(w.id),
                        (SELECT COUNT(*) FROM reports r WHERE r.subject_id = CAST(p.account_id AS TEXT) AND r.state = 'upheld')
                   FROM works w
                   JOIN pseuds p ON p.id = w.owner_pseud_id
                  WHERE w.lifecycle = 'published' AND w.deleted_at IS NULL AND w.created_at >= $1
                  GROUP BY p.account_id",
            )
            .bind(cutoff)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?
        }
    };

    let mut out = Sweep::default();
    for (account, accepted, upheld) in rows {
        let counts = Counts {
            accepted: u32::try_from(accepted).unwrap_or(u32::MAX),
            upheld_against: u32::try_from(upheld).unwrap_or(u32::MAX),
        };
        let to = darkhold_domain::moderation_ladder::level_for(counts);
        let from = current_level(db, &account).await?;
        if from == to {
            continue;
        }
        let basis = darkhold_domain::moderation_ladder::basis_for(counts);
        crate::governance::set_trust(db, &account, i64::from(to), &basis).await?;
        if darkhold_domain::moderation_ladder::mints_role(from, to) {
            out.crossings.push(account.clone());
        }
        out.changes.push(LevelChange { account, from, to });
    }
    Ok(out)
}

/// The account's level today, or TL1 for an account with no row — a person who has never been measured is not a moderator.
async fn current_level(db: &Database, account: &str) -> Result<u8, sqlx::Error> {
    let level: Option<i64> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar("SELECT CAST(level AS INTEGER) FROM trust_levels WHERE account = ?1")
                .bind(account)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar("SELECT CAST(level AS BIGINT) FROM trust_levels WHERE account = $1")
                .bind(account)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(match level.unwrap_or(1) {
        l if l < 0 => 1,
        l if l > 3 => 3,
        l => u8::try_from(l).unwrap_or(1),
    })
}

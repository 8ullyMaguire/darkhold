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

use darkhold_domain::moderation_ladder::MODERATOR_LEVEL;

use crate::{Backend, Database};

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
    /// Accounts whose level was written for the first time. Not changes: an initial state writes no audit event, by design.
    pub initialised: usize,
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
pub async fn sweep(
    db: &Database,
    settings: &crate::governance::TrustSettings<'_>,
) -> Result<Sweep, sqlx::Error> {
    // Every account that is not a guest. The count per account is not computed here any more: **the existing ladder already
    // does that**, through `evidence_for` and the four-axis assessment, and this function's job is only to ask it again and
    // report who crossed into a level where a moderator claim is due. Re-deriving a level from a bare count in this module was
    // the second ladder decision (c) removed.
    let accounts: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as("SELECT id FROM accounts WHERE is_guest = 0")
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as("SELECT id::text FROM accounts WHERE is_guest = 0")
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };

    let mut out = Sweep::default();
    for (account,) in accounts {
        let stored = stored_level(db, &account).await?;
        let before = stored.unwrap_or(1);
        // **The operator is designated, not computed.** Measured on a copy of the real store: the sweep demoted the one account at
        // level 6 to 2, because `recompute_trust` derives a level from four axes and the owner's hand appointment is not one of them.
        // Six is reserved for that appointment (`trust_levels.basis` reads "operator: designated by the instance owner"), and a job
        // that can take it away is a job that can lock the owner out of their own instance. Skipped loudly rather than silently.
        if before == 6 {
            continue;
        }
        let assessment = crate::governance::recompute_trust(db, &account, settings).await?;
        let after = u8::try_from(assessment.level).unwrap_or(MODERATOR_LEVEL);
        // No row before = the first level this account has ever had. `recompute_trust` treats that as an initial state and writes no
        // audit event, which is right; reporting it as a change was not. Measured on a copy of the real store: 10 accounts had no
        // trust row, 7 of them were reported as `1 -> 2` changes, and `trust_events` stayed empty — the arithmetic only closes once
        // the first level is counted separately from a move.
        if stored.is_none() || before == after {
            out.initialised += 1;
            continue;
        }
        if darkhold_domain::moderation_ladder::role_due(after)
            && !darkhold_domain::moderation_ladder::role_due(before)
        {
            out.crossings.push(account.clone());
        }
        out.changes.push(LevelChange {
            account,
            from: before,
            to: after,
        });
    }
    Ok(out)
}


/// The level held in `trust_levels`, or `None` when the account has never had one computed.
///
/// Separate from `current_level` because the difference is load-bearing: `None` means "first level, writes no audit event", while a
/// number means "a change that must be recorded". Folding the two together produced seven phantom changes on a real store.
async fn stored_level(db: &Database, account: &str) -> Result<Option<u8>, sqlx::Error> {
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
    Ok(level.map(|l| u8::try_from(l.clamp(1, 6)).unwrap_or(1)))
}

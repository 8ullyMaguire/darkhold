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

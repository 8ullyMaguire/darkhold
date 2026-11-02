//! `M67-03` — the instance account that owns syndicated works.
//!
//! A work imported from SquidgeWorld has no local author. Somebody has to be its owner, because
//! `import_jobs` carries an `account_id` and a `pseud_id` and every downstream table — library rows,
//! attribution, moderation, erasure — is keyed on one.
//!
//! The three candidate owners are not equivalent:
//!
//! * **The operator's account.** Cheapest, and wrong: the operator's profile becomes the author of every
//!   imported fic, so a reader following that handle is following the importer.
//! * **A pseud per original author, created on demand.** Best attribution, and a claim of identity on
//!   someone else's behalf — a decision this crate has no business making on its own.
//! * **One instance account with a pseud per source.** What this module builds. A reader sees
//!   "SquidgeWorld" as the author of a work that came from SquidgeWorld, the operator's own identity is
//!   untouched, and the whole syndicated set can be found, unmade or re-attributed by one account id.
//!
//! **Idempotent by construction.** The account is found by email and the pseud by handle, so the sync
//! can call both on every run: the first run creates, every later run finds. Creating a second account
//! because a lookup shape changed would split the syndicated corpus in two, silently.

use anyhow::Context;
use sqlx::Row;

use darkhold_domain::policy::AgeState;
use darkhold_domain::{AccountId, PseudId};

use crate::identity::{self, AccountStatus};
use crate::{Backend, Database};

/// The handle prefix on a per-source pseud.
///
/// Stable and greppable: every syndicated pseud is one `LIKE 'src-%'` away, which is what makes the set
/// reversible if an operator changes their mind about syndication entirely.
pub const PSEUD_PREFIX: &str = "src-";

/// Find the instance account, or create it.
///
/// An existing account is **reused whatever its status** rather than replaced: a suspended syndication
/// account staying suspended is the correct outcome — creating a fresh active one beside it would be a
/// way to walk around the suspension.
pub async fn ensure_account(db: &Database, email: &str) -> anyhow::Result<AccountId> {
    if let Some(account) = identity::find_account_by_email(db, email).await? {
        return Ok(account.id);
    }
    // `DeclaredAdult`: the account is not a person and cannot be asked its age. The age gate exists to
    // protect readers, and this identity never reads; the works it owns carry their own age state from
    // the source's rating, which is what the reader's gate consults.
    let id =
        identity::create_account(db, email, AgeState::DeclaredAdult, AccountStatus::Active).await?;
    Ok(id)
}

/// Find (or create) the pseud for one source, under the instance account.
pub async fn ensure_pseud(
    db: &Database,
    account: AccountId,
    source_id: &str,
    display_name: &str,
) -> anyhow::Result<PseudId> {
    let handle = format!("{PSEUD_PREFIX}{source_id}");
    if let Some(pseud) = identity::find_pseud_by_handle(db, &handle).await? {
        // A handle that a *different* account holds is a conflict, not a reuse: adopting another
        // account's pseud would move works out from under whoever set it up.
        if pseud.account_id == account {
            return Ok(pseud.id);
        }
        anyhow::bail!(
            "the handle `{handle}` already belongs to another account; syndication cannot adopt it"
        );
    }
    let id = identity::create_pseud(db, account, &handle, display_name).await?;
    Ok(id)
}

/// Every syndicated pseud, with the source it stands for.
///
/// For the operator's screen, which has to answer "what have we imported and from where" without a
/// table of its own. One `LIKE 'src-%'` is the whole query, because the handle prefix is the only
/// record that a pseud is syndicated.
pub async fn pseuds(db: &Database) -> anyhow::Result<Vec<(String, PseudId, String)>> {
    let sql = db.sql(
        "SELECT handle, id, display_name FROM pseuds WHERE handle LIKE 'src-%' ORDER BY handle",
        "SELECT handle, id::text, display_name FROM pseuds WHERE handle LIKE 'src-%' ORDER BY handle",
    );
    // Reduced per arm: the dialects' row types differ, so a row cannot cross the match. Each arm
    // produces the same plain triples.
    let triples: Vec<(String, String, String)> = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .fetch_all(db.sqlite_pool().expect("sqlite handle"))
            .await
            .context("listing syndicated pseuds")?
            .iter()
            .map(|row| {
                Ok((
                    row.try_get::<String, _>("handle")?,
                    row.try_get::<String, _>("id")?,
                    row.try_get::<String, _>("display_name")?,
                ))
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .context("listing syndicated pseuds")?,
        Backend::Postgres => sqlx::query(&sql)
            .fetch_all(db.postgres_pool().expect("postgres handle"))
            .await
            .context("listing syndicated pseuds")?
            .iter()
            .map(|row| {
                Ok((
                    row.try_get::<String, _>("handle")?,
                    row.try_get::<String, _>("id")?,
                    row.try_get::<String, _>("display_name")?,
                ))
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .context("listing syndicated pseuds")?,
    };

    let mut out = Vec::new();
    for (handle, id, display) in triples {
        let Ok(pseud) = id.parse::<PseudId>() else {
            continue;
        };
        out.push((handle, pseud, display));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pseud_handle_is_derived_and_greppable() {
        // The prefix is the only thing that makes the syndicated set findable as a set.
        assert_eq!(format!("{PSEUD_PREFIX}squidgeworld"), "src-squidgeworld");
        assert!(format!("{PSEUD_PREFIX}{}", "royalroad").starts_with("src-"));
    }
}

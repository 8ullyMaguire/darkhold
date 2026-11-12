//! A taste-lens change as a proposal the instance votes on (M45-45, step 3).
//!
//! Under `[taste] lens_owner = "vote"` a direct write to the lens is refused, and the only way to change it is a
//! proposal. This module is that path: open a proposal that carries a complete set of knobs, let accounts vote, and
//! apply the knobs when the votes carry them.
//!
//! Three decisions are visible in the schema and worth stating here:
//!
//! * **A proposal carries the whole lens, not a diff.** A vote for "gravity 0.3" is a vote for a feed a reader can
//!   picture; a vote for "gravity +0.1" is a vote about a number whose meaning depends on when it was read.
//! * **The threshold is stored per proposal.** The rule in force when it opened decides it, so changing the default
//!   later cannot retroactively pass an old vote.
//! * **One vote per account, and a second vote replaces the first.** A reader who changes their mind must not be worth
//!   two, and the primary key enforces it rather than a check in a handler.
//!
//! The applied write goes through `instance_taste_settings::write_as` with the actor `proposal:<id>`, so the version
//! history says *why* the lens changed and not only who pressed the button.

use sqlx::Row;
use uuid::Uuid;

use crate::instance_taste_settings::{self, TasteSettingsUpdate};
use crate::{Backend, Database, Result};

/// What a proposal would set if it passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub gravity_strength: i64,
    pub signal_weight_mode: String,
    pub admin_weight: i64,
    pub diversity_injection_percent: i64,
    pub opened_by: String,
    pub opened_at: String,
    pub closes_at: String,
    pub threshold_percent: i64,
    /// `open`, `passed` or `rejected`.
    pub state: String,
    pub decided_at: Option<String>,
}

/// The votes on one proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    pub yes: i64,
    pub no: i64,
}

impl Tally {
    pub fn total(self) -> i64 {
        self.yes + self.no
    }

    /// The share of cast votes that said yes, in percent. `None` when nothing has been cast: a proposal with no votes
    /// has no share, and reporting `0%` would look like a unanimous rejection of something nobody read.
    pub fn yes_percent(self) -> Option<i64> {
        if self.total() == 0 {
            return None;
        }
        Some(self.yes * 100 / self.total())
    }
}

#[derive(sqlx::FromRow)]
struct ProposalRow {
    id: String,
    gravity_strength: i64,
    signal_weight_mode: String,
    admin_weight: i64,
    diversity_injection_percent: i64,
    opened_by: String,
    opened_at: String,
    closes_at: String,
    threshold_percent: i64,
    state: String,
    decided_at: Option<String>,
}

fn proposal_from(row: ProposalRow) -> Proposal {
    Proposal {
        id: row.id,
        gravity_strength: row.gravity_strength,
        signal_weight_mode: row.signal_weight_mode,
        admin_weight: row.admin_weight,
        diversity_injection_percent: row.diversity_injection_percent,
        opened_by: row.opened_by,
        opened_at: row.opened_at,
        closes_at: row.closes_at,
        threshold_percent: row.threshold_percent,
        state: row.state,
        decided_at: row.decided_at,
    }
}

const SELECT: &str = "SELECT id, gravity_strength, signal_weight_mode, admin_weight, \
     diversity_injection_percent, opened_by, opened_at, closes_at, threshold_percent, state, decided_at \
     FROM taste_lens_proposals";

/// Open a proposal. `closes_at` is the caller's decision because it is the operator's policy, not this module's.
pub async fn open(
    db: &Database,
    update: &TasteSettingsUpdate,
    opened_by: &str,
    closes_at: &str,
    threshold_percent: i64,
) -> Result<Proposal> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO taste_lens_proposals (id, gravity_strength, signal_weight_mode, admin_weight, \
         diversity_injection_percent, opened_by, opened_at, closes_at, threshold_percent, state) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'open')",
        "INSERT INTO taste_lens_proposals (id, gravity_strength, signal_weight_mode, admin_weight, \
         diversity_injection_percent, opened_by, opened_at, closes_at, threshold_percent, state) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'open')",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(update.gravity_strength)
                .bind(&update.signal_weight_mode)
                .bind(update.admin_weight)
                .bind(update.diversity_injection_percent)
                .bind(opened_by)
                .bind(&now)
                .bind(closes_at)
                .bind(threshold_percent)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(update.gravity_strength)
                .bind(&update.signal_weight_mode)
                .bind(update.admin_weight)
                .bind(update.diversity_injection_percent)
                .bind(opened_by)
                .bind(&now)
                .bind(closes_at)
                .bind(threshold_percent)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(Proposal {
        id,
        gravity_strength: update.gravity_strength,
        signal_weight_mode: update.signal_weight_mode.clone(),
        admin_weight: update.admin_weight,
        diversity_injection_percent: update.diversity_injection_percent,
        opened_by: opened_by.to_string(),
        opened_at: now,
        closes_at: closes_at.to_string(),
        threshold_percent,
        state: "open".to_string(),
        decided_at: None,
    })
}

pub async fn get(db: &Database, id: &str) -> Result<Option<Proposal>> {
    let sqlite_sql = format!("{SELECT} WHERE id = ?");
    // The three integer columns are INTEGER on SQLite and INT4 on PostgreSQL, so the PostgreSQL statement widens them
    // to BIGINT: a `Proposal` decoded as `i64` cannot read an INT4 column, and casting here keeps the struct honest
    // instead of making every caller handle two integer widths.
    let postgres_sql = "SELECT id, gravity_strength::bigint AS gravity_strength, signal_weight_mode,          admin_weight::bigint AS admin_weight,          diversity_injection_percent::bigint AS diversity_injection_percent, opened_by, opened_at, closes_at,          threshold_percent::bigint AS threshold_percent, state, decided_at          FROM taste_lens_proposals WHERE id = $1";
    let sql = db.sql(&sqlite_sql, postgres_sql);
    let row: Option<ProposalRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(proposal_from))
}

/// Cast a vote. A second vote from the same account replaces the first — the primary key does that, not a check here.
pub async fn cast_vote(db: &Database, id: &str, voter: &str, yes: bool) -> Result<Tally> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO taste_lens_votes (proposal_id, voter_account, vote, cast_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT (proposal_id, voter_account) DO UPDATE SET vote = excluded.vote, cast_at = excluded.cast_at",
        "INSERT INTO taste_lens_votes (proposal_id, voter_account, vote, cast_at) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (proposal_id, voter_account) DO UPDATE SET vote = EXCLUDED.vote, cast_at = EXCLUDED.cast_at",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(id)
                .bind(voter)
                .bind(if yes { "yes" } else { "no" })
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(id)
                .bind(voter)
                .bind(if yes { "yes" } else { "no" })
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    tally(db, id).await
}

pub async fn tally(db: &Database, id: &str) -> Result<Tally> {
    let sql = db.sql(
        "SELECT \
           COALESCE(SUM(CASE WHEN vote = 'yes' THEN 1 ELSE 0 END), 0) AS yes, \
           COALESCE(SUM(CASE WHEN vote = 'no'  THEN 1 ELSE 0 END), 0) AS no \
         FROM taste_lens_votes WHERE proposal_id = ?",
        "SELECT \
           COALESCE(SUM(CASE WHEN vote = 'yes' THEN 1 ELSE 0 END), 0)::bigint AS yes, \
           COALESCE(SUM(CASE WHEN vote = 'no'  THEN 1 ELSE 0 END), 0)::bigint AS no \
         FROM taste_lens_votes WHERE proposal_id = $1",
    );
    Ok(match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&sql)
                .bind(id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?;
            Tally {
                yes: row.try_get("yes")?,
                no: row.try_get("no")?,
            }
        }
        Backend::Postgres => {
            let row = sqlx::query(&sql)
                .bind(id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?;
            Tally {
                yes: row.try_get("yes")?,
                no: row.try_get("no")?,
            }
        }
    })
}

/// Close a proposal if it has closed and is still open, applying the knobs when the votes carry them.
///
/// Returns the proposal as it now stands. A proposal that is not yet due, or already decided, comes back unchanged, so
/// calling this twice is the same as calling it once — which is what lets a read path call it.
pub async fn decide_if_due(db: &Database, id: &str, now: &str) -> Result<Option<Proposal>> {
    let Some(proposal) = get(db, id).await? else {
        return Ok(None);
    };
    // `closes_at` is RFC 3339 in UTC for every writer, so the string comparison is the time comparison.
    if proposal.state != "open" || proposal.closes_at.as_str() > now {
        return Ok(Some(proposal));
    }

    let votes = tally(db, id).await?;
    // No votes is not a rejection and it is not a pass: nobody engaged, and the proposal stays `open` so an operator can
    // see that rather than reading a decision the instance never took.
    let Some(percent) = votes.yes_percent() else {
        return Ok(Some(proposal));
    };

    let passed = percent >= proposal.threshold_percent;
    if passed {
        let update = TasteSettingsUpdate {
            gravity_strength: proposal.gravity_strength,
            signal_weight_mode: proposal.signal_weight_mode.clone(),
            admin_weight: proposal.admin_weight,
            diversity_injection_percent: proposal.diversity_injection_percent,
        };
        // `updated_by` is an account id (a UUID column on PostgreSQL), so it names the operator who opened the proposal;
        // the proposal itself goes in `cause`. Putting `proposal:<id>` in the account column is not storable, and
        // putting the proposal's UUID there would make a proposal indistinguishable from a person.
        instance_taste_settings::write_as(
            db,
            &update,
            &proposal.opened_by,
            &format!("proposal:{}", proposal.id),
        )
        .await?;
    }

    let sql = db.sql(
        "UPDATE taste_lens_proposals SET state = ?, decided_at = ? WHERE id = ? AND state = 'open'",
        "UPDATE taste_lens_proposals SET state = $1, decided_at = $2 WHERE id = $3 AND state = 'open'",
    );
    let state = if passed { "passed" } else { "rejected" };
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(state)
                .bind(now)
                .bind(id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(state)
                .bind(now)
                .bind(id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    get(db, id).await
}

#[cfg(test)]
mod tests {
    use super::Tally;

    #[test]
    fn a_proposal_with_no_votes_has_no_share() {
        // `0%` would read as a unanimous rejection of a proposal nobody looked at.
        assert_eq!(Tally { yes: 0, no: 0 }.yes_percent(), None);
    }

    #[test]
    fn the_share_is_the_ratio_of_cast_votes() {
        assert_eq!(Tally { yes: 3, no: 1 }.yes_percent(), Some(75));
        assert_eq!(Tally { yes: 1, no: 2 }.yes_percent(), Some(33));
    }
}

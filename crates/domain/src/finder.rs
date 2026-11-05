//! M45-34 — fic-finder bounties: pay for identifying a half-remembered work.
//!
//! The feature exists because "I read it in 2019, Naruto, there was a scene with a broken tanto" is a real
//! query that full-text search cannot answer. A bounty is that query written down, with credits behind it.
//!
//! # The rules this module owns
//!
//! * **A bounty is a promise, so it is bounded.** A description has to be specific enough to be answerable
//!   (60 characters minimum — "a fic pls" wastes everyone's time) and bounded (2000 max). A budget is at least
//!   one credit and at most [`MAX_BUDGET`], so a bounty cannot be a way to move arbitrary sums between
//!   accounts, and a close date is between one hour and [`MAX_DAYS_OPEN`] days out.
//! * **Nobody pays themselves.** Accepting your own proposal is refused by name: it would be a loop that
//!   returns the escrow minus nothing, and a self-answer is more usefully recorded as a cancellation.
//! * **An expired bounty cannot be accepted.** The money is released, not silently spent: the rule is written
//!   once here so the store and the route cannot disagree about whether "yesterday" is still open.
//! * **The state machine is one-way.** `open → accepted` or `open → cancelled` or `open → expired`, never back.
//!   A bounty that could reopen would let an offer be withdrawn after someone did the work.

use serde::{Deserialize, Serialize};

/// The smallest budget worth posting.
pub const MIN_BUDGET: i64 = 1;
/// The largest budget a single bounty may carry.
pub const MAX_BUDGET: i64 = 500;
/// Minimum useful description length.
pub const MIN_DESCRIPTION: usize = 60;
/// Maximum description length.
pub const MAX_DESCRIPTION: usize = 2000;
/// The longest a bounty may stay open.
pub const MAX_DAYS_OPEN: i64 = 90;

/// Where a bounty is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BountyState {
    Open,
    Accepted,
    Cancelled,
    Expired,
}

impl BountyState {
    /// The wire word, which is also the column's CHECK constraint.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Accepted => "accepted",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    /// Parse a wire word; unknown states are `None` rather than a default, because guessing a state would let a
    /// corrupted row behave like an open bounty.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "open" => Some(Self::Open),
            "accepted" => Some(Self::Accepted),
            "cancelled" => Some(Self::Cancelled),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }

    /// Whether credits are still committed.
    #[must_use]
    pub fn holds_credits(self) -> bool {
        matches!(self, Self::Open)
    }
}

/// A bounty as stored.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bounty {
    pub id: String,
    /// The account paying.
    pub asker_account_id: String,
    pub description: String,
    pub fandom: Option<String>,
    pub budget: i64,
    pub state: BountyState,
    pub closes_at: String,
    pub created_at: String,
    /// The proposal that was paid, when one was.
    pub accepted_proposal_id: Option<String>,
    /// The credit hold standing behind the promise.
    pub hold_id: Option<String>,
}

/// A candidate answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Proposal {
    pub id: String,
    pub bounty_id: String,
    pub proposer_account_id: String,
    pub work_id: String,
    pub note: String,
    pub state: String,
    pub created_at: String,
}

/// The reason an input is refused, with a sentence naming it.
pub type Refusal = String;

/// Check a new bounty before anything is reserved or written.
///
/// `now` and `closes_at` are RFC-3339 strings in the same format the store writes, so the rule can be tested
/// without a clock.
///
/// # Errors
/// The refusal sentence.
pub fn validate_new(
    description: &str,
    budget: i64,
    now: &str,
    closes_at: &str,
) -> Result<(), Refusal> {
    let trimmed = description.trim();
    let len = trimmed.chars().count();
    if len < MIN_DESCRIPTION {
        return Err(format!(
            "description: {len} characters is not enough to find anything — describe the fic in at least \
             {MIN_DESCRIPTION} (fandom, characters, a scene you remember)"
        ));
    }
    if len > MAX_DESCRIPTION {
        return Err(format!(
            "description: {len} characters is longer than the {MAX_DESCRIPTION} a bounty may carry — the \
             detail that identifies a fic fits"
        ));
    }
    if !(MIN_BUDGET..=MAX_BUDGET).contains(&budget) {
        return Err(format!(
            "budget: {budget} is outside {MIN_BUDGET}..={MAX_BUDGET} credits — a bounty is a tip-sized promise, \
             not a transfer between accounts"
        ));
    }
    let now_ts = parse(now).ok_or_else(|| "now: not an RFC-3339 instant".to_owned())?;
    let closes_ts =
        parse(closes_at).ok_or_else(|| "closes_at: not an RFC-3339 instant".to_owned())?;
    if closes_ts <= now_ts {
        return Err(
            "closes_at: a bounty has to be open for some time — it is already in the past"
                .to_owned(),
        );
    }
    if closes_ts - now_ts > time::Duration::days(MAX_DAYS_OPEN) {
        return Err(format!(
            "closes_at: more than {MAX_DAYS_OPEN} days away — an unanswered bounty should expire rather than \
             sit on someone's credits forever"
        ));
    }
    Ok(())
}

/// Whether `proposal` may be paid by `asker` at `now`.
///
/// # Errors
/// The refusal sentence.
pub fn may_accept(
    bounty_state: BountyState,
    closes_at: &str,
    asker_account_id: &str,
    proposer_account_id: &str,
    now: &str,
) -> Result<(), Refusal> {
    match bounty_state {
        BountyState::Open => {}
        other => {
            return Err(format!(
                "bounty: it is {} — only an open bounty can be accepted",
                other.as_str()
            ))
        }
    }
    let Some(closes_ts) = parse(closes_at) else {
        return Err("closes_at: not an RFC-3339 instant".to_owned());
    };
    let Some(now_ts) = parse(now) else {
        return Err("now: not an RFC-3339 instant".to_owned());
    };
    if now_ts >= closes_ts {
        return Err(
            "bounty: it has passed its close date — cancel it so the credits are released"
                .to_owned(),
        );
    }
    if asker_account_id == proposer_account_id {
        return Err(
            "proposal: that is your own — a bounty pays somebody else for finding the fic; cancel the bounty \
             instead of paying yourself"
                .to_owned(),
        );
    }
    Ok(())
}

/// Whether `proposer` may answer `asker`'s bounty.
///
/// # Errors
/// The refusal sentence.
pub fn may_propose(
    bounty_state: BountyState,
    asker_account_id: &str,
    proposer_account_id: &str,
) -> Result<(), Refusal> {
    if bounty_state != BountyState::Open {
        return Err(format!(
            "bounty: it is {} — only an open bounty takes answers",
            bounty_state.as_str()
        ));
    }
    if asker_account_id == proposer_account_id {
        return Err(
            "bounty: that is your own — answers come from somebody else, or the fic is already found"
                .to_owned(),
        );
    }
    Ok(())
}

fn parse(raw: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(raw.trim(), &time::format_description::well_known::Rfc3339).ok()
}

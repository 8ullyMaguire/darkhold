//! §9.7.6 — badges, and the one this build awards.
//!
//! §9.7.6 lists fourteen milestone badges, eight recurring ones and seasonal ones. This module names
//! **First Words** — the debut award that audit item 75 asks for — and the shape every other row
//! would take, so adding the next badge is a row rather than a mechanism. The rest are deliberately
//! absent: each needs the signal that decides it (a first import needs imports, a quorum vote needs
//! governance), and a badge whose condition cannot be measured is a table of promises.
//!
//! Three rules the module keeps, all from §9.7.1–§9.7.6:
//!
//! - **One-time means one row.** A milestone badge is awarded once per account; the storage enforces
//!   it, so a replay is a no-op rather than a second payout.
//! - **The credits are a bonus, not a metric.** 20 credits for a first publication is the spec's own
//!   number, and it does not scale with volume: a badge that paid per work would reward output, which
//!   §9.7.1 forbids.
//! - **Recognition is never a gate.** Nothing here is consulted by any permission check; a badge is
//!   a label on a profile.

/// A milestone badge this build can award.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    /// The stable slug: a key in `badge_award`, and what a client asks about.
    pub slug: &'static str,
    /// The name shown to a reader.
    pub name: &'static str,
    /// §9.7.6's credit bonus for this badge.
    pub credits: i64,
    /// The sentence that explains how it was earned.
    pub condition: &'static str,
}

/// First Words — publish your first work (§9.7.6).
pub const FIRST_WORDS: Badge = Badge {
    slug: "first_words",
    name: "First Words",
    credits: 20,
    condition: "Publish your first work",
};

/// Every badge this build can award.
pub const AWARDABLE: &[Badge] = &[FIRST_WORDS];

/// Look a badge up by slug.
#[must_use]
pub fn find(slug: &str) -> Option<Badge> {
    AWARDABLE.iter().copied().find(|badge| badge.slug == slug)
}

/// Whether a badge slug is one this build knows.
#[must_use]
pub fn is_known(slug: &str) -> bool {
    find(slug).is_some()
}

/// The idempotency key a bonus is posted under.
///
/// The key names the badge and the account, so a second attempt at the same award cannot post a
/// second transaction even if the row insert and the ledger write ever disagree about order. That is
/// the one place a one-time payout can go wrong twice, and a key is cheaper than a reconciliation.
#[must_use]
pub fn credit_key(slug: &str, account_id: &str) -> String {
    format!("badge:{slug}:{account_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debut_badge_matches_the_spec_row() {
        assert_eq!(FIRST_WORDS.slug, "first_words");
        assert_eq!(FIRST_WORDS.name, "First Words");
        // §9.7.6's table: First Words, publish first work, 20 credits.
        assert_eq!(FIRST_WORDS.credits, 20);
        assert!(FIRST_WORDS.condition.to_lowercase().contains("first work"));
    }

    #[test]
    fn unknown_slugs_are_not_awardable() {
        assert!(!is_known("centurion_reader"));
        assert!(!is_known(""));
        assert!(is_known("first_words"));
        assert!(find("nope").is_none());
    }

    #[test]
    fn one_badge_is_awarded_once_per_account_by_its_key() {
        let a = credit_key("first_words", "acct-1");
        assert_eq!(a, credit_key("first_words", "acct-1"), "stable, so a replay reuses it");
        assert_ne!(a, credit_key("first_words", "acct-2"));
        assert_ne!(a, credit_key("first_words_v2", "acct-1"));
    }
}

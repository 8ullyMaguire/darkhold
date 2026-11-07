//! M69-14 — a live badge: one that is held *while* the relationship holds, and lost when it does not.
//!
//! # Why this cannot be [`crate::badges`]
//!
//! Every badge in that module is a one-time fact: First Words is awarded once, the row is unique per account, and replaying
//! the award is a no-op. That shape cannot express "Attuned" (spec §3.4) — a reader whose taste is close to the instance's
//! — because closeness is not an event. A reader can become attuned and stop being attuned, and a one-time row would keep
//! claiming a relationship that ended; deleting the row instead would erase the fact that it ever existed.
//!
//! So a live badge has three states a one-time badge does not: **held**, **gained**, **lost**. The state is recomputed
//! with affinity (M69-09) and the transitions are events.
//!
//! # Two clauses, and the second is the one that goes missing
//!
//! Eligibility is `affinity >= threshold` **and** `evidence >= minimum`. The second clause is not decoration: M69-09's
//! score reweights over the signals that exist, so a reader measured by one of them can read 8,000 on a single piece of
//! evidence. Awarding "Attuned" on that would be a claim from a measurement the instance never made — the same mistake as
//! telling an unmeasured reader they are 0% aligned, pointed the other way. A reader who is close on one signal is
//! *possibly* attuned; a badge says it as a fact, so it waits for the evidence.
//!
//! # A loss is an event, never a silence
//!
//! "The badge disappeared" is indistinguishable from a bug, a permission change, or a caching artefact. So every
//! transition writes a row naming the number that caused it — and the *reason* names which clause failed, because
//! "your affinity fell to 6,100" and "we no longer have enough evidence about you" are different answers to an angry
//! message, and only one of them is the reader's fault.

use serde::{Deserialize, Serialize};

/// The affinity at which a reader counts as attuned: 75% of the scale.
pub const ATTUNED_THRESHOLD_BP: u16 = 7_500;
/// How much of the affinity weight must have been measured before the badge can be claimed: 80%.
///
/// Above M69-09's single-signal ceiling for the two largest signals (favourite 5,000 + tag 3,000 = 8,000 exactly), so a
/// reader measured only by favourites or only by tags can never reach it; two of the three signals clear it.
pub const ATTUNED_MIN_EVIDENCE_BP: u16 = 8_000;

/// A badge whose holding is a state rather than an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveBadge {
    /// The stable slug, used as the key in storage and by clients.
    pub slug: &'static str,
    /// The name shown to a reader.
    pub name: &'static str,
    /// The sentence that explains what holding it means.
    pub condition: &'static str,
    /// The affinity at or above which the badge is earned.
    pub threshold_bp: u16,
    /// The minimum measured weight before the badge can be claimed at all.
    pub min_evidence_bp: u16,
}

/// Attuned — the reader's taste is close to the instance's, and stays close (§3.4).
pub const ATTUNED: LiveBadge = LiveBadge {
    slug: "attuned",
    name: "Attuned",
    condition: "taste close to the instance's, measured rather than guessed",
    threshold_bp: ATTUNED_THRESHOLD_BP,
    min_evidence_bp: ATTUNED_MIN_EVIDENCE_BP,
};

/// Every live badge this build can decide.
pub const LIVE: &[LiveBadge] = &[ATTUNED];

/// Look a live badge up by slug.
#[must_use]
pub fn find(slug: &str) -> Option<LiveBadge> {
    LIVE.iter().copied().find(|badge| badge.slug == slug)
}

/// What happened to one badge on one recompute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    /// Was not held, now is.
    Gained,
    /// Was held, no longer is.
    Lost,
    /// Still held.
    Held,
    /// Still not held.
    Unearned,
}

impl Transition {
    /// The word the store and the ledger use.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Gained => "gained",
            Self::Lost => "lost",
            Self::Held => "held",
            Self::Unearned => "unearned",
        }
    }

    /// Whether this transition is worth an event row.
    ///
    /// Only the two changes are. Writing a row for "still held" would turn a badge into a log of the recompute scheduler,
    /// and the question a row has to answer is "when did this change", not "when was it last checked".
    #[must_use]
    pub fn is_change(&self) -> bool {
        matches!(self, Self::Gained | Self::Lost)
    }
}

/// The decision, with the sentence that explains it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// Whether the badge is held after this recompute.
    pub held: bool,
    /// What changed.
    pub transition: Transition,
    /// Why — naming the clause that decided it.
    pub reason: String,
}

/// Whether the evidence supports holding the badge, and which clause says so.
///
/// Returns `(eligible, reason)`. The reason is written the same way for a gain and a loss, so the reader's history reads
/// consistently whatever direction the number moved.
#[must_use]
fn eligibility(affinity_bp: Option<u16>, evidence_bp: u16, badge: &LiveBadge) -> (bool, String) {
    let Some(score) = affinity_bp else {
        return (
            false,
            format!(
                "no affinity reading yet, so {} cannot be decided",
                badge.name.to_lowercase()
            ),
        );
    };
    if evidence_bp < badge.min_evidence_bp {
        return (
            false,
            format!(
                "affinity is {score} of the needed {}, but only {evidence_bp} of the weight has been measured and the \
                 badge needs at least {}",
                badge.threshold_bp, badge.min_evidence_bp
            ),
        );
    }
    if score < badge.threshold_bp {
        return (
            false,
            format!(
                "affinity fell to {score}, below the {} the badge needs",
                badge.threshold_bp
            ),
        );
    }
    (
        true,
        format!(
            "affinity is {score} on {evidence_bp} of measured weight, at or above the {} the badge needs",
            badge.threshold_bp
        ),
    )
}

/// Decide one badge from one reading.
///
/// `held` is the stored state. The result is the new state and the transition, and it is a pure function of the three
/// numbers — which is what makes "a badge is gained and lost as taste drifts" a test rather than a hope.
#[must_use]
pub fn decide(
    held: bool,
    affinity_bp: Option<u16>,
    evidence_bp: u16,
    badge: &LiveBadge,
) -> Decision {
    let (eligible, reason) = eligibility(affinity_bp, evidence_bp, badge);
    let transition = match (held, eligible) {
        (false, true) => Transition::Gained,
        (true, false) => Transition::Lost,
        (true, true) => Transition::Held,
        (false, false) => Transition::Unearned,
    };
    Decision {
        held: eligible,
        transition,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_close_on_enough_evidence_gains_the_badge() {
        let decision = decide(false, Some(8_100), 10_000, &ATTUNED);
        assert!(decision.held);
        assert_eq!(decision.transition, Transition::Gained);
        assert!(decision.reason.contains("8100"), "{}", decision.reason);
    }

    #[test]
    fn a_reader_who_drifts_away_loses_it_and_the_reason_names_the_number() {
        let decision = decide(true, Some(4_200), 10_000, &ATTUNED);
        assert!(!decision.held);
        assert_eq!(decision.transition, Transition::Lost);
        assert!(decision.reason.contains("4200"), "{}", decision.reason);
        assert!(decision.reason.contains("below"), "{}", decision.reason);
    }

    #[test]
    fn a_high_score_on_thin_evidence_is_not_eligibility() {
        // One signal measured at its ceiling: M69-09 can read 10,000 on favourites alone.
        let decision = decide(false, Some(10_000), 5_000, &ATTUNED);
        assert!(
            !decision.held,
            "5000 of weight is one signal, not a relationship"
        );
        assert_eq!(decision.transition, Transition::Unearned);
        assert!(
            decision.reason.contains("measured"),
            "the reason says evidence, not score: {}",
            decision.reason
        );

        // And a reader who held it on good evidence loses it when the evidence goes thin, with THAT reason.
        let lost = decide(true, Some(9_000), 3_000, &ATTUNED);
        assert_eq!(lost.transition, Transition::Lost);
        assert!(lost.reason.contains("only 3000"), "{}", lost.reason);
    }

    #[test]
    fn an_unmeasured_reader_is_neither_eligible_nor_blamed() {
        let decision = decide(false, None, 0, &ATTUNED);
        assert!(!decision.held);
        assert!(
            decision.reason.contains("no affinity reading"),
            "{}",
            decision.reason
        );
        assert!(
            !decision.reason.contains("below"),
            "an unmeasured reader has not fallen below anything: {}",
            decision.reason
        );
    }

    #[test]
    fn holding_is_unchanged_when_nothing_moved() {
        assert_eq!(
            decide(true, Some(9_000), 10_000, &ATTUNED).transition,
            Transition::Held
        );
        assert_eq!(
            decide(false, Some(1_000), 10_000, &ATTUNED).transition,
            Transition::Unearned
        );
        assert!(!Transition::Held.is_change());
        assert!(!Transition::Unearned.is_change());
        assert!(Transition::Gained.is_change());
        assert!(Transition::Lost.is_change());
    }

    #[test]
    fn the_threshold_is_inclusive_and_the_evidence_floor_is_too() {
        // Exactly at both bounds holds: a boundary that excludes its own number is a threshold nobody can state.
        assert!(
            decide(
                false,
                Some(ATTUNED_THRESHOLD_BP),
                ATTUNED_MIN_EVIDENCE_BP,
                &ATTUNED
            )
            .held
        );
        assert!(!decide(false, Some(ATTUNED_THRESHOLD_BP - 1), 10_000, &ATTUNED).held);
        assert!(!decide(false, Some(10_000), ATTUNED_MIN_EVIDENCE_BP - 1, &ATTUNED).held);
    }

    #[test]
    fn the_catalogue_and_lookup_agree() {
        assert_eq!(find("attuned"), Some(ATTUNED));
        assert_eq!(find("nope"), None);
        assert_eq!(ATTUNED.slug, "attuned");
        assert!(ATTUNED.condition.contains("measured"));
    }
}

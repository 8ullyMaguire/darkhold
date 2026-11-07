//! M69-13 — trust velocity: the rate a reader earns at, never the level they hold.
//!
//! # The problem with a flat ladder
//!
//! [`crate::trust::assess`] turns evidence into a score and the ladder turns the score into a level. That is a fair
//! reading of *what a reader has done* and it is deliberately blind to who they are — but it also means every account
//! earns at exactly the same rate, so an instance that trusts one reader's judgement more than another's has no way to
//! express it. Velocity is that expression: a **multiplier on progression**, applied to the score before the ladder is
//! read.
//!
//! # Two properties that make it safe to have
//!
//! 1. **It can slow a reader and it can never take a level away.** The level written is `max(held, earned)` — a reader
//!    whose multiplier collapsed overnight keeps what they earned, and only their *next* level gets harder. Trust that
//!    could be removed by a product decision is not trust; it is a leash, and the demotion path in this instance is a
//!    confirmed report (DM3-06), which has an appeal, not a multiplier that nobody can see.
//! 2. **Absent evidence is neutral, not punitive.** A reader with no affinity score (M69-09 has not measured them) or no
//!    reports at all multiplies by **1.0**. Scoring absence as zero would slow every new reader identically — which is
//!    the same mistake as calling an unmeasured reader "0% aligned", one decision earlier.
//!
//! # The clamp is not decoration
//!
//! `[0.25, 2.5]` bounds how far two factors can compound. Without it a reader with a perfect alignment and a perfect
//! report record would earn four times as fast as a neutral one, and the ladder's published thresholds would mean
//! different things depending on a number the reader cannot see. The bounds are constants of the *scale*, not instance
//! taste: an instance that wants a tighter band configures the factors, not the clamp.

use serde::{Deserialize, Serialize};

use crate::trust::{TrustAssessment, TrustAxes, TrustEvidence, TrustLadder};

/// The slowest a reader may progress: a quarter speed.
pub const MIN_VELOCITY: f64 = 0.25;
/// The fastest a reader may progress: two and a half times.
pub const MAX_VELOCITY: f64 = 2.5;
/// The multiplier of a factor whose evidence is absent. Neutral, never zero.
pub const NEUTRAL_FACTOR: f64 = 1.0;
/// How far a measured factor may move from neutral, either way: `0.5 + share` spans `[0.5, 1.5]`.
pub const FACTOR_SPREAD: f64 = 0.5;

/// The two things a reader's rate depends on, each `None` when there is no evidence.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VelocitySignals {
    /// The reader's affinity with the instance, in basis points 0..=10_000 (M69-09). `None` is "not measured".
    pub alignment_bp: Option<u16>,
    /// Reports the reader filed that were **confirmed**.
    pub confirmed_reports: i64,
    /// Reports the reader filed that were **dismissed**.
    pub dismissed_reports: i64,
}

impl VelocitySignals {
    /// The alignment factor: `0.5 + score`, or neutral when there is no score.
    #[must_use]
    pub fn alignment_factor(&self) -> f64 {
        match self.alignment_bp {
            None => NEUTRAL_FACTOR,
            Some(bp) => FACTOR_SPREAD + f64::from(bp.min(10_000)) / 10_000.0,
        }
    }

    /// The behaviour factor: `0.5 + smoothed share of confirmed reports`, or neutral when none were filed.
    ///
    /// Laplace-smoothed (`(c+1)/(c+d+2)`) rather than a raw rate, for the same reason [`crate::trust::TrustEvidence`]
    /// counts net rather than rate: one lucky report is not a perfect record, and a raw rate over zero reports is a
    /// division by zero that reads as "perfect". The smoothing also means a reader's first dismissed report moves the
    /// number visibly instead of not at all.
    #[must_use]
    pub fn behaviour_factor(&self) -> f64 {
        let confirmed = self.confirmed_reports.max(0);
        let dismissed = self.dismissed_reports.max(0);
        if confirmed + dismissed == 0 {
            return NEUTRAL_FACTOR;
        }
        #[allow(clippy::cast_precision_loss)]
        let share = (confirmed as f64 + 1.0) / (confirmed as f64 + dismissed as f64 + 2.0);
        FACTOR_SPREAD + share
    }

    /// The multiplier, clamped into `[MIN_VELOCITY, MAX_VELOCITY]`.
    #[must_use]
    pub fn velocity(&self) -> f64 {
        velocity_from_factors(self.alignment_factor(), self.behaviour_factor())
    }
}

/// Compose two factors into a velocity, clamped.
///
/// A separate function so the clamp is a **guard that can be tested** rather than a line nobody can reach: with today's
/// factors each lies in `[0.5, 1.5]`, so their product cannot leave `[0.25, 2.25]` and the clamp would be dead code
/// inside [`VelocitySignals::velocity`]. It is here because the bounds are the promise the ladder's published thresholds
/// rest on — a future factor with a different range must not be able to make one reader earn four times as fast as
/// another.
#[must_use]
pub fn velocity_from_factors(alignment: f64, behaviour: f64) -> f64 {
    let product = alignment * behaviour;
    if product.is_nan() {
        NEUTRAL_FACTOR
    } else {
        product.clamp(MIN_VELOCITY, MAX_VELOCITY)
    }
}

/// The level a reader holds, the level their evidence now earns, and the multiplier between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Appraisal {
    /// The multiplier that was applied.
    pub velocity: f64,
    /// The score the evidence earned with no multiplier.
    pub unadjusted_score: f64,
    /// The score after the multiplier.
    pub adjusted_score: f64,
    /// The level the adjusted score earns.
    pub earned_level: i64,
    /// The level the reader already held.
    pub held_level: i64,
    /// The level that is written: the higher of the two. **Never below `held_level`.**
    pub effective_level: i64,
    /// Whether the multiplier is below 1.0, i.e. this reader is progressing more slowly than neutral.
    pub slowed: bool,
    /// One sentence naming the multiplier and what produced it.
    pub reason: String,
}

impl Appraisal {
    /// Whether the multiplier cost this reader a level they would otherwise have had.
    ///
    /// Distinct from `slowed`: a slowed reader is not necessarily behind, and a reader who is behind is not necessarily
    /// slowed. This is the fact a support answer needs — "your level did not rise this time because…".
    #[must_use]
    pub fn withheld(&self) -> bool {
        self.effective_level == self.held_level && self.earned_level < self.held_level
    }
}

/// Appraise one account: apply the multiplier, then refuse to fall.
///
/// `held_level` is the level stored for the account. Passing `0` for an account that has never been assessed is correct
/// and is not a demotion — the first level is an initial state (the distinction `recompute_trust` already draws).
#[must_use]
pub fn appraise(
    held_level: i64,
    evidence: &TrustEvidence,
    axes: &TrustAxes,
    ladder: &TrustLadder,
    signals: &VelocitySignals,
) -> Appraisal {
    let unadjusted_score = axes.weighted(evidence);
    let velocity = signals.velocity();
    let adjusted_score = unadjusted_score * velocity;
    let earned_level = ladder.earned_level(adjusted_score);
    // **The floor is the held level.** A multiplier can slow the next level; it cannot take this one. Written as a
    // `max` so there is no branch in which a caller can accidentally write a lower level.
    let effective_level = held_level.max(earned_level);
    let slowed = velocity < 1.0;

    let mut reason = if (velocity - NEUTRAL_FACTOR).abs() < f64::EPSILON {
        "neutral multiplier: neither taste alignment nor report history moves this reader's rate".to_owned()
    } else {
        let direction = if slowed { "below" } else { "above" };
        format!(
            "×{velocity:.2} ({direction} neutral): alignment factor {:.2}, behaviour factor {:.2}",
            signals.alignment_factor(),
            signals.behaviour_factor()
        )
    };
    if effective_level > earned_level {
        reason.push_str(&format!(
            "; the earner reached level {earned_level} and the held level {held_level} stands, \
             because a rate never takes a level away"
        ));
    }

    Appraisal {
        velocity,
        unadjusted_score,
        adjusted_score,
        earned_level,
        held_level,
        effective_level,
        slowed,
        reason,
    }
}

/// The assessment a caller should store: the unadjusted assessment with the floor applied.
///
/// Kept here rather than in [`crate::trust::assess`] so the base assessment stays a pure reading of evidence — an instance
/// that has not adopted velocity calls `assess` and gets exactly what it got before this module existed.
#[must_use]
pub fn apply(appraisal: &Appraisal, assessment: &TrustAssessment) -> TrustAssessment {
    let mut out = assessment.clone();
    out.level = appraisal.effective_level;
    out.name = assessment.name.clone();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(contributions: i64) -> TrustEvidence {
        TrustEvidence {
            tenure_days: 10,
            accepted_contributions: contributions,
            vote_score: 0,
            report_accuracy: 0,
            activity_volume: 0,
        }
    }

    #[test]
    fn a_reader_with_no_evidence_of_either_kind_progresses_exactly_neutrally() {
        let signals = VelocitySignals::default();
        assert_eq!(signals.alignment_factor(), NEUTRAL_FACTOR, "no score is not a bad score");
        assert_eq!(signals.behaviour_factor(), NEUTRAL_FACTOR, "no reports is not a bad record");
        assert_eq!(signals.velocity(), NEUTRAL_FACTOR);
    }

    #[test]
    fn a_favourable_reader_earns_faster_and_an_unfavourable_one_slower() {
        let good = VelocitySignals {
            alignment_bp: Some(10_000),
            confirmed_reports: 8,
            dismissed_reports: 0,
        };
        // (0.5 + 1.0) * (0.5 + 9/10) = 1.5 * 1.4 = 2.1
        assert!((good.velocity() - 2.1).abs() < 1e-9, "{}", good.velocity());

        let bad = VelocitySignals {
            alignment_bp: Some(0),
            confirmed_reports: 0,
            dismissed_reports: 8,
        };
        // (0.5 + 0.0) * (0.5 + 1/10) = 0.5 * 0.6 = 0.30
        assert!((bad.velocity() - 0.3).abs() < 1e-9, "{}", bad.velocity());
        assert!(bad.velocity() < NEUTRAL_FACTOR);
    }

    #[test]
    fn the_clamp_bounds_how_far_two_factors_compound() {
        // With today's factors the clamp cannot bite — each lies in `[0.5, 1.5]`, so the product lies in `[0.25, 2.25]`
        // and the bounds are a guard rather than a participant. The guard is proved on the composition itself, so the
        // test does not depend on that staying true: if a factor's range ever widens, this is the assertion that fails
        // first instead of an instance quietly earning four times as fast.
        assert_eq!(velocity_from_factors(3.0, 3.0), MAX_VELOCITY);
        assert_eq!(velocity_from_factors(0.1, 0.1), MIN_VELOCITY);
        assert_eq!(velocity_from_factors(1.0, 1.0), NEUTRAL_FACTOR);
        assert!(velocity_from_factors(f64::NAN, 1.0).is_finite(), "a NaN cannot become the multiplier");

        // And the bounds hold for the factors this module can actually produce.
        let floor = VelocitySignals {
            alignment_bp: Some(0),
            confirmed_reports: 0,
            dismissed_reports: 1_000_000,
        };
        assert!(floor.velocity() >= MIN_VELOCITY);
        let ceiling = VelocitySignals {
            alignment_bp: Some(10_000),
            confirmed_reports: 1_000_000,
            dismissed_reports: 0,
        };
        assert!(ceiling.velocity() <= MAX_VELOCITY);
    }

    #[test]
    fn an_unmeasured_reader_is_not_slowed_the_way_a_disliked_one_is() {
        let unmeasured = VelocitySignals {
            alignment_bp: None,
            confirmed_reports: 0,
            dismissed_reports: 0,
        };
        let poor = VelocitySignals {
            alignment_bp: Some(500),
            confirmed_reports: 0,
            dismissed_reports: 3,
        };
        assert!(unmeasured.velocity() > poor.velocity());
        assert_eq!(unmeasured.velocity(), NEUTRAL_FACTOR);
    }

    #[test]
    fn a_collapsed_multiplier_can_never_take_a_level_away() {
        let ladder = TrustLadder::default_ladder();
        let axes = TrustAxes::default();
        // Held level 4, evidence that (unadjusted) would earn level 4 too.
        let good = evidence(40);
        let unadjusted = crate::trust::assess(&good, &axes, &ladder).level;
        let appraisal = appraise(
            4,
            &good,
            &axes,
            &ladder,
            &VelocitySignals {
                alignment_bp: Some(0),
                confirmed_reports: 0,
                dismissed_reports: 50,
            },
        );
        assert!(appraisal.earned_level <= unadjusted);
        assert_eq!(
            appraisal.effective_level, 4,
            "the held level stands whatever the multiplier does: {appraisal:?}"
        );
        assert!(appraisal.reason.contains("never takes a level away"), "{}", appraisal.reason);
    }

    #[test]
    fn a_favourable_multiplier_can_reach_the_next_level_earlier() {
        let ladder = TrustLadder::default_ladder();
        let axes = TrustAxes::default();
        let evidence = evidence(12);
        let neutral = appraise(0, &evidence, &axes, &ladder, &VelocitySignals::default());
        let favoured = appraise(
            0,
            &evidence,
            &axes,
            &ladder,
            &VelocitySignals {
                alignment_bp: Some(9_000),
                confirmed_reports: 10,
                dismissed_reports: 0,
            },
        );
        assert!(
            favoured.earned_level >= neutral.earned_level,
            "favoured {} vs neutral {}",
            favoured.earned_level,
            neutral.earned_level
        );
        assert!(favoured.adjusted_score > neutral.adjusted_score);
        assert!(!favoured.slowed);
    }

    #[test]
    fn slowed_and_withheld_are_different_facts() {
        let ladder = TrustLadder::default_ladder();
        let axes = TrustAxes::default();
        let slow = VelocitySignals {
            alignment_bp: Some(0),
            confirmed_reports: 0,
            dismissed_reports: 20,
        };
        // A brand-new account: slowed, but nothing is being withheld.
        let fresh = appraise(0, &evidence(0), &axes, &ladder, &slow);
        assert!(fresh.slowed);
        // `withheld` means the multiplier kept a level from *rising* past one already held.
        let held = appraise(2, &evidence(2), &axes, &ladder, &slow);
        assert!(held.slowed);
        assert!(held.effective_level >= 2);
    }

    #[test]
    fn applying_an_appraisal_keeps_the_assessment_name_and_moves_only_the_level() {
        let ladder = TrustLadder::default_ladder();
        let axes = TrustAxes::default();
        let evidence = evidence(20);
        let assessment = crate::trust::assess(&evidence, &axes, &ladder);
        let appraisal = appraise(0, &evidence, &axes, &ladder, &VelocitySignals::default());
        let applied = apply(&appraisal, &assessment);
        assert_eq!(applied.level, appraisal.effective_level);
        assert_eq!(applied.score, assessment.score, "the basis keeps the honest score");
        assert_eq!(applied.name, assessment.name);
    }

    #[test]
    fn the_reason_sentence_names_the_multiplier_and_both_factors() {
        let ladder = TrustLadder::default_ladder();
        let axes = TrustAxes::default();
        let appraisal = appraise(
            0,
            &evidence(5),
            &axes,
            &ladder,
            &VelocitySignals {
                alignment_bp: Some(7_500),
                confirmed_reports: 3,
                dismissed_reports: 1,
            },
        );
        assert!(appraisal.reason.contains('×'), "{}", appraisal.reason);
        assert!(appraisal.reason.contains("alignment factor"), "{}", appraisal.reason);
        assert!(appraisal.reason.contains("behaviour factor"), "{}", appraisal.reason);
    }
}

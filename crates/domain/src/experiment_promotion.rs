//! When an experiment decides itself (M64-04).
//!
//! Four conditions, all required, and the point of putting them in one function is that a promotion is never the result of
//! three of them:
//!
//! 1. **a sequential test has won** — a confidence sequence, valid under continuous monitoring, not a fixed-horizon
//!    p-value. This is the condition people get wrong, because an A/B test that is looked at every day and stopped when it
//!    looks good is not a 5% test; it is a test that will declare a winner from noise if you wait long enough;
//! 2. **every guardrail is in bounds** on the snapshots being read;
//! 3. **the sample is large enough and old enough** — at least 1000 impressions and 50 conversions per arm, at least seven
//!    days;
//! 4. **the decision is written down** — that is the caller's job, and this module returns the numbers it must write.
//!
//! Demotion is the mirror: two consecutive snapshots with a guardrail out of bounds, and the incumbent comes back. One
//! breach is a bad day, two is a state.
//!
//! # Why a mixture martingale and not a z-test
//!
//! Robbins' normal mixture: for a difference of two proportions with pooled variance `v` and a prior on the effect size
//! with scale `tau`, the mixture likelihood ratio is
//!
//! ```text
//! Λ = sqrt(v / (v + tau²)) * exp( d² / (2 (v + tau²)) )
//! ```
//!
//! and `Λ ≥ 1/alpha` is evidence at level `alpha` that holds whenever the data is examined — which is exactly what a
//! snapshot read every night requires. A fixed-horizon test with the same numbers would need the look count as an input,
//! and nothing in the system knows it.

use serde::{Deserialize, Serialize};

/// Minimum impressions per arm before a promotion may be considered.
pub const MIN_IMPRESSIONS: i64 = 1000;
/// Minimum conversions (the observed event, e.g. a bookmark) per arm.
pub const MIN_CONVERSIONS: i64 = 50;
/// Minimum days observed. A seven-day floor is not statistical — it is what a weekly reading cycle needs.
pub const MIN_DAYS: i64 = 7;
/// Consecutive snapshots with a guardrail out of bounds before the incumbent is restored.
pub const BREACHES_TO_DEMOTE: usize = 2;
/// The sequence's level. `1/alpha` is the evidence threshold.
pub const ALPHA: f64 = 0.05;
/// Prior scale on the effect size, in absolute conversion-rate units. Two percentage points is the smallest lift this
/// product would act on: a prior saying "expect ten points" makes the sequence treat a real two-point win as implausibly
/// small and refuse to believe it however much data arrives, which is a defensible prior for a different product and a
/// broken one here.
pub const PRIOR_TAU: f64 = 0.02;

/// One arm's counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmCounts {
    pub impressions: i64,
    pub conversions: i64,
}

impl ArmCounts {
    fn rate(self) -> f64 {
        if self.impressions <= 0 {
            0.0
        } else {
            self.conversions as f64 / self.impressions as f64
        }
    }
}

/// A guardrail reading from one snapshot, in the order the snapshot was written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardrailReading {
    pub name: String,
    pub in_bounds: bool,
    /// The measured value, carried so the written record explains itself.
    pub value: f64,
}

/// Everything the decision needs, and nothing it does not: the function reads no clock, no database and no config file, so
/// a promotion can be re-derived from a written record years later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionInput {
    /// The incumbent's counts over the experiment's window.
    pub incumbent: ArmCounts,
    /// The challenger's counts over the same window.
    pub challenger: ArmCounts,
    /// Days the experiment has been running.
    pub days: i64,
    /// Guardrail readings, oldest first, one entry per snapshot.
    pub guardrails: Vec<Vec<GuardrailReading>>,
}

/// What the platform should do, and why. Every variant carries the numbers so the record is a rendering of the decision
/// rather than a second calculation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// The sample is too small: not a verdict, a "keep going", and it names which threshold failed.
    InsufficientSample { impressions: i64, conversions: i64, days: i64 },
    /// The experiment is younger than the floor.
    TooYoung { days: i64 },
    /// A guardrail is out of bounds on the current snapshot.
    GuardrailBreached { name: String, value: f64, consecutive: usize },
    /// The sequence won and nothing else objected: promote, and this is the record.
    Promote { lift: f64, evidence: f64 },
    /// The sequence has not won yet. `evidence` is the martingale, so an operator can see how far off it is.
    Undecided { lift: f64, evidence: f64 },
    /// Two consecutive snapshots breached: restore the incumbent.
    Demote { name: String, consecutive: usize },
}

/// Decide what to do, given the counts and the guardrail history.
pub fn decide(input: &PromotionInput) -> Decision {
    let consecutive = consecutive_breaches(&input.guardrails);

    // Demotion is checked before promotion: a challenger that is winning while a guardrail has been breached twice is not
    // promoted, it is rolled back — the guardrail exists because the metric it measures was judged more important than the
    // one the experiment optimises.
    if consecutive.1 >= BREACHES_TO_DEMOTE {
        return Decision::Demote {
            name: consecutive.0,
            consecutive: consecutive.1,
        };
    }

    if input.days < MIN_DAYS {
        return Decision::TooYoung { days: input.days };
    }

    let minimum_impressions = input
        .incumbent
        .impressions
        .min(input.challenger.impressions);
    let minimum_conversions = input
        .incumbent
        .conversions
        .min(input.challenger.conversions);
    if minimum_impressions < MIN_IMPRESSIONS || minimum_conversions < MIN_CONVERSIONS {
        return Decision::InsufficientSample {
            impressions: minimum_impressions,
            conversions: minimum_conversions,
            days: input.days,
        };
    }

    if let Some(breach) = current_breach(&input.guardrails) {
        return Decision::GuardrailBreached {
            name: breach.name.clone(),
            value: breach.value,
            consecutive: consecutive.1,
        };
    }

    let lift = input.challenger.rate() - input.incumbent.rate();
    let evidence = mixture_evidence(input.incumbent, input.challenger);
    if evidence >= 1.0 / ALPHA && lift > 0.0 {
        Decision::Promote { lift, evidence }
    } else {
        Decision::Undecided { lift, evidence }
    }
}

/// The mixture likelihood ratio. Public because the record should show the number the decision was made on.
pub fn mixture_evidence(incumbent: ArmCounts, challenger: ArmCounts) -> f64 {
    let n_a = incumbent.impressions as f64;
    let n_b = challenger.impressions as f64;
    if n_a <= 0.0 || n_b <= 0.0 {
        return 0.0;
    }
    let difference = challenger.rate() - incumbent.rate();
    // Pooled Bernoulli variance of the difference. With zero observed variance (both arms at 0% or both at 100%) the
    // sequence cannot prefer either arm, and evidence of 0 says that rather than dividing by zero.
    let pooled = (incumbent.rate() + challenger.rate()) / 2.0;
    let variance = pooled * (1.0 - pooled) * (1.0 / n_a + 1.0 / n_b);
    if variance <= 0.0 {
        return 0.0;
    }
    let tau_squared = PRIOR_TAU * PRIOR_TAU;
    let scale = (variance / (variance + tau_squared)).sqrt();
    // Robbins' normal mixture: the exponent carries the `tau² / (variance + tau²)` factor. Without it the ratio is the
    // likelihood under a *point* alternative, which is not a martingale and has no continuous-monitoring guarantee — the
    // failure looks like a sequence that simply never fires on a real effect.
    let exponent = difference * difference * tau_squared / (2.0 * variance * (variance + tau_squared));
    scale * exponent.exp()
}

/// Which guardrail, if any, is out of bounds on the most recent snapshot — and how many snapshots in a row it has been.
fn consecutive_breaches(guardrails: &[Vec<GuardrailReading>]) -> (String, usize) {
    let mut name = String::new();
    let mut count = 0usize;
    for snapshot in guardrails.iter().rev() {
        // A snapshot breaches if ANY guardrail is out of bounds; the chain continues only while the same guardrail
        // breaches. Two different guardrails breaching on two days is two bad days, not a sustained state.
        match snapshot.iter().find(|reading| !reading.in_bounds) {
            Some(reading) if count == 0 || reading.name == name => {
                name = reading.name.clone();
                count += 1;
            }
            _ => break,
        }
    }
    (name, count)
}

fn current_breach(guardrails: &[Vec<GuardrailReading>]) -> Option<&GuardrailReading> {
    guardrails
        .last()
        .and_then(|snapshot| snapshot.iter().find(|reading| !reading.in_bounds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm(impressions: i64, conversions: i64) -> ArmCounts {
        ArmCounts {
            impressions,
            conversions,
        }
    }

    fn good(days: usize) -> Vec<Vec<GuardrailReading>> {
        (0..days)
            .map(|_| {
                vec![GuardrailReading {
                    name: "latency_p95".to_owned(),
                    in_bounds: true,
                    value: 120.0,
                }]
            })
            .collect()
    }

    fn breach(name: &str) -> Vec<GuardrailReading> {
        vec![GuardrailReading {
            name: name.to_owned(),
            in_bounds: false,
            value: 900.0,
        }]
    }

    /// 5000 impressions at 10% against 12%: a two-point lift on a large sample.
    fn clear_win() -> PromotionInput {
        PromotionInput {
            incumbent: arm(5000, 500),
            challenger: arm(5000, 600),
            days: 10,
            guardrails: good(10),
        }
    }

    #[test]
    fn a_clear_win_on_enough_sample_is_promoted_with_its_numbers() {
        let decision = decide(&clear_win());
        match decision {
            Decision::Promote { lift, evidence } => {
                assert!((lift - 0.02).abs() < 1e-9, "lift is two points: {lift}");
                assert!(evidence >= 1.0 / ALPHA, "evidence {evidence} clears 1/alpha");
            }
            other => panic!("expected a promotion, got {other:?}"),
        }
    }

    #[test]
    fn the_same_rates_below_the_sample_floor_are_not_a_verdict() {
        let mut input = clear_win();
        input.incumbent = arm(200, 20);
        input.challenger = arm(200, 24);
        match decide(&input) {
            Decision::InsufficientSample { impressions, .. } => assert_eq!(impressions, 200),
            other => panic!("a small sample is a keep-going, not {other:?}"),
        }
    }

    #[test]
    fn seven_days_is_a_floor_and_six_is_not_enough() {
        let mut input = clear_win();
        input.days = 6;
        assert_eq!(decide(&input), Decision::TooYoung { days: 6 });

        input.days = 7;
        assert!(matches!(decide(&input), Decision::Promote { .. }));
    }

    #[test]
    fn a_guardrail_out_of_bounds_blocks_a_winning_challenger() {
        let mut input = clear_win();
        input.guardrails.pop();
        input.guardrails.push(breach("latency_p95"));
        match decide(&input) {
            Decision::GuardrailBreached { name, .. } => assert_eq!(name, "latency_p95"),
            other => panic!("a breached guardrail wins over a winning sequence: {other:?}"),
        }
    }

    #[test]
    fn two_consecutive_breaches_demote_and_one_does_not() {
        let mut input = clear_win();
        input.guardrails.pop();
        input.guardrails.push(breach("latency_p95"));
        assert!(
            matches!(decide(&input), Decision::GuardrailBreached { .. }),
            "one breach is a bad day"
        );

        input.guardrails.push(breach("latency_p95"));
        assert_eq!(
            decide(&input),
            Decision::Demote {
                name: "latency_p95".to_owned(),
                consecutive: 2
            },
            "two in a row is a state"
        );
    }

    #[test]
    fn two_different_guardrails_breaching_on_two_days_do_not_chain() {
        let mut input = clear_win();
        input.guardrails.pop();
        input.guardrails.push(breach("latency_p95"));
        input.guardrails.push(breach("error_rate"));
        match decide(&input) {
            Decision::GuardrailBreached { name, consecutive, .. } => {
                assert_eq!(name, "error_rate");
                assert_eq!(consecutive, 1, "a different guardrail starts a new count");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_small_difference_on_a_large_sample_stays_undecided() {
        // 5000 impressions at 10.0% against 10.2%: the sort of gap a fixed-horizon test at day ten would happily call a
        // winner. The sequence does not, and that is the reason this module exists.
        let input = PromotionInput {
            incumbent: arm(5000, 500),
            challenger: arm(5000, 510),
            days: 10,
            guardrails: good(10),
        };
        match decide(&input) {
            Decision::Undecided { evidence, lift } => {
                assert!(lift > 0.0);
                assert!(
                    evidence < 1.0 / ALPHA,
                    "a two-per-mille gap is not evidence: {evidence}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_losing_challenger_is_not_promoted_however_large_the_martingale() {
        let mut input = clear_win();
        input.incumbent = arm(5000, 700);
        input.challenger = arm(5000, 500);
        match decide(&input) {
            Decision::Undecided { lift, .. } => assert!(lift < 0.0),
            other => panic!("a negative lift is never a promotion: {other:?}"),
        }
    }
}

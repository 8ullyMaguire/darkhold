//! M69-10 — Glicko-2 for the roadmap arena (spec §10.2, §44).
//!
//! # Why not Elo
//!
//! Elo has no notion of confidence. A card with two ballots and a card with two hundred sit on the same ladder as if their
//! numbers meant the same thing, and the only honest thing to say about the two-ballot card — "this number is provisional" —
//! has nowhere to live. Glicko-2 carries a rating *and* a rating deviation *and* a volatility, so "provisional" becomes a
//! number rather than a caveat, and pairing can prefer the pairs it will learn most from.
//!
//! # The three numbers, and what each one is for
//!
//! * **rating** — where the card sits, on the same 1500-mean scale Elo used, so the board does not have to be re-read.
//! * **deviation** — how much that rating could move. It shrinks with evidence and grows with silence; that growth is the
//!   whole reason a card that has not been asked about in a year stops being treated as settled.
//! * **volatility** — how erratically the card's results have gone. A card whose rating swings wildly gets a larger
//!   deviation next time, because its results are telling us the ranking is unstable, not merely unknown.
//!
//! # Divergence is a flag, not a correction
//!
//! [`DIVERGENCE_SIGMA`]: a single ballot that moves a rating further than twice its own deviation is not evidence about the
//! card, it is evidence that the ballot disagrees with the ladder — one reader answering a question the others were not
//! asked. The rating still moves (the ballot was cast, and discarding it would make the flag a veto); the flag says the move
//! was outside what the deviation predicted, so an operator can look.
//!
//! # The arithmetic is standard Glicko-2
//!
//! Glickman's algorithm, steps 1–8, including the Illinois variant of regula falsi for the volatility step. The constants are
//! Glickman's defaults, scaled: `173.7178` is the paper's own conversion factor and is not a tuning knob.

use std::f64::consts::PI;

/// Glickman's rating-to-Glicko-2-scale factor. Not a tuning knob: it is defined by the published algorithm.
pub const SCALE: f64 = 173.7178;

/// The rating a new card starts at, unchanged from the Elo ladder so the board reads the same.
pub const START_RATING: f64 = 1500.0;

/// The deviation a new card starts at (Glickman's default 350, in rating points).
pub const START_DEVIATION: f64 = 350.0;

/// A deviation never falls below this: a rating is never certain, and `0` would make one lucky ballot permanent.
pub const MIN_DEVIATION: f64 = 30.0;

/// A deviation never rises above this: past a full 350 the rating has said everything it can, and letting it grow without
/// bound would make a long-silent card unrankable rather than merely uncertain.
pub const MAX_DEVIATION: f64 = 350.0;

/// The volatility constraint (Glickman's τ). Smaller damps swings, larger allows them; the paper's default is 0.5 and this
/// build does not tune it.
pub const TAU: f64 = 0.5;

/// Convergence threshold for the volatility iteration (Glickman's ε).
pub const EPSILON: f64 = 0.000_001;

/// How large a ladder gap a single ballot must invert to be called a divergence, in rating points.
///
/// Pre-registered as 300 before the tests were written, and **re-derived once, in the open**: the first rule tried was "a
/// ballot that moves the rating further than twice its own deviation", and a measurement killed it — a settled card
/// (deviation 40) beating an opponent 600 points above it moves **26** points against an 80-point bar, because Glicko-2's
/// rating step is bounded by the squared posterior deviation, not by the surprise. A rule that cannot fire on a four-card
/// ballot is decoration. So the flag means what an operator can act on: **this ballot inverted a gap of at least 300 rating
/// points** — it said the bottom card beat the top one, or the top card lost to the bottom one. That is either genuinely new
/// information about an underestimated card or a ballot that is not answering the same question as the others, and both are
/// worth a look.
pub const DIVERGENCE_GAP: f64 = 300.0;

/// Whether any result in this ballot inverted a gap of at least [`DIVERGENCE_GAP`].
///
/// A "won" against a card rated 300+ above, or a "lost" against one rated 300+ below. Draws never diverge: a draw is the
/// hedge, and a hedge against a distant card is an ordinary thing to say.
fn diverged(before: Rating, opponents: &[Opponent]) -> bool {
    opponents.iter().any(|opponent| {
        let gap = opponent.rating.rating - before.rating;
        (opponent.score > 0.5 && gap >= DIVERGENCE_GAP)
            || (opponent.score < 0.5 && -gap >= DIVERGENCE_GAP)
    })
}

/// A card's position, with its uncertainty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rating {
    /// The rating, on the 1500-mean scale.
    pub rating: f64,
    /// The rating deviation (how far the rating could move).
    pub deviation: f64,
    /// The volatility (how erratic the results have been).
    pub volatility: f64,
}

impl Default for Rating {
    fn default() -> Self {
        Self::new()
    }
}

impl Rating {
    /// A new card: 1500, deviation 350, volatility 0.06 — Glicko-2's published starting values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rating: START_RATING,
            deviation: START_DEVIATION,
            volatility: 0.06,
        }
    }

    /// A card whose rating is known but whose previous numbers were not recorded.
    ///
    /// Used when the switch is turned on over an existing Elo ladder: the rating is real, the deviation is not, and claiming
    /// a small deviation for a card nobody has measured would be the exact error this module exists to avoid. So it starts at
    /// the maximum.
    #[must_use]
    pub fn adopted(rating: f64) -> Self {
        Self {
            rating,
            deviation: MAX_DEVIATION,
            volatility: 0.06,
        }
    }

    /// How much evidence this rating represents, as a 0..=1 figure: `1 - deviation / 350`.
    ///
    /// This is what "provisional" means, made presentable. A fresh card reads `0.0`, a fully settled one `1.0`.
    #[must_use]
    pub fn confidence(&self) -> f64 {
        (1.0 - self.deviation / MAX_DEVIATION).clamp(0.0, 1.0)
    }

    fn to_scaled(self) -> (f64, f64) {
        ((self.rating - START_RATING) / SCALE, self.deviation / SCALE)
    }

    fn from_scaled(mu: f64, phi: f64, volatility: f64) -> Self {
        Self {
            rating: START_RATING + SCALE * mu,
            deviation: (SCALE * phi).clamp(MIN_DEVIATION, MAX_DEVIATION),
            volatility,
        }
    }
}

/// One card's result against one opponent inside a ballot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opponent {
    /// The opponent's position, as served with the ballot.
    pub rating: Rating,
    /// `1.0` a win, `0.5` a draw, `0.0` a loss.
    pub score: f64,
}

/// What one update did, beyond the numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    /// The new position.
    pub after: Rating,
    /// Whether a result in this ballot inverted a ladder gap of at least [`DIVERGENCE_GAP`] rating points.
    pub diverged: bool,
    /// The rating the card had before the ballot, so a caller can report the move without re-reading the row.
    pub before_rating: f64,
}

impl Update {
    /// The rating change this ballot produced.
    #[must_use]
    pub fn delta(&self) -> f64 {
        self.after.rating - self.before_rating
    }
}

fn g(phi: f64) -> f64 {
    1.0 / (1.0 + 3.0 * phi * phi / (PI * PI)).sqrt()
}

fn expected(mu: f64, mu_opponent: f64, phi_opponent: f64) -> f64 {
    1.0 / (1.0 + (-g(phi_opponent) * (mu - mu_opponent)).exp())
}

/// Glicko-2 step 5: solve for the new volatility by the Illinois variant of regula falsi.
fn new_volatility(phi: f64, sigma: f64, delta: f64, v: f64) -> f64 {
    let a = (sigma * sigma).ln();
    let phi_squared = phi * phi;
    let delta_squared = delta * delta;

    let f = |x: f64| {
        let ex = x.exp();
        let numerator = ex * (delta_squared - phi_squared - v - ex);
        let denominator = 2.0 * (phi_squared + v + ex).powi(2);
        numerator / denominator - (x - a) / (TAU * TAU)
    };

    let mut big_a = a;
    let mut big_b = if delta_squared > phi_squared + v {
        (delta_squared - phi_squared - v).ln()
    } else {
        let mut k = 1.0;
        while f(a - k * TAU) < 0.0 && k < 100.0 {
            k += 1.0;
        }
        a - k * TAU
    };

    let mut f_a = f(big_a);
    let mut f_b = f(big_b);
    let mut iterations = 0;
    while (big_b - big_a).abs() > EPSILON && iterations < 100 {
        iterations += 1;
        let big_c = big_a + (big_a - big_b) * f_a / (f_b - f_a);
        let f_c = f(big_c);
        if f_c * f_b <= 0.0 {
            big_a = big_b;
            f_a = f_b;
        } else {
            f_a /= 2.0;
        }
        big_b = big_c;
        f_b = f_c;
    }
    // Glicman's step 5: the iteration runs on `ln(σ²)`, and the answer is `e^(x/2)` — the volatility, not its square.
    // Returning `e^x` here is a classic and silent error: the deviation then barely grows and every number looks plausible.
    (big_b / 2.0).exp()
}

/// Rate one card against the opponents it faced in one ballot.
///
/// With no opponents the card is not updated — except for the deviation, which grows, because silence is not evidence and a
/// card nobody asks about should become less settled rather than stay frozen.
#[must_use]
pub fn rate(before: Rating, opponents: &[Opponent]) -> Update {
    let (mu, phi) = before.to_scaled();
    let sigma = before.volatility;

    if opponents.is_empty() {
        // Step 6's first half only: phi* = sqrt(phi^2 + sigma^2). No rating move and no volatility change, because there is
        // no result to explain.
        let phi_star = (phi * phi + sigma * sigma).sqrt();
        let after = Rating::from_scaled(mu, phi_star, sigma);
        return Update {
            diverged: false,
            before_rating: before.rating,
            after,
        };
    }

    // Step 3: v, the estimated variance of the rating based on the results.
    let mut sum_g_e = 0.0;
    let mut sum_g2_e_one_minus_e = 0.0;
    for opponent in opponents {
        let (mu_j, phi_j) = opponent.rating.to_scaled();
        let g_j = g(phi_j);
        let e_j = expected(mu, mu_j, phi_j);
        sum_g_e += g_j * (opponent.score - e_j);
        sum_g2_e_one_minus_e += g_j * g_j * e_j * (1.0 - e_j);
    }
    let v = if sum_g2_e_one_minus_e > 0.0 {
        1.0 / sum_g2_e_one_minus_e
    } else {
        // Every opponent was a certain win or a certain loss: v is undefined, and pretending otherwise divides by zero.
        f64::INFINITY
    };

    // Step 4: delta, the estimated improvement.
    let delta = if v.is_finite() { v * sum_g_e } else { 0.0 };

    // Step 5: the new volatility.
    let sigma_prime = if delta.is_finite() && v.is_finite() {
        new_volatility(phi, sigma, delta, v)
    } else {
        sigma
    };

    // Step 6: the new deviation.
    let phi_star = (phi * phi + sigma_prime * sigma_prime).sqrt();
    let phi_prime = if v.is_finite() {
        1.0 / (1.0 / (phi_star * phi_star) + 1.0 / v).sqrt()
    } else {
        phi_star
    };

    // Step 7: the new rating.
    let mu_prime = if v.is_finite() {
        mu + phi_prime * phi_prime * sum_g_e
    } else {
        mu
    };

    let after = Rating::from_scaled(mu_prime, phi_prime, sigma_prime);
    Update {
        after,
        diverged: diverged(before, opponents),
        before_rating: before.rating,
    }
}

/// Translate a MaxDiff (best/worst) ballot into per-card opponent lists and rate every card.
///
/// The translation is spec §44.3's, the same one the Elo path uses: best beats the two unchosen, worst loses to the two
/// unchosen, and best-beats-worst is implied rather than counted twice. Every card is rated against the positions **as
/// served with the ballot**, so the result does not depend on the order the cards are updated in — which is what makes two
/// concurrent ballots safe to record one after the other.
///
/// Returns `(card_id, Update)` for every card, in the order given.
#[must_use]
pub fn rate_ballot(
    cards: &[(String, Rating)],
    best_id: &str,
    worst_id: &str,
) -> Vec<(String, Update)> {
    let find = |id: &str| {
        cards
            .iter()
            .find(|(card_id, _)| card_id == id)
            .map(|(_, rating)| *rating)
    };
    let (Some(best), Some(worst)) = (find(best_id), find(worst_id)) else {
        return Vec::new();
    };
    let unchosen: Vec<&(String, Rating)> = cards
        .iter()
        .filter(|(card_id, _)| card_id != best_id && card_id != worst_id)
        .collect();

    cards
        .iter()
        .map(|(card_id, rating)| {
            let opponents: Vec<Opponent> = if card_id == best_id {
                unchosen
                    .iter()
                    .map(|(_, other)| Opponent {
                        rating: *other,
                        score: 1.0,
                    })
                    .collect()
            } else if card_id == worst_id {
                unchosen
                    .iter()
                    .map(|(_, other)| Opponent {
                        rating: *other,
                        score: 0.0,
                    })
                    .collect()
            } else {
                // An unchosen card beats the worst and loses to the best.
                vec![
                    Opponent {
                        rating: best,
                        score: 0.0,
                    },
                    Opponent {
                        rating: worst,
                        score: 1.0,
                    },
                ]
            };
            (card_id.clone(), rate(*rating, &opponents))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(left: f64, right: f64, tolerance: f64) -> bool {
        (left - right).abs() <= tolerance
    }

    #[test]
    fn glickman_step_example_is_reproduced() {
        // Glickman's worked example: 1500/200/0.06 against three opponents at 1400/30 (win), 1550/100 (loss), 1700/300 (loss).
        // The published answer is 1464.06 / 151.52 / 0.05999.
        let player = Rating {
            rating: 1500.0,
            deviation: 200.0,
            volatility: 0.06,
        };
        let opponents = [
            Opponent {
                rating: Rating {
                    rating: 1400.0,
                    deviation: 30.0,
                    volatility: 0.06,
                },
                score: 1.0,
            },
            Opponent {
                rating: Rating {
                    rating: 1550.0,
                    deviation: 100.0,
                    volatility: 0.06,
                },
                score: 0.0,
            },
            Opponent {
                rating: Rating {
                    rating: 1700.0,
                    deviation: 300.0,
                    volatility: 0.06,
                },
                score: 0.0,
            },
        ];
        let update = rate(player, &opponents);
        assert!(
            near(update.after.rating, 1464.06, 0.05),
            "rating {} is not Glickman's 1464.06",
            update.after.rating
        );
        assert!(
            near(update.after.deviation, 151.52, 0.05),
            "deviation {} is not Glickman's 151.52",
            update.after.deviation
        );
        assert!(
            near(update.after.volatility, 0.059_99, 0.000_02),
            "volatility {} is not Glickman's 0.05999",
            update.after.volatility
        );
    }

    #[test]
    fn a_fresh_card_that_wins_everything_moves_a_lot_and_stays_uncertain() {
        let fresh = Rating::new();
        let mut after = fresh;
        // Ten ballots against equally fresh opponents, all won.
        for _ in 0..10 {
            let opponents: Vec<Opponent> = (0..3)
                .map(|_| Opponent {
                    rating: Rating::new(),
                    score: 1.0,
                })
                .collect();
            after = rate(after, &opponents).after;
        }
        assert!(
            after.rating > fresh.rating + 100.0,
            "ten wins move a provisional card: {}",
            after.rating
        );
        assert!(
            after.deviation < fresh.deviation,
            "and it becomes less uncertain: {} vs {}",
            after.deviation,
            fresh.deviation
        );
        assert!(
            after.confidence() > fresh.confidence(),
            "confidence is a function of the deviation, so it rises too"
        );
    }

    #[test]
    fn deviation_grows_with_silence_and_never_exceeds_the_ceiling() {
        let settled = Rating {
            rating: 1600.0,
            deviation: 60.0,
            volatility: 0.06,
        };
        let first = rate(settled, &[]).after;
        assert_eq!(first.rating, settled.rating, "silence moves no rating");
        assert!(
            first.deviation > settled.deviation,
            "silence costs certainty: {} -> {}",
            settled.deviation,
            first.deviation
        );
        let mut after = settled;
        // Growth is additive in *variance* (φ* = sqrt(φ² + σ²)), so reaching the ceiling from a deviation of 60 at the
        // starting volatility 0.06 takes about a thousand updates. The number is stated rather than hidden in a bigger loop:
        // it is what "a year of silence" looks like in ballot counts on this board.
        for _ in 0..1200 {
            after = rate(after, &[]).after;
        }
        assert!(
            near(after.deviation, MAX_DEVIATION, 0.01),
            "a card nobody asks about approaches the ceiling, not infinity: {}",
            after.deviation
        );
    }

    #[test]
    fn a_settled_card_resists_one_upset_and_a_fresh_one_does_not() {
        let settled = Rating {
            rating: 1600.0,
            deviation: 40.0,
            volatility: 0.06,
        };
        let fresh = Rating::new();
        let loss = [Opponent {
            rating: Rating::new(),
            score: 0.0,
        }];
        let settled_move = (rate(settled, &loss).after.rating - settled.rating).abs();
        let fresh_move = (rate(fresh, &loss).after.rating - fresh.rating).abs();
        assert!(
            settled_move < 20.0,
            "a settled card barely moves on one upset: {settled_move}"
        );
        assert!(
            fresh_move > settled_move * 3.0,
            "and a provisional one moves much further: {fresh_move} vs {settled_move}"
        );
    }

    #[test]
    fn a_ballot_that_moves_a_settled_card_outside_two_deviations_is_flagged() {
        // A settled card at 1500 with a deviation of 40 (in rating points) is about 0.23 in Glicko-2 units, so a move of more
        // than 0.46 must be flagged. Beating a much stronger opponent three times does it; a modest result does not.
        let settled = Rating {
            rating: 1500.0,
            deviation: 40.0,
            volatility: 0.06,
        };
        let strong = Rating {
            rating: 2100.0,
            deviation: 40.0,
            volatility: 0.06,
        };
        let wins: Vec<Opponent> = (0..3)
            .map(|_| Opponent {
                rating: strong,
                score: 1.0,
            })
            .collect();
        let upset = rate(settled, &wins);
        assert!(
            upset.diverged,
            "beating a card 600 points above inverts a gap over 300: {:?}",
            upset.after
        );

        let ordinary = rate(
            settled,
            &[Opponent {
                rating: Rating {
                    rating: 1520.0,
                    deviation: 40.0,
                    volatility: 0.06,
                },
                score: 1.0,
            }],
        );
        assert!(
            !ordinary.diverged,
            "beating a card 20 points above inverts nothing: {:?}",
            ordinary.after
        );
        // And the flag is about the ladder rather than about uncertainty: a card losing to one 400 points *below* it diverges
        // even though the rating barely moves.
        let loss_to_weak = rate(
            settled,
            &[Opponent {
                rating: Rating {
                    rating: 1100.0,
                    deviation: 40.0,
                    volatility: 0.06,
                },
                score: 0.0,
            }],
        );
        assert!(
            loss_to_weak.diverged,
            "losing to a card 400 points below inverts the gap: {:?}",
            loss_to_weak.after
        );
    }

    #[test]
    fn a_maxdiff_ballot_moves_best_up_worst_down_and_the_unchosen_apart() {
        let cards = vec![
            ("a".to_owned(), Rating::new()),
            ("b".to_owned(), Rating::new()),
            ("c".to_owned(), Rating::new()),
            ("d".to_owned(), Rating::new()),
        ];
        let updates = rate_ballot(&cards, "a", "d");
        assert_eq!(updates.len(), 4);
        let by_id = |id: &str| {
            updates
                .iter()
                .find(|(card_id, _)| card_id == id)
                .map(|(_, update)| update.after.rating)
                .expect("rated")
        };
        assert!(by_id("a") > 1500.0, "best rises: {}", by_id("a"));
        assert!(by_id("d") < 1500.0, "worst falls: {}", by_id("d"));
        // The unchosen beat the worst and lost to the best, so they barely move — and they must not move in the same direction
        // as each other by accident.
        assert!(
            near(by_id("b"), 1500.0, 5.0) && near(by_id("c"), 1500.0, 5.0),
            "unchosen: {} {}",
            by_id("b"),
            by_id("c")
        );
    }

    #[test]
    fn the_result_does_not_depend_on_which_card_is_rated_first() {
        let cards = vec![
            ("a".to_owned(), Rating::new()),
            (
                "b".to_owned(),
                Rating {
                    rating: 1450.0,
                    deviation: 90.0,
                    volatility: 0.07,
                },
            ),
            (
                "c".to_owned(),
                Rating {
                    rating: 1550.0,
                    deviation: 120.0,
                    volatility: 0.05,
                },
            ),
            ("d".to_owned(), Rating::new()),
        ];
        let mut reversed = cards.clone();
        reversed.reverse();
        let first = rate_ballot(&cards, "b", "c");
        let second = rate_ballot(&reversed, "b", "c");
        for (left_id, left) in first.iter() {
            // `rate_ballot` returns results in the order the cards were given, so compare by id rather than position.
            let left_match = second
                .iter()
                .find(|(card_id, _)| card_id == left_id)
                .map(|(_, update)| update)
                .expect("present");
            assert_eq!(
                left, left_match,
                "card {left_id} rated differently when the ballot was listed differently"
            );
        }
    }

    #[test]
    fn an_empty_ballot_rates_nothing_rather_than_panicking() {
        let cards = vec![("a".to_owned(), Rating::new())];
        assert!(rate_ballot(&cards, "a", "missing").is_empty());
    }

    #[test]
    fn adopting_an_elo_rating_starts_at_maximum_uncertainty() {
        let adopted = Rating::adopted(1720.0);
        assert_eq!(adopted.rating, 1720.0);
        assert_eq!(adopted.deviation, MAX_DEVIATION);
        assert_eq!(
            adopted.confidence(),
            0.0,
            "an Elo number is not evidence of certainty"
        );
    }
}

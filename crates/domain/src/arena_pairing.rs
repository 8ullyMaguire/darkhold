//! M69-11 — which four works the arena shows, and what a vote on them is allowed to mean.
//!
//! # Pairing is a choice with consequences, not a shuffle
//!
//! The arena asks for a forced tradeoff between four works. Which four decides what the ballot can teach:
//!
//! * Four works already at the same rating confirm what the ladder believes. That is cheap information and it is what a
//!   settled ladder wants — **proximate pairing**.
//! * Four works at the extremes teach where a work sits rather than re-stating it — **exploration** — at the cost of a
//!   comparison that may be so obvious it carries little signal.
//!
//! So the default is proximate, and a small share of rounds (epsilon) explores. The roll is **passed in**, not drawn
//! here: a pairing function that calls its own RNG cannot be tested for what it does at epsilon, and "sometimes explores"
//! becomes a claim about a coin rather than about a decision.
//!
//! # Deviation is the second half of "similar"
//!
//! Two works both at 1500 are not equally good pairing partners: one measured over two hundred ballots with a deviation
//! of 60, the other over two. Glicko-2's deviation is exactly the statement "I do not know where this work sits", so
//! within an equally tight window the round prefers the cards whose deviation is largest — the comparison that can still
//! move a number. Proximity decides the window; deviation breaks the tie.
//!
//! # The calibration window: recorded, not applied
//!
//! A work in its first few ballots is being positioned, not ranked. Its rating is mostly the first voter's taste with an
//! error bar, and letting that number sit on the global ladder as though it were shared judgement is how a new work
//! inherits one reader as a fact. So a round **names** the cards inside their calibration window ([`CALIBRATION_MATCHES`])
//! and the ballot records that fact. What it does not do is refuse the vote, hide the card, or adjust the ladder by hand:
//! the window is a label on the evidence, and whoever ranks by it decides what to do — including the estate's taste
//! service, which owns the arithmetic (ADR 0001).
//!
//! A card with no reading at all is treated as inside its window: absent evidence is not evidence of being settled.

use serde::{Deserialize, Serialize};

use crate::taste_vector::{dimension_index, ArenaCard, ArenaRound, TasteDimension};

/// The rating a work starts at when the ladder has never seen it (Glicko-2's own starting value).
pub const DEFAULT_RATING: f64 = 1_500.0;
/// Glicko-2's initial deviation: "I know nothing about where this work sits".
pub const INITIAL_DEVIATION: f64 = 350.0;
/// The share of rounds that explore instead of pairing proximately.
pub const DEFAULT_EPSILON: f64 = 0.15;
/// How many ballots a work may appear in before its rating counts as a shared judgement rather than a first reader.
pub const CALIBRATION_MATCHES: u64 = 5;

/// What the ladder currently says about one card.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArenaSignal {
    /// The Glicko-2 rating.
    pub rating: f64,
    /// The deviation, when the service reported it. `None` is "not reported", never zero — an unknown deviation is
    /// maximal uncertainty, which is the opposite of a settled card.
    pub rd: Option<f64>,
    /// How many comparisons the rating rests on.
    pub matches: u64,
}

impl ArenaSignal {
    /// The deviation to reason with: the reported one, or the initial 350 for a card nobody has measured.
    #[must_use]
    pub fn deviation(&self) -> f64 {
        self.rd
            .filter(|rd| rd.is_finite() && *rd >= 0.0)
            .unwrap_or(INITIAL_DEVIATION)
    }

    /// Whether this card is still being positioned.
    #[must_use]
    pub fn calibrating(&self, calibration_matches: u64) -> bool {
        self.matches < calibration_matches
    }
}

/// How a round was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingMode {
    /// Four cards close together on the ladder: the comparison sharpens what is already believed.
    Proximate,
    /// Four cards spread apart: the comparison locates a work instead of confirming it.
    Explore,
}

impl PairingMode {
    /// The word the client and the ledger use.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Proximate => "proximate",
            Self::Explore => "explore",
        }
    }
}

/// The knobs. Passed in rather than read from config here so the domain stays a pure function of its arguments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairingConfig {
    /// Probability in `0..=1` of exploring. Clamped on use.
    pub epsilon: f64,
    /// Ballots before a work's rating counts as settled.
    pub calibration_matches: u64,
}

impl Default for PairingConfig {
    fn default() -> Self {
        Self {
            epsilon: DEFAULT_EPSILON,
            calibration_matches: CALIBRATION_MATCHES,
        }
    }
}

/// A chosen round plus the reasoning that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct PairingOutcome {
    /// The four cards, in the order they will be presented.
    pub cards: Vec<ArenaCard>,
    /// Which side of the epsilon decision this round fell on.
    pub mode: PairingMode,
    /// The cards still inside their calibration window, in presentation order.
    pub calibrating: Vec<String>,
    /// One sentence for the client and the log: why these four.
    pub reason: String,
}

/// Where a card sits on the ladder, with unknown ratings treated as the starting value.
fn rating_of(card: &ArenaCard) -> f64 {
    card.signal
        .map_or(DEFAULT_RATING, |signal| signal.rating)
        .clamp(-10_000.0, 10_000.0)
}

fn deviation_of(card: &ArenaCard) -> f64 {
    card.signal
        .map_or(INITIAL_DEVIATION, |signal| signal.deviation())
}

fn calibrating_of(card: &ArenaCard, calibration_matches: u64) -> bool {
    card.signal
        .is_none_or(|signal| signal.calibrating(calibration_matches))
}

/// Choose the round.
///
/// `roll` is a uniform draw in `0..1` supplied by the caller; the round explores when `roll < epsilon`. This is the whole
/// randomness of the function, which is what makes "explores about 15% of the time" testable instead of hopeful.
///
/// Returns `None` for the same two ordinary states [`crate::taste_vector::generate_arena_round`] refuses: no usable
/// dimension to vary on, or no fandom holding four cards. `None` is "there is no round to offer", not a fault.
#[must_use]
pub fn select_round(
    pool: &[ArenaCard],
    dimensions: &[TasteDimension],
    target_dimensions: &[String],
    config: PairingConfig,
    roll: f64,
) -> Option<PairingOutcome> {
    if pool.len() < 4 {
        return None;
    }
    let targets: Vec<String> = target_dimensions
        .iter()
        .filter(|key| dimension_index(dimensions, key).is_some())
        .cloned()
        .collect();
    if targets.is_empty() {
        return None;
    }

    // The same pairing rule the arena has always used: four works that share a fandom. Grouping lives here rather than in
    // SQL because it is the arena's presentation rule, and a group of three must fall through to "no round", not to a
    // three-card tradeoff.
    let mut groups: std::collections::BTreeMap<&str, Vec<&ArenaCard>> =
        std::collections::BTreeMap::new();
    for card in pool {
        groups.entry(card.fandom.as_str()).or_default().push(card);
    }
    let group = groups.values().find(|members| members.len() >= 4)?;

    let exploring = {
        let epsilon = if config.epsilon.is_finite() {
            config.epsilon.clamp(0.0, 1.0)
        } else {
            0.0
        };
        roll.is_finite() && roll >= 0.0 && roll < epsilon
    };
    let mode = if exploring {
        PairingMode::Explore
    } else {
        PairingMode::Proximate
    };

    // Sort by rating and slide a window of four. Proximity minimises the span, exploration maximises it; both break ties
    // on the largest mean deviation, because within an equally good window the cards that can still move are the ones
    // worth asking about.
    let mut ordered: Vec<&ArenaCard> = group.clone();
    ordered.sort_by(|a, b| {
        rating_of(a)
            .partial_cmp(&rating_of(b))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.work_id.cmp(&b.work_id))
    });

    let mut best: Option<&[&ArenaCard]> = None;
    let mut best_key = (0.0_f64, 0.0_f64);
    for window in ordered.windows(4) {
        let span = rating_of(window[3]) - rating_of(window[0]);
        let mean_deviation = window.iter().map(|card| deviation_of(card)).sum::<f64>() / 4.0;
        let better = match best {
            None => true,
            Some(_) if exploring => {
                span > best_key.0 || (span == best_key.0 && mean_deviation > best_key.1)
            }
            Some(_) => span < best_key.0 || (span == best_key.0 && mean_deviation > best_key.1),
        };
        if better {
            best = Some(window);
            best_key = (span, mean_deviation);
        }
    }
    let chosen = best?;

    let cards: Vec<ArenaCard> = chosen.iter().map(|card| (*card).clone()).collect();
    let calibrating: Vec<String> = cards
        .iter()
        .filter(|card| calibrating_of(card, config.calibration_matches))
        .map(|card| card.work_id.clone())
        .collect();

    let mut reason = match mode {
        // A zero span means exploration had nothing to explore: every card is at the starting value, so the four are
        // interchangeable and the ballot calibrates rather than locates. Saying "these four span 0 points, so the vote
        // locates a work" would be a sentence that contradicts its own numbers.
        PairingMode::Explore if best_key.0 == 0.0 => "exploring: no card here has a rating yet, so the choice among              them is arbitrary and the ballot calibrates rather than locates a work"
            .to_owned(),
        PairingMode::Explore => format!(
            "exploring: these four span {:.0} rating points, so the vote locates a work rather than confirming it",
            best_key.0
        ),
        PairingMode::Proximate => format!(
            "proximate: these four sit within {:.0} rating points of each other, mean deviation {:.0}",
            best_key.0, best_key.1
        ),
    };
    if !calibrating.is_empty() {
        reason.push_str(&format!(
            "; {} of them {} still calibrating and their ratings will not be treated as the ladder's",
            calibrating.len(),
            if calibrating.len() == 1 { "is" } else { "are" }
        ));
    }

    Some(PairingOutcome {
        cards,
        mode,
        calibrating,
        reason,
    })
}

/// The round shape the arena route returns, with the pairing decision attached.
///
/// A separate type from [`ArenaRound`] rather than three new fields on it: `ArenaRound` is also constructed by the vote
/// route from the two cards a ballot named, where there is no pairing decision to report — and a struct whose extra
/// fields are always `None`/empty teaches the next reader that they are optional when they are not.
#[derive(Debug, Clone, PartialEq)]
pub struct ArenaPairing {
    pub round: ArenaRound,
    pub mode: PairingMode,
    pub calibrating: Vec<String>,
    pub reason: String,
}

impl PairingOutcome {
    /// The round shape, carrying the decision.
    #[must_use]
    pub fn into_round(self) -> ArenaPairing {
        ArenaPairing {
            round: ArenaRound { cards: self.cards },
            mode: self.mode,
            calibrating: self.calibrating,
            reason: self.reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dimensions() -> Vec<TasteDimension> {
        vec![TasteDimension {
            key: "prose".to_owned(),
            label: "Prose".to_owned(),
            admin_target: 0.5,
            weight: 1.0,
        }]
    }

    fn card(id: &str, rating: Option<f64>, rd: Option<f64>, matches: u64) -> ArenaCard {
        ArenaCard {
            work_id: id.to_owned(),
            title: id.to_owned(),
            fandom: "Same Fandom".to_owned(),
            tags: vec![],
            word_count: 1_000,
            excerpt: String::new(),
            target_dimension: "prose".to_owned(),
            vector: vec![0.5],
            signal: rating.map(|rating| ArenaSignal {
                rating,
                rd,
                matches,
            }),
        }
    }

    fn targets() -> Vec<String> {
        vec!["prose".to_owned()]
    }

    #[test]
    fn proximate_picks_the_four_closest_together_and_leaves_the_outliers_alone() {
        let pool = vec![
            card("a", Some(1_400.0), Some(80.0), 40),
            card("b", Some(1_420.0), Some(90.0), 40),
            card("c", Some(1_450.0), Some(70.0), 40),
            card("d", Some(1_460.0), Some(85.0), 40),
            card("far", Some(1_900.0), Some(60.0), 40),
            card("farther", Some(900.0), Some(60.0), 40),
        ];
        let outcome = select_round(
            &pool,
            &dimensions(),
            &targets(),
            PairingConfig::default(),
            0.5,
        )
        .expect("a round");
        assert_eq!(outcome.mode, PairingMode::Proximate);
        let ids: Vec<&str> = outcome.cards.iter().map(|c| c.work_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["a", "b", "c", "d"],
            "the tight four, not the spread six"
        );
        assert!(outcome.reason.contains("proximate"), "{}", outcome.reason);
    }

    #[test]
    fn deviation_breaks_a_tie_between_equally_tight_windows() {
        // Two windows with the same 100-point span; the second is measured far less.
        let pool = vec![
            card("a", Some(1_000.0), Some(50.0), 80),
            card("b", Some(1_050.0), Some(50.0), 80),
            card("c", Some(1_080.0), Some(50.0), 80),
            card("d", Some(1_100.0), Some(50.0), 80),
            card("e", Some(1_200.0), Some(300.0), 1),
            card("f", Some(1_250.0), Some(300.0), 1),
            card("g", Some(1_280.0), Some(300.0), 1),
            card("h", Some(1_300.0), Some(300.0), 1),
        ];
        let outcome = select_round(
            &pool,
            &dimensions(),
            &targets(),
            PairingConfig::default(),
            0.5,
        )
        .expect("a round");
        let ids: Vec<&str> = outcome.cards.iter().map(|c| c.work_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["e", "f", "g", "h"],
            "equal span, and these four are the ones the vote can still move"
        );
        assert_eq!(
            outcome.calibrating, ids,
            "one ballot each is inside the window"
        );
    }

    #[test]
    fn a_roll_under_epsilon_explores_and_takes_the_widest_four() {
        let pool = vec![
            card("a", Some(1_000.0), Some(80.0), 40),
            card("b", Some(1_050.0), Some(80.0), 40),
            card("c", Some(1_080.0), Some(80.0), 40),
            card("d", Some(1_100.0), Some(80.0), 40),
            card("e", Some(1_800.0), Some(200.0), 2),
        ];
        let config = PairingConfig {
            epsilon: 0.2,
            ..PairingConfig::default()
        };
        let explored = select_round(&pool, &dimensions(), &targets(), config, 0.05).expect("round");
        assert_eq!(explored.mode, PairingMode::Explore);
        let ids: Vec<&str> = explored.cards.iter().map(|c| c.work_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["b", "c", "d", "e"],
            "the widest window, not the tightest"
        );
        assert!(explored.reason.contains("exploring"), "{}", explored.reason);

        // The same pool and the same four under a roll outside epsilon: no exploration, no ambiguity.
        let paired = select_round(&pool, &dimensions(), &targets(), config, 0.9).expect("round");
        assert_eq!(paired.mode, PairingMode::Proximate);
        assert_ne!(
            paired
                .cards
                .iter()
                .map(|c| c.work_id.clone())
                .collect::<Vec<_>>(),
            ids,
            "the two modes must not converge on the same four here, or the test proves nothing"
        );
    }

    #[test]
    fn a_card_with_no_reading_at_all_is_calibrating_rather_than_settled_at_zero() {
        let pool = vec![
            card("rated", Some(1_500.0), Some(60.0), 50),
            card("rated2", Some(1_510.0), Some(60.0), 50),
            card("rated3", Some(1_520.0), Some(60.0), 50),
            card("unrated", None, None, 0),
        ];
        let outcome = select_round(
            &pool,
            &dimensions(),
            &targets(),
            PairingConfig::default(),
            0.5,
        )
        .expect("a round");
        assert_eq!(outcome.calibrating, vec!["unrated".to_owned()]);
        assert!(
            outcome.reason.contains("calibrating"),
            "the round says so: {}",
            outcome.reason
        );
    }

    #[test]
    fn three_cards_or_no_dimension_is_no_round_rather_than_a_smaller_one() {
        let three = vec![
            card("a", Some(1_500.0), None, 9),
            card("b", Some(1_500.0), None, 9),
            card("c", Some(1_500.0), None, 9),
        ];
        assert_eq!(
            select_round(
                &three,
                &dimensions(),
                &targets(),
                PairingConfig::default(),
                0.0
            ),
            None
        );
        let four: Vec<ArenaCard> = (0..4)
            .map(|n| card(&format!("w{n}"), None, None, 0))
            .collect();
        assert_eq!(
            select_round(
                &four,
                &dimensions(),
                &["missing".to_owned()],
                PairingConfig::default(),
                0.0
            ),
            None,
            "a round varying on no known dimension teaches nothing"
        );
        // And the same four are offerable once a real dimension is named.
        assert!(select_round(
            &four,
            &dimensions(),
            &targets(),
            PairingConfig::default(),
            0.0
        )
        .is_some());
    }

    #[test]
    fn epsilon_zero_never_explores_and_one_always_does() {
        let pool: Vec<ArenaCard> = (0..6)
            .map(|n| {
                card(
                    &format!("w{n}"),
                    Some(1_500.0 + f64::from(n) * 100.0),
                    Some(90.0),
                    30,
                )
            })
            .collect();
        let never = PairingConfig {
            epsilon: 0.0,
            ..PairingConfig::default()
        };
        for roll in [0.0, 0.5, 0.999_999] {
            let outcome =
                select_round(&pool, &dimensions(), &targets(), never, roll).expect("round");
            assert_eq!(outcome.mode, PairingMode::Proximate, "roll {roll}");
        }
        let always = PairingConfig {
            epsilon: 1.0,
            ..PairingConfig::default()
        };
        let outcome = select_round(&pool, &dimensions(), &targets(), always, 0.999).expect("round");
        assert_eq!(outcome.mode, PairingMode::Explore);
    }

    #[test]
    fn the_same_roll_gives_the_same_round() {
        let pool: Vec<ArenaCard> = (0..8)
            .map(|n| {
                card(
                    &format!("w{n}"),
                    Some(1_400.0 + f64::from(n) * 50.0),
                    Some(120.0),
                    12,
                )
            })
            .collect();
        let first = select_round(
            &pool,
            &dimensions(),
            &targets(),
            PairingConfig::default(),
            0.42,
        );
        let second = select_round(
            &pool,
            &dimensions(),
            &targets(),
            PairingConfig::default(),
            0.42,
        );
        assert_eq!(first, second, "no hidden RNG, so a rerun is a rerun");
    }
}

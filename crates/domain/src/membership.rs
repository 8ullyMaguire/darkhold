//! Taste-gated membership: a probation for new readers once an instance is big enough (M69-15).
//!
//! The row is the one with "the most ways to be cruel", so the exits are part of the rule rather than policy prose:
//!
//! * **Probation is time-bounded.** It ends on a date, not on an operator's mood.
//! * **The floor is configurable, and rises with the instance.** A thousand readers and a hundred thousand cannot ask the
//!   same amount of a newcomer, and the amount has to be a number the operator can read.
//! * **Existing readers are grandfathered.** A gate that appears one morning and puts the current membership on probation
//!   is a gate that punishes people for having arrived earlier.
//! * **A reader who does not graduate is redirected, never deleted.** Deletion is the one outcome a probation must not have:
//!   the account holds bookmarks, reading positions and a pseud, and the person may come back.
//! * **Default off.** This changes what a stranger sees, so it waits for an operator's decision rather than shipping on.
//!
//! The rule is pure — population, the reader's age and their taste samples in, a standing out — because every one of those
//! five properties is a claim about a boundary, and a boundary is only testable where the clock and the database are absent.

/// The configured rules, by their `[governance]` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MembershipRules {
    /// `governance.membership_probation` — off unless an operator turned it on.
    pub enabled: bool,
    /// `governance.membership_population_threshold` — below this many readers, nobody is on probation.
    pub population_threshold: i64,
    /// `governance.membership_probation_days` — how long the probation lasts.
    pub probation_days: i64,
    /// `governance.membership_grace_days` — how long after probation a reader is still redirected rather than closed.
    pub grace_days: i64,
    /// `governance.membership_floor_base` — the taste samples a newcomer needs at the threshold population.
    pub floor_base: i64,
    /// `governance.membership_floor_per_thousand` — how much the floor rises for each thousand readers beyond it.
    pub floor_per_thousand: i64,
}

impl Default for MembershipRules {
    fn default() -> Self {
        Self {
            enabled: false,
            population_threshold: 1_000,
            probation_days: 14,
            grace_days: 30,
            floor_base: 3,
            floor_per_thousand: 2,
        }
    }
}

impl MembershipRules {
    /// The number of taste samples a newcomer needs at `population`.
    ///
    /// Rises with the instance and never falls: an instance that loses readers does not un-learn what it asked of the ones
    /// it has, because a floor that moves down would put readers who already graduated back on probation.
    #[must_use]
    pub fn floor_at(&self, population: i64) -> i64 {
        let beyond = (population - self.population_threshold).max(0);
        self.floor_base + self.floor_per_thousand * (beyond / 1_000)
    }
}

/// What the instance knows about one reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReaderFacts {
    /// Days since the account was created.
    pub age_days: i64,
    /// How many taste samples (ratings with enough signal to count) the reader has contributed.
    pub taste_samples: i64,
    /// Whether the account existed before the gate was switched on, and is therefore a full member by right.
    pub grandfathered: bool,
}

/// Where a reader stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// A full member: the gate does not apply, or they graduated.
    Full {
        /// Why, for an operator who has to explain a decision to a person.
        because: Reason,
    },
    /// Still inside the window: the reader may read, and the instance says what would finish the probation.
    Probation {
        /// Days left in the window, never negative.
        days_left: i64,
        /// Taste samples still needed.
        samples_needed: i64,
    },
    /// Out of window and out of grace: the reader is redirected with a passport, not deleted.
    Redirect {
        /// Taste samples they still lacked when the window closed.
        samples_short: i64,
    },
}

/// Why a reader holds a full membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// `governance.membership_probation` is off.
    GateOff,
    /// The instance is below `membership_population_threshold`.
    BelowThreshold,
    /// The account predates the gate.
    Grandfathered,
    /// The floor was met, inside the window or before it.
    Graduated,
}

/// Decide a reader's standing.
#[must_use]
pub fn decide(rules: &MembershipRules, reader: ReaderFacts, population: i64) -> Standing {
    if !rules.enabled {
        return Standing::Full {
            because: Reason::GateOff,
        };
    }
    if population < rules.population_threshold {
        return Standing::Full {
            because: Reason::BelowThreshold,
        };
    }
    if reader.grandfathered {
        return Standing::Full {
            because: Reason::Grandfathered,
        };
    }

    let floor = rules.floor_at(population);
    let samples_needed = (floor - reader.taste_samples).max(0);
    if samples_needed == 0 {
        // **Earning it early is allowed.** A newcomer who reads and rates on their first day is exactly the reader the
        // gate is looking for; making them wait out the clock would teach them the clock is what matters.
        return Standing::Full {
            because: Reason::Graduated,
        };
    }

    if reader.age_days < rules.probation_days {
        return Standing::Probation {
            days_left: rules.probation_days - reader.age_days,
            samples_needed,
        };
    }
    if reader.age_days < rules.probation_days + rules.grace_days {
        // The window closed but the grace period has not: still redirected to the passport, still able to finish.
        return Standing::Probation {
            days_left: rules.probation_days + rules.grace_days - reader.age_days,
            samples_needed,
        };
    }
    Standing::Redirect {
        samples_short: samples_needed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> MembershipRules {
        MembershipRules {
            enabled: true,
            ..MembershipRules::default()
        }
    }

    fn reader(age_days: i64, taste_samples: i64) -> ReaderFacts {
        ReaderFacts {
            age_days,
            taste_samples,
            grandfathered: false,
        }
    }

    #[test]
    fn the_gate_is_off_unless_it_is_switched_on() {
        let rules = MembershipRules::default();
        assert!(
            !rules.enabled,
            "M69-15 ships off: it changes what a stranger sees"
        );
        let standing = decide(&rules, reader(1, 0), 1_000_000);
        assert_eq!(
            standing,
            Standing::Full {
                because: Reason::GateOff
            },
            "a switch that is off cannot put a reader on probation"
        );
    }

    #[test]
    fn a_small_instance_has_no_probation() {
        let standing = decide(&on(), reader(1, 0), 400);
        assert_eq!(
            standing,
            Standing::Full {
                because: Reason::BelowThreshold
            }
        );
    }

    #[test]
    fn a_newcomer_on_a_large_instance_is_on_probation_with_its_end_in_sight() {
        let standing = decide(&on(), reader(3, 0), 5_000);
        match standing {
            Standing::Probation {
                days_left,
                samples_needed,
            } => {
                assert_eq!(days_left, 11, "14 days, 3 spent");
                assert_eq!(samples_needed, 11, "floor at 5,000 readers is 3 + 2*4 = 11");
            }
            other => panic!("expected probation, got {other:?}"),
        }
    }

    #[test]
    fn the_floor_rises_with_the_instance_and_never_falls() {
        let rules = on();
        assert_eq!(rules.floor_at(1_000), 3, "at the threshold, the base");
        assert_eq!(rules.floor_at(5_000), 11);
        assert_eq!(rules.floor_at(20_000), 41);
        assert!(
            rules.floor_at(400) <= rules.floor_at(1_000),
            "below the threshold the floor does not go below the base"
        );
        assert!(
            rules.floor_at(9_000) <= rules.floor_at(10_000),
            "a floor that fell would put graduates back on probation"
        );
    }

    #[test]
    fn earning_the_floor_early_is_graduation() {
        let standing = decide(&on(), reader(1, 12), 5_000);
        assert_eq!(
            standing,
            Standing::Full {
                because: Reason::Graduated
            },
            "the gate is about taste, not about waiting"
        );
    }

    #[test]
    fn an_account_that_predates_the_gate_is_grandfathered() {
        let facts = ReaderFacts {
            age_days: 900,
            taste_samples: 0,
            grandfathered: true,
        };
        assert_eq!(
            decide(&on(), facts, 100_000),
            Standing::Full {
                because: Reason::Grandfathered
            }
        );
    }

    #[test]
    fn the_window_ends_and_the_grace_is_the_last_chance() {
        let rules = on();
        // 14 days of probation, 30 of grace: day 20 is still probation, day 44 is not.
        assert!(matches!(
            decide(&rules, reader(20, 0), 5_000),
            Standing::Probation { days_left: 24, .. }
        ));
        assert_eq!(
            decide(&rules, reader(44, 0), 5_000),
            Standing::Redirect { samples_short: 11 },
            "a reader who never graduated is redirected, never deleted"
        );
    }
}

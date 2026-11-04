//! M45-48 — what a machine is, what that buys, and what it withholds.
//!
//! Two names, kept apart on purpose. `Capacity` is what the machine **is**: cores, memory, engine,
//! whether a cache is configured. `Tier` is what that **buys**. The derivation is a pure function over
//! `Capacity`, so a test can sit on either side of every threshold without owning a small computer —
//! and so the answer for "why are embeddings off" is a comparison, not an opinion.
//!
//! This is not the instance *preset* (`analytics::Preset`, spec §0.6). A preset is what an operator
//! decided this instance is **for**; a tier is what the hardware can **afford**. Conflating them would
//! report "this instance deliberately does not do analytics" and "this machine cannot" as one fact.

/// The storage engine, as a capacity fact.
///
/// It is a fact and not an assumption about "big machines" because incremental analytics needs the
/// engine's write concurrency: eight cores behind SQLite is a `Small` instance, and saying otherwise
/// would promise a surface the engine cannot deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Sqlite,
    Postgres,
}

impl Engine {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }
}

/// What the machine is. Facts, injected — never measured inside the rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capacity {
    pub cores: usize,
    pub memory_mb: u64,
    pub engine: Engine,
    pub has_redis: bool,
}

/// What `Capacity` buys. Ordered: `Minimal < Small < Standard < Large`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Minimal,
    Small,
    Standard,
    Large,
}

impl Tier {
    /// The lowest tier whose requirements are all met.
    ///
    /// The rule is "lowest that clears", not "highest that nearly clears": a machine that clears eight
    /// cores but runs SQLite is `Small`, because the tier that promises incremental analytics requires
    /// the engine that can serve it. Each tier's requirements are stated once, here, and a test sits on
    /// either side of each.
    #[must_use]
    pub fn of(capacity: &Capacity) -> Self {
        if capacity.cores >= 8
            && capacity.memory_mb >= 8_192
            && capacity.engine == Engine::Postgres
            && capacity.has_redis
        {
            Self::Large
        } else if capacity.cores >= 4
            && capacity.memory_mb >= 4_096
            && capacity.engine == Engine::Postgres
        {
            Self::Standard
        } else if capacity.cores >= 2 && capacity.memory_mb >= 1_024 {
            Self::Small
        } else {
            Self::Minimal
        }
    }

    /// The requirements this tier asks for, as a sentence an operator can read.
    #[must_use]
    pub fn requirement(self) -> &'static str {
        match self {
            Self::Minimal => "any machine",
            Self::Small => "2 cores and 1024 MB",
            Self::Standard => "4 cores, 4096 MB and PostgreSQL",
            Self::Large => "8 cores, 8192 MB, PostgreSQL and a cache",
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Small => "small",
            Self::Standard => "standard",
            Self::Large => "large",
        }
    }

    /// What this tier affords. A **ceiling**, never a floor: nothing here switches on a surface the
    /// operator turned off, and nothing here switches off a surface the machine can afford.
    #[must_use]
    pub fn report(self) -> TierReport {
        match self {
            Self::Minimal => TierReport {
                embeddings: false,
                incremental_analytics: false,
                batch_size: 50,
                jobs: 1,
            },
            Self::Small => TierReport {
                embeddings: false,
                incremental_analytics: false,
                batch_size: 200,
                jobs: 2,
            },
            Self::Standard => TierReport {
                embeddings: true,
                incremental_analytics: true,
                batch_size: 500,
                jobs: 4,
            },
            Self::Large => TierReport {
                embeddings: true,
                incremental_analytics: true,
                batch_size: 2_000,
                jobs: 8,
            },
        }
    }
}

/// What a tier affords, as published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct TierReport {
    /// Whether vector embeddings are affordable at all — the row's own example.
    pub embeddings: bool,
    /// Whether analytics run incrementally or as a weekly batch.
    pub incremental_analytics: bool,
    /// Rows per batch when a job does have to sweep.
    pub batch_size: u32,
    /// How many jobs may run at once.
    pub jobs: u32,
}

/// An operator's word for the tier, or `Auto` to let the measurement answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierChoice {
    Auto,
    Fixed(Tier),
}

/// Parse `[capacity] tier`. An unknown name is refused **with the list**, because a typo that silently
/// meant `auto` would make an operator's decision disappear without a word.
pub fn parse_tier(raw: &str) -> Result<TierChoice, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "auto" | "" => Ok(TierChoice::Auto),
        "minimal" => Ok(TierChoice::Fixed(Tier::Minimal)),
        "small" => Ok(TierChoice::Fixed(Tier::Small)),
        "standard" => Ok(TierChoice::Fixed(Tier::Standard)),
        "large" => Ok(TierChoice::Fixed(Tier::Large)),
        other => Err(format!(
            "{other}: not a capacity tier — expected auto, minimal, small, standard or large"
        )),
    }
}

/// The tier that answers: the operator's word when they gave one, the measurement otherwise.
#[must_use]
pub fn resolve(capacity: &Capacity, choice: TierChoice) -> Tier {
    match choice {
        TierChoice::Auto => Tier::of(capacity),
        TierChoice::Fixed(tier) => tier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capacity(cores: usize, memory_mb: u64, engine: Engine, has_redis: bool) -> Capacity {
        Capacity {
            cores,
            memory_mb,
            engine,
            has_redis,
        }
    }

    #[test]
    fn each_threshold_is_cleared_at_the_number_and_missed_below_it() {
        // Small: both facts at the line, and one core under it.
        assert_eq!(
            Tier::of(&capacity(2, 1_024, Engine::Sqlite, false)),
            Tier::Small
        );
        assert_eq!(
            Tier::of(&capacity(1, 1_024, Engine::Sqlite, false)),
            Tier::Minimal
        );
        assert_eq!(
            Tier::of(&capacity(2, 1_023, Engine::Sqlite, false)),
            Tier::Minimal,
            "1023 MB is not 1024"
        );

        // Standard needs the engine as well as the numbers.
        assert_eq!(
            Tier::of(&capacity(4, 4_096, Engine::Postgres, false)),
            Tier::Standard
        );
        assert_eq!(
            Tier::of(&capacity(4, 4_095, Engine::Postgres, false)),
            Tier::Small,
            "one megabyte short is short"
        );

        // Large needs the cache too.
        assert_eq!(
            Tier::of(&capacity(8, 8_192, Engine::Postgres, true)),
            Tier::Large
        );
        assert_eq!(
            Tier::of(&capacity(8, 8_192, Engine::Postgres, false)),
            Tier::Standard,
            "without a cache it is the tier below, not the one with the number"
        );
    }

    #[test]
    fn the_engine_beats_the_cores() {
        assert_eq!(
            Tier::of(&capacity(64, 131_072, Engine::Sqlite, true)),
            Tier::Small,
            "a huge machine on SQLite cannot serve incremental analytics, and the tier that promises \
             it requires the engine that can"
        );
    }

    #[test]
    fn a_tier_only_withholds_more_as_it_shrinks() {
        let tiers = [Tier::Minimal, Tier::Small, Tier::Standard, Tier::Large];
        for pair in tiers.windows(2) {
            let (small, big) = (pair[0].report(), pair[1].report());
            assert!(
                big.batch_size >= small.batch_size,
                "{} has a smaller batch than {}",
                pair[1].as_str(),
                pair[0].as_str()
            );
            assert!(
                big.jobs >= small.jobs,
                "{} runs fewer jobs than {}",
                pair[1].as_str(),
                pair[0].as_str()
            );
            assert!(
                !(big.embeddings && !small.embeddings && big.batch_size == small.batch_size),
                "a tier that turns embeddings on with no other change is suspicious"
            );
        }
    }

    #[test]
    fn embeddings_are_withheld_until_the_standard_tier() {
        assert!(!Tier::Minimal.report().embeddings);
        assert!(
            !Tier::Small.report().embeddings,
            "the row's own example: SQLite instances run weekly batches and no embeddings"
        );
        assert!(Tier::Standard.report().embeddings);
        assert!(Tier::Standard.report().incremental_analytics);
        assert!(!Tier::Small.report().incremental_analytics);
    }

    #[test]
    fn a_name_is_parsed_and_a_typo_is_refused_with_the_list() {
        assert_eq!(parse_tier("auto"), Ok(TierChoice::Auto));
        assert_eq!(parse_tier(""), Ok(TierChoice::Auto));
        assert_eq!(parse_tier(" Large "), Ok(TierChoice::Fixed(Tier::Large)));
        let failure = parse_tier("enormous").expect_err("refused");
        assert!(
            failure.contains("minimal, small, standard or large"),
            "{failure}"
        );
        assert!(
            failure.starts_with("enormous"),
            "the refusal names what was written: {failure}"
        );
    }

    #[test]
    fn an_operators_word_answers_and_the_measurement_is_still_there_to_disagree_with() {
        let small = capacity(2, 1_024, Engine::Sqlite, false);
        assert_eq!(resolve(&small, TierChoice::Auto), Tier::Small);
        assert_eq!(resolve(&small, TierChoice::Fixed(Tier::Large)), Tier::Large);
        // `Capacity` is untouched by the choice: the route can publish both.
        assert_eq!(small.cores, 2);
        assert_eq!(small.engine, Engine::Sqlite);
    }
}

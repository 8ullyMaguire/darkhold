//! A jury drawn from the eligible pool (spec §14, M66 GS-5).
//!
//! The specification's rule, in full (§14, "the escalation ladder"):
//!
//! > A jury is drawn **randomly** from the eligible pool (level ≥ `governance.jury_min_level`, default 3), size
//! > `governance.jury_size` (default 5), excluding the reporter, the reported, and — from the appeal rule below — anyone
//! > who already sat on the decision being appealed.
//!
//! > **Elasticity applies here too.** If nobody has reached level 3, the jury is drawn from the **top of the actual
//! > ladder** with `governance.jury_elastic_min` (default 3) members.
//!
//! This module is the *rule*, with no database and no clock: the caller hands in the candidates it found and the seed it
//! wants. That split is deliberate, because it is the only way the two properties that matter can be tested without
//! pretending:
//!
//! * **The draw is random but reproducible.** Given the same candidates and the same seed, the panel is the same panel —
//!   so a decision can be audited ("show me the draw that decided this") months later. The seed is a parameter and not
//!   `rand::thread_rng()` for exactly that reason.
//! * **The exclusions are structural.** The reporter and the reported are removed *inside* the draw, not by the caller,
//!   because a caller that forgets is a jury that includes the person who filed the report — and nobody would notice until
//!   the decision looked strange.
//!
//! What this module deliberately does **not** do: read the pool, persist the panel, or decide that a report needs a jury.
//! Those are the store's and the route's halves and they are named as missing in the ledger row.

use std::collections::BTreeSet;

/// The settings a draw needs, by their configuration names (spec §14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JuryRules {
    /// `governance.jury_size`.
    pub size: usize,
    /// `governance.jury_min_level`: the level a candidate must hold to be eligible.
    pub min_level: i64,
    /// `governance.jury_elastic_min`: how many members the elastic fallback must find.
    pub elastic_min: usize,
}

impl Default for JuryRules {
    fn default() -> Self {
        Self {
            size: 5,
            min_level: 3,
            elastic_min: 3,
        }
    }
}

/// One candidate: an account and the level it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Juror {
    /// The account id.
    pub account: String,
    /// The trust level it holds, as the ladder computed it.
    pub level: i64,
}

/// Why a panel could not be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JuryError {
    /// Fewer eligible people than the elastic minimum, even at the top of the ladder.
    ///
    /// **Not an error to work around.** The specification's answer to "there is no pool" is not "decide with whoever is
    /// left": a two-person panel on an instance of twenty is a panel that can be stacked. The caller escalates to an
    /// operator, and this variant is how it says so.
    PoolTooSmall {
        /// The number of candidates that survived the exclusions.
        available: usize,
        /// The minimum the rules require.
        required: usize,
    },
    /// The rules ask for a panel of nobody.
    EmptyPanel,
}

impl std::fmt::Display for JuryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PoolTooSmall {
                available,
                required,
            } => write!(
                formatter,
                "a jury needs at least {required} eligible people and this pool has {available}"
            ),
            Self::EmptyPanel => write!(formatter, "a jury of nobody decides nothing"),
        }
    }
}

impl std::error::Error for JuryError {}

/// Draw a panel (spec §14).
///
/// `candidates` is the pool the store found — **every** active account with its level, including the reporter and the
/// reported, because removing them here is what makes the exclusion structural. `exclude` names the accounts that must not
/// sit: the reporter, the reported, and on an appeal the members of the original panel (`governance.appeal_fresh_jury`).
///
/// The order of the returned accounts is the draw's order, and it is stable for a given `seed`.
pub fn draw(
    candidates: &[Juror],
    exclude: &[&str],
    rules: JuryRules,
    seed: u64,
) -> Result<Vec<String>, JuryError> {
    if rules.size == 0 {
        return Err(JuryError::EmptyPanel);
    }
    let excluded: BTreeSet<&str> = exclude.iter().copied().collect();
    let mut pool: Vec<&Juror> = candidates
        .iter()
        .filter(|candidate| !excluded.contains(candidate.account.as_str()))
        .collect();

    // **The eligible pool first.** Sorted by account id before shuffling, so the shuffle's input order does not depend on
    // the order the database happened to return — two runs of the same draw have to produce the same panel, and that is a
    // property of the input being canonical, not of the shuffle being lucky.
    pool.sort_by(|left, right| left.account.cmp(&right.account));

    let eligible: Vec<&Juror> = pool
        .iter()
        .copied()
        .filter(|candidate| candidate.level >= rules.min_level)
        .collect();

    if eligible.len() >= rules.size {
        return Ok(take(&eligible, rules.size, seed));
    }

    // **The elastic fallback** (§14): nobody reached `min_level`, so draw from the top of the *actual* ladder instead of
    // deciding that the instance has no jury. The fallback is itself floored: below `elastic_min` there is no panel, and
    // saying so is the honest answer.
    let mut ranked = pool.clone();
    ranked.sort_by(|left, right| {
        right
            .level
            .cmp(&left.level)
            .then_with(|| left.account.cmp(&right.account))
    });
    let top_level = ranked.first().map(|candidate| candidate.level).unwrap_or(0);
    let top: Vec<&Juror> = ranked
        .into_iter()
        .filter(|candidate| candidate.level == top_level)
        .collect();
    if top.len() < rules.elastic_min {
        return Err(JuryError::PoolTooSmall {
            available: top.len(),
            required: rules.elastic_min,
        });
    }
    // **The elastic panel is `elastic_min` members, not `jury_size`** (§14 says "drawn from the top of the actual ladder
    // with `governance.jury_elastic_min` (default 3) members"). Five people from a top rung that only has three is either a
    // silent short panel or a panel padded with the next rung down, and the spec chose neither.
    let size = rules.elastic_min.min(top.len());
    Ok(take(&top, size, seed))
}

/// `size` distinct accounts from `pool`, in a deterministic shuffle's order.
fn take(pool: &[&Juror], size: usize, seed: u64) -> Vec<String> {
    let mut indices: Vec<usize> = (0..pool.len()).collect();
    let mut state = seed | 1;
    // Fisher-Yates with xorshift64*: no dependency, and a seed that makes the draw reproducible for an audit.
    for position in (1..indices.len()).rev() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let value = state.wrapping_mul(0x2545_f491_4f6c_dd1d);
        let swap = (value % (position as u64 + 1)) as usize;
        indices.swap(position, swap);
    }
    indices
        .into_iter()
        .take(size)
        .map(|index| pool[index].account.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn juror(account: &str, level: i64) -> Juror {
        Juror {
            account: account.to_owned(),
            level,
        }
    }

    fn pool() -> Vec<Juror> {
        let mut pool = Vec::new();
        for index in 0..12 {
            pool.push(juror(&format!("senior-{index}"), 4));
        }
        for index in 0..6 {
            pool.push(juror(&format!("member-{index}"), 2));
        }
        pool
    }

    #[test]
    fn a_panel_comes_from_the_eligible_ladder() {
        let panel = draw(&pool(), &[], JuryRules::default(), 7).expect("a panel");
        assert_eq!(panel.len(), 5, "{panel:?}");
        assert!(
            panel.iter().all(|account| account.starts_with("senior-")),
            "level 3 is the floor and `member-*` holds 2: {panel:?}"
        );
        let unique: BTreeSet<&String> = panel.iter().collect();
        assert_eq!(
            unique.len(),
            panel.len(),
            "a panel is people, not seats: {panel:?}"
        );
    }

    #[test]
    fn the_same_seed_draws_the_same_panel_and_another_seed_does_not() {
        let first = draw(&pool(), &[], JuryRules::default(), 11).expect("panel");
        let again = draw(&pool(), &[], JuryRules::default(), 11).expect("panel");
        assert_eq!(first, again, "an audited draw has to be reproducible");
        let other = draw(&pool(), &[], JuryRules::default(), 12).expect("panel");
        assert_ne!(first, other, "a seed that changes nothing is not a seed");
    }

    #[test]
    fn the_candidates_order_does_not_change_the_draw() {
        let mut reversed = pool();
        reversed.reverse();
        assert_eq!(
            draw(&pool(), &[], JuryRules::default(), 3).expect("panel"),
            draw(&reversed, &[], JuryRules::default(), 3).expect("panel"),
            "the database's row order is not part of the draw"
        );
    }

    #[test]
    fn the_reporter_and_the_reported_never_sit() {
        let mut candidates = vec![juror("reporter", 6), juror("reported", 6)];
        for index in 0..6 {
            candidates.push(juror(&format!("senior-{index}"), 4));
        }
        for seed in 0..40 {
            let panel = draw(
                &candidates,
                &["reporter", "reported"],
                JuryRules::default(),
                seed,
            )
            .expect("panel");
            assert!(
                !panel.iter().any(|account| account == "reporter"),
                "{panel:?}"
            );
            assert!(
                !panel.iter().any(|account| account == "reported"),
                "{panel:?}"
            );
        }
    }

    #[test]
    fn nobody_at_the_minimum_falls_back_to_the_top_of_the_ladder() {
        // The instance nobody has earned their way up yet: the highest level is 2, and the rules want 3.
        let candidates: Vec<Juror> = (0..7)
            .map(|index| juror(&format!("new-{index}"), 2))
            .collect();
        let panel = draw(&candidates, &[], JuryRules::default(), 5).expect("the elastic panel");
        assert_eq!(panel.len(), 3, "elastic_min, not jury_size: {panel:?}");
    }

    #[test]
    fn a_pool_below_the_elastic_minimum_is_refused_rather_than_padded() {
        let candidates = vec![juror("one", 1), juror("two", 1)];
        let error = draw(&candidates, &[], JuryRules::default(), 1).expect_err("two is not a jury");
        assert_eq!(
            error,
            JuryError::PoolTooSmall {
                available: 2,
                required: 3
            }
        );
    }

    #[test]
    fn an_appeal_can_exclude_the_original_panel() {
        let candidates: Vec<Juror> = (0..10)
            .map(|index| juror(&format!("s-{index}"), 5))
            .collect();
        let original = draw(&candidates, &[], JuryRules::default(), 21).expect("first panel");
        let appeal = draw(
            &candidates,
            &original.iter().map(String::as_str).collect::<Vec<_>>(),
            JuryRules::default(),
            22,
        )
        .expect("fresh panel");
        assert_eq!(appeal.len(), 5);
        assert!(
            appeal.iter().all(|account| !original.contains(account)),
            "a fresh jury is fresh: {appeal:?} against {original:?}"
        );
    }
}

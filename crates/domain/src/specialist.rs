//! M45-33 — specialist matchmaking: betas, Britpickers, canon consultants, sensitivity readers.
//!
//! # Three rules, and the reason each exists
//!
//! * **Opt-in means absence, not a flag.** A wildcard profile would let a reader "volunteer" by never acting.
//!   Everything here starts from a person writing one (`Skills`, `Capacity`), and the store's candidate query
//!   is the only reader of it.
//! * **Matching ranks by fit and then by load.** A specialist who names the work's fandom beats one who
//!   offers "any"; among equals the one carrying the fewest live engagements goes first, so a single
//!   helpful person is not handed ten briefs by a deterministic tie-break nobody thought about.
//! * **Critique only where requested.** The state machine has no path from nothing to `matched`: a request
//!   exists because it was written, and `accept` is the only way a specialist enters it. `withdraw` is
//!   reachable from `matched` as well as `open`, because a person's availability changes.

use std::fmt;

/// What someone offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Skill {
    /// Line-by-line beta reading.
    Beta,
    /// Setting, idiom and vocabulary checks against a real place and period.
    Britpick,
    /// Canon consistency against the source.
    Canon,
    /// Sensitivity reading for a subject the work handles.
    Sensitivity,
}

impl Skill {
    /// The wire word, which is also the column's CHECK constraint.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Beta => "beta",
            Self::Britpick => "britpick",
            Self::Canon => "canon",
            Self::Sensitivity => "sensitivity",
        }
    }

    /// Parse a wire word. `None` for anything else: an unknown skill is a caller error, never a new skill.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "beta" => Some(Self::Beta),
            "britpick" => Some(Self::Britpick),
            "canon" => Some(Self::Canon),
            "sensitivity" => Some(Self::Sensitivity),
            _ => None,
        }
    }

    /// Every skill, in the order the interface lists them.
    #[must_use]
    pub fn all() -> [Self; 4] {
        [Self::Beta, Self::Britpick, Self::Canon, Self::Sensitivity]
    }
}

impl fmt::Display for Skill {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a profile is taking work at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capacity {
    Open,
    Closed,
}

impl Capacity {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

/// Where a request is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Written, nobody has taken it.
    Open,
    /// A specialist accepted it.
    Matched,
    /// The work was returned.
    Completed,
    /// The requester took it back, or the specialist released it.
    Withdrawn,
}

impl State {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Matched => "matched",
            Self::Completed => "completed",
            Self::Withdrawn => "withdrawn",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "matched" => Some(Self::Matched),
            "completed" => Some(Self::Completed),
            "withdrawn" => Some(Self::Withdrawn),
            _ => None,
        }
    }
}

/// What a caller wants to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accept,
    Complete,
    Withdraw,
}

/// Why a transition was refused, in words the caller can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Apply `action` to `state`.
///
/// The transitions are deliberately few: `open → matched → completed`, with `withdraw` available from the two
/// live states. **Nothing returns from `completed` or `withdrawn`** — a finished engagement is history, and a
/// request that could be reopened would let the same work be "completed" twice.
///
/// # Errors
/// A [`Refusal`] naming the current state, when the action does not belong to it.
pub fn transition(state: State, action: Action) -> Result<State, Refusal> {
    let next = match (state, action) {
        (State::Open, Action::Accept) => State::Matched,
        (State::Matched, Action::Complete) => State::Completed,
        (State::Open | State::Matched, Action::Withdraw) => State::Withdrawn,
        (state, action) => {
            return Err(Refusal(format!(
                "cannot {} a request that is {}",
                match action {
                    Action::Accept => "accept",
                    Action::Complete => "complete",
                    Action::Withdraw => "withdraw",
                },
                state.as_str()
            )))
        }
    };
    Ok(next)
}

/// One specialist a request could go to.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub pseud_id: String,
    /// The fandoms they named. Empty means any.
    pub fandoms: Vec<String>,
    /// Engagements they are carrying right now (`matched`), used to spread the load.
    pub live_engagements: i64,
    /// How many they have finished, shown rather than used for ordering.
    pub completed: i64,
}

/// A candidate with the score that placed it.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub candidate: Candidate,
    /// 2 for a named fandom in common, 1 for "any fandom".
    pub fit: i64,
}

/// Rank candidates for a request.
///
/// `fandom` empty means the request named none, which is a request open to anyone: then every candidate fits
/// equally and the load decides. Otherwise a candidate naming that fandom (case-insensitively) outranks one
/// offering an open-ended hand, and both outrank a candidate who named *other* fandoms and no "any" — those
/// are dropped, because offering to beta a fandom you did not name is not an offer for this work.
///
/// Ordering is total and deterministic: fit, then fewest live engagements, then pseud id. A caller can page it.
#[must_use]
pub fn rank(fandom: &str, candidates: Vec<Candidate>) -> Vec<Ranked> {
    let wanted = fandom.trim().to_ascii_lowercase();
    let mut ranked: Vec<Ranked> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let fit = if wanted.is_empty() || candidate.fandoms.is_empty() {
                1
            } else if candidate
                .fandoms
                .iter()
                .any(|named| named.trim().eq_ignore_ascii_case(&wanted))
            {
                2
            } else {
                return None;
            };
            Some(Ranked { candidate, fit })
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.fit
            .cmp(&a.fit)
            .then(
                a.candidate
                    .live_engagements
                    .cmp(&b.candidate.live_engagements),
            )
            .then(a.candidate.pseud_id.cmp(&b.candidate.pseud_id))
    });
    ranked
}

/// The confidence a brief deserves, as a sentence: what the interface prints above the matching list.
#[must_use]
pub fn summary(fandom: &str, ranked: &[Ranked]) -> String {
    if ranked.is_empty() {
        return match fandom.trim().is_empty() {
            true => "nobody is offering this skill right now".to_owned(),
            false => format!(
                "nobody offering this skill is open to {} — the request stays open, and anyone may accept it",
                fandom.trim()
            ),
        };
    }
    let named = ranked.iter().filter(|row| row.fit == 2).count();
    match named {
        0 => format!("{} open to any fandom", plural(ranked.len())),
        n => format!(
            "{n} who named {}, {} open to any fandom",
            fandom.trim(),
            ranked.len() - n
        ),
    }
}

fn plural(n: usize) -> String {
    match n {
        1 => "1 specialist is".to_owned(),
        n => format!("{n} specialists are"),
    }
}

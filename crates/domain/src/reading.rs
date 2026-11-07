//! Reading position, progress resolution and reading-time estimates (spec §9).
//!
//! Spec §9.3 is the rule that shapes this module: "When devices disagree,
//! present a choice rather than always taking the furthest position." That is a
//! *decision about what to show a reader*, so it belongs here, in a pure
//! function, rather than in whichever handler happens to load the rows first —
//! a second handler that took the furthest position silently would be exactly
//! the kind of "subtly different check" the project's history warns about.
//!
//! Everything here is pure: no database, no clock, no I/O. Callers load facts
//! and hand them in.

use crate::ids::RevisionId;

/// A stored position within a subject.
///
/// `fraction` is permille (0..=1000) rather than a fraction, because the crate
/// binds only `String`/`i64` to the database and a `REAL` column would need a
/// second bind path. A thousandths precision is enough for "where was I" and is
/// what the `position_permille` column stores.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReadingPosition {
    /// The revision the reader was looking at, when one was recorded.
    pub revision: Option<RevisionId>,
    /// A stable paragraph anchor, when one was recorded.
    pub anchor: Option<String>,
    /// Position in the content, in permille (0..=1000).
    pub fraction: u16,
    /// A per-device identifier, when one was recorded.
    pub device: Option<String>,
}

impl ReadingPosition {
    /// Whether two positions point at the same revision.
    ///
    /// Anchors are revision-specific: a position that was recorded against an
    /// older revision cannot be honoured against a newer one, even if the
    /// fraction is the same, because the text in between may have changed.
    #[must_use]
    pub fn same_revision(&self, other: &Self) -> bool {
        self.revision.is_some() && self.revision == other.revision
    }
}

/// What to do when a reader has more than one stored position for a subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressResolution {
    /// One position, or several that agree: use it.
    UseStored(ReadingPosition),
    /// Several that disagree: the reader must choose.
    AskTheReader {
        /// The device's own stored position, if any.
        mine: Option<ReadingPosition>,
        /// The other device's position, if any.
        other: Option<ReadingPosition>,
    },
    /// Nothing stored at all.
    NoPosition,
}

/// Resolve a reader's stored positions into a single decision.
///
/// Spec §9.3: one position uses it; several that agree use the (shared)
/// value; several that disagree ask the reader.
pub fn resolve_progress(rows: &[ReadingPosition]) -> ProgressResolution {
    match rows {
        [] => ProgressResolution::NoPosition,
        [single] => ProgressResolution::UseStored(single.clone()),
        other => {
            let first = &other[0];
            if other
                .iter()
                .all(|row| row.same_revision(first) && row.fraction == first.fraction)
            {
                ProgressResolution::UseStored(first.clone())
            } else {
                // The reader's own device is identified by the most recent
                // device id present; if there is none, fall back to the first.
                let mine = other
                    .iter()
                    .find(|row| row.device.is_some())
                    .cloned()
                    .or_else(|| Some(first.clone()));
                let other_position = other
                    .iter()
                    .find(|row| !row.same_revision(first) || row.fraction != first.fraction)
                    .cloned();
                ProgressResolution::AskTheReader {
                    mine,
                    other: other_position,
                }
            }
        }
    }
}

/// Whether a stored position still points at content the reader saw.
///
/// A position recorded against a revision that no longer resolves is not
/// reliable: the anchor or fraction would point at text that has since been
/// rewritten (ADR 0002's append-only revisions make this the common case).
#[must_use]
pub fn position_is_reliable(position: &ReadingPosition, current_revision: RevisionId) -> bool {
    position
        .revision
        .is_some_and(|revision| revision == current_revision)
}

/// An estimate of how long a text takes to read.
///
/// The figure is an *assumption*, not a measurement: 200 words per minute,
/// which is the commonly cited average for silent English prose reading. It is
/// stated here, next to the constant, so it can be found and revisited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadingTime {
    /// Whole minutes; `0` for an empty text.
    pub minutes: u32,
}

/// Words per minute used by [`estimate_reading_time`].
///
/// A named constant rather than a literal, so the assumption lives in one place
/// and is documented as such (spec §3.2 requires documented interpretation for
/// statistical estimates).
pub const WORDS_PER_MINUTE: u32 = 200;

/// Estimate the reading time of a word count.
///
/// Empty text is `0 minutes`; anything else is rounded up to one minute and
/// then rounded to whole minutes above that, so a one-word story is not
/// reported as "instant".
#[must_use]
pub fn estimate_reading_time(word_count: u32) -> ReadingTime {
    if word_count == 0 {
        return ReadingTime { minutes: 0 };
    }
    let minutes = word_count.div_ceil(WORDS_PER_MINUTE);
    ReadingTime {
        minutes: minutes.max(1),
    }
}

// ---------------------------------------------------------------------------
// M71-09: an honest resume state
// ---------------------------------------------------------------------------
//
// `resolve_progress` answers *which* stored position to use. It does not answer the two questions a reader actually has, and that silence is
// the bug: a position recorded against a revision that has since been rewritten is presented as though it were current, and a row with no
// evidence of where the reader was renders as `0%`, which is a fabricated measurement (§11.3 forbids fake precision; §8.14 forbids silence
// about a gap).
//
// So the states are enumerated, each with its own sentence, and the percentage is an `Option` — `None` is rendered `—`, never `0%`.

/// A stored position together with the revision its unit currently points at.
///
/// The current revision travels beside the position rather than being looked up by the caller, because comparing the two is the whole of
/// the staleness question and a caller that forgot would produce exactly the silent lie this type exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPosition {
    pub position: ReadingPosition,
    /// The revision the position's unit points at *now*, when the unit still exists.
    pub current_revision: Option<RevisionId>,
}

/// What a reader can be offered when they come back to a subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeState {
    /// Nothing stored: there is nothing to resume and nothing to explain.
    Nothing,
    /// A position that was recorded without a revision, an anchor, or any movement: the reader was at the top, or nothing was saved.
    Unmeasured,
    /// A position against the text as it stands.
    Current {
        fraction: u16,
        anchor: Option<String>,
    },
    /// A position against a revision that is no longer the current one. Offered as a choice, never rendered as a percentage: the
    /// permille was measured against text that has changed.
    Stale {
        recorded: RevisionId,
        /// The revision the unit points at now; `None` when the unit itself is gone, which is also why the place is not a percentage.
        current: Option<RevisionId>,
        fraction: u16,
        anchor: Option<String>,
    },
    /// Positions from more than one device that disagree. The reader chooses; the page shows both, and no single percentage.
    Conflict {
        mine: ReadingPosition,
        other: ReadingPosition,
    },
}

impl ResumeState {
    /// The percentage to render, or `None` when there is no honest number to render.
    ///
    /// Only [`ResumeState::Current`] has one. A stale position was measured against different text; an unmeasured one was never measured;
    /// a conflict is two numbers and no basis to pick. `None` is rendered `—`.
    #[must_use]
    pub fn percent(&self) -> Option<u16> {
        match self {
            ResumeState::Current { fraction, .. } => Some(*fraction),
            _ => None,
        }
    }

    /// The string a page prints where a percentage would go.
    #[must_use]
    pub fn display(&self) -> String {
        match self.percent() {
            Some(per_mille) => format!("{:.0}%", f64::from(per_mille) / 10.0),
            None => "—".to_owned(),
        }
    }

    /// The sentence that explains the state, in the reader's terms.
    #[must_use]
    pub fn sentence(&self) -> &'static str {
        match self {
            ResumeState::Nothing => "You have not started this yet.",
            ResumeState::Unmeasured => "There is no saved place in this text.",
            ResumeState::Current { .. } => "You can pick up where you left off.",
            ResumeState::Stale { .. } => {
                "Your place was saved on an earlier version of this text. You can go back to it, or start from the top."
            }
            ResumeState::Conflict { .. } => {
                "Two devices have different places in this text. Choose the one to continue from."
            }
        }
    }

    /// Whether the page must offer a choice rather than resume silently (§9.3).
    #[must_use]
    pub fn needs_a_choice(&self) -> bool {
        matches!(
            self,
            ResumeState::Stale { .. } | ResumeState::Conflict { .. }
        )
    }
}

/// Positions closer than this agree: a slider dropped a pixel to the left is not a second opinion.
pub const AGREEMENT_PERMILLE: u16 = 50;

/// Decide what to offer a reader, from their stored positions and the revisions those units point at now.
pub fn resume_state(rows: &[StoredPosition]) -> ResumeState {
    let Some(first) = rows.first() else {
        return ResumeState::Nothing;
    };

    // Disagreement first: if two devices say different things, which one is "current" is the reader's decision and not ours.
    if let Some(other) = rows.iter().skip(1).find(|row| {
        row.position.revision != first.position.revision
            || row.position.fraction.abs_diff(first.position.fraction) > AGREEMENT_PERMILLE
    }) {
        return ResumeState::Conflict {
            mine: first.position.clone(),
            other: other.position.clone(),
        };
    }

    let position = &first.position;
    let has_evidence =
        position.revision.is_some() || position.anchor.is_some() || position.fraction > 0;
    if !has_evidence {
        return ResumeState::Unmeasured;
    }

    match (&position.revision, &first.current_revision) {
        (Some(recorded), Some(current)) if recorded != current => ResumeState::Stale {
            recorded: *recorded,
            current: Some(*current),
            fraction: position.fraction,
            anchor: position.anchor.clone(),
        },
        // A revision was recorded but the unit it belonged to is gone. The permille was not measured against text as it stands, so it is
        // not a percentage; the place is still offered, with `current: None` saying why.
        (Some(recorded), None) => ResumeState::Stale {
            recorded: *recorded,
            current: None,
            fraction: position.fraction,
            anchor: position.anchor.clone(),
        },
        _ => ResumeState::Current {
            fraction: position.fraction,
            anchor: position.anchor.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(revision: &str, fraction: u16) -> ReadingPosition {
        ReadingPosition {
            revision: Some(revision.parse().unwrap()),
            anchor: None,
            fraction,
            device: Some("device-a".to_owned()),
        }
    }

    #[test]
    fn a_single_position_is_used_directly() {
        let only = position("00000000-0000-0000-0000-000000000011", 500);
        assert_eq!(
            resolve_progress(std::slice::from_ref(&only)),
            ProgressResolution::UseStored(only)
        );
    }

    #[test]
    fn several_agreeing_positions_are_used_directly() {
        let a = position("00000000-0000-0000-0000-000000000011", 500);
        let b = ReadingPosition {
            revision: a.revision,
            anchor: None,
            fraction: 500,
            device: Some("device-b".to_owned()),
        };
        assert_eq!(
            resolve_progress(&[a.clone(), b]),
            ProgressResolution::UseStored(a)
        );
    }

    #[test]
    fn disagreeing_positions_ask_the_reader() {
        let mine = position("00000000-0000-0000-0000-000000000011", 100);
        let theirs = ReadingPosition {
            revision: mine.revision,
            anchor: None,
            fraction: 900,
            device: Some("device-b".to_owned()),
        };
        assert_eq!(
            resolve_progress(&[mine.clone(), theirs.clone()]),
            ProgressResolution::AskTheReader {
                mine: Some(mine),
                other: Some(theirs),
            }
        );
    }

    #[test]
    fn positions_on_different_revisions_disagree() {
        let mine = position("00000000-0000-0000-0000-000000000011", 500);
        let theirs = position("00000000-0000-0000-0000-000000000022", 500);
        assert_eq!(
            resolve_progress(&[mine.clone(), theirs.clone()]),
            ProgressResolution::AskTheReader {
                mine: Some(mine),
                other: Some(theirs),
            }
        );
    }

    #[test]
    fn no_positions_is_a_choice_with_no_options() {
        assert_eq!(resolve_progress(&[]), ProgressResolution::NoPosition);
    }

    #[test]
    fn a_stale_position_is_not_reliable() {
        let current: RevisionId = "00000000-0000-0000-0000-000000000022".parse().unwrap();
        let position = ReadingPosition {
            revision: Some("00000000-0000-0000-0000-000000000011".parse().unwrap()),
            anchor: None,
            fraction: 500,
            device: None,
        };
        assert!(!position_is_reliable(&position, current));

        let current_position = ReadingPosition {
            revision: Some(current),
            anchor: None,
            fraction: 500,
            device: None,
        };
        assert!(position_is_reliable(&current_position, current));
    }

    #[test]
    fn reading_time_rounds_up_to_one_minute_and_then_to_whole_minutes() {
        assert_eq!(estimate_reading_time(0).minutes, 0);
        assert_eq!(estimate_reading_time(1).minutes, 1);
        assert_eq!(estimate_reading_time(199).minutes, 1);
        assert_eq!(estimate_reading_time(200).minutes, 1);
        assert_eq!(estimate_reading_time(1000).minutes, 5);
    }

    #[test]
    fn reading_time_uses_the_documented_words_per_minute() {
        assert_eq!(WORDS_PER_MINUTE, 200);
    }
    // --- M71-09: the resume states, one test per state, each demanding its sentence and refusing a fabricated percentage ---

    fn stored(revision: Option<&str>, fraction: u16, current: Option<&str>) -> StoredPosition {
        StoredPosition {
            position: ReadingPosition {
                revision: revision.map(|value| value.parse().unwrap()),
                anchor: None,
                fraction,
                device: Some("device-a".to_owned()),
            },
            current_revision: current.map(|value| value.parse().unwrap()),
        }
    }

    #[test]
    fn a_position_on_the_current_revision_is_the_one_percentage_that_is_offered() {
        let state = resume_state(&[stored(
            Some("00000000-0000-0000-0000-000000000011"),
            420,
            Some("00000000-0000-0000-0000-000000000011"),
        )]);
        assert_eq!(
            state,
            ResumeState::Current {
                fraction: 420,
                anchor: None
            }
        );
        assert_eq!(state.percent(), Some(420));
        assert_eq!(state.display(), "42%");
        assert!(!state.needs_a_choice(), "nothing to decide");
    }

    #[test]
    fn a_position_against_an_older_revision_is_offered_as_a_choice_and_never_as_a_percentage() {
        let state = resume_state(&[stored(
            Some("00000000-0000-0000-0000-000000000011"),
            420,
            Some("00000000-0000-0000-0000-000000000022"),
        )]);
        assert!(matches!(state, ResumeState::Stale { .. }), "{state:?}");
        assert_eq!(
            state.percent(),
            None,
            "the permille was measured against text that has changed"
        );
        assert_eq!(state.display(), "—", "no fabricated 42%");
        assert!(state.needs_a_choice());
        assert!(
            state.sentence().contains("earlier version")
                && state.sentence().contains("start from the top"),
            "the choice is stated: {}",
            state.sentence()
        );
    }

    #[test]
    fn a_position_with_no_evidence_renders_a_dash_rather_than_zero_percent() {
        let state = resume_state(&[stored(None, 0, None)]);
        assert_eq!(state, ResumeState::Unmeasured);
        assert_eq!(state.percent(), None);
        assert_eq!(state.display(), "—");
        assert_eq!(state.sentence(), "There is no saved place in this text.");

        // No rows at all is a different fact, and says so.
        let nothing = resume_state(&[]);
        assert_eq!(nothing, ResumeState::Nothing);
        assert_eq!(nothing.display(), "—");
        assert_eq!(nothing.sentence(), "You have not started this yet.");
    }

    #[test]
    fn two_devices_that_disagree_ask_the_reader_and_show_no_single_percentage() {
        let mine = stored(
            Some("00000000-0000-0000-0000-000000000011"),
            100,
            Some("00000000-0000-0000-0000-000000000011"),
        );
        let theirs = StoredPosition {
            position: ReadingPosition {
                revision: mine.position.revision,
                anchor: None,
                fraction: 900,
                device: Some("device-b".to_owned()),
            },
            current_revision: mine.current_revision,
        };
        let state = resume_state(&[mine, theirs]);
        assert!(matches!(state, ResumeState::Conflict { .. }), "{state:?}");
        assert_eq!(state.percent(), None, "two places, no basis to pick one");
        assert_eq!(state.display(), "—");
        assert!(state.needs_a_choice());
        assert!(state.sentence().contains("Two devices"));

        // A slider dropped a few permille to the left is not a second opinion.
        let close = resume_state(&[
            stored(
                Some("00000000-0000-0000-0000-000000000011"),
                500,
                Some("00000000-0000-0000-0000-000000000011"),
            ),
            StoredPosition {
                position: ReadingPosition {
                    revision: Some("00000000-0000-0000-0000-000000000011".parse().unwrap()),
                    anchor: None,
                    fraction: 520,
                    device: Some("device-b".to_owned()),
                },
                current_revision: Some("00000000-0000-0000-0000-000000000011".parse().unwrap()),
            },
        ]);
        assert!(matches!(close, ResumeState::Current { .. }), "{close:?}");
    }
}

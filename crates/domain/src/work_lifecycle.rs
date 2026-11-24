//! Automatic WIP/abandoned lifecycle (M45-76).
//!
//! The decision is pure: the store supplies the clocks and the current state,
//! this module says what should happen. Keeping it here means the boundary
//! cases (exactly four months, an update after the mark, a source-complete
//! work) are tested without a database.

use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Duration, OffsetDateTime, PrimitiveDateTime};

/// The default number of months without an update before abandonment.
pub const DEFAULT_ABANDON_AFTER_MONTHS: u32 = 4;

/// Who last moved `works.completion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    Import,
    Owner,
    Review,
    Sweep,
    Update,
}

impl CompletionSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Owner => "owner",
            Self::Review => "review",
            Self::Sweep => "sweep",
            Self::Update => "update",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "import" => Some(Self::Import),
            "owner" => Some(Self::Owner),
            "review" => Some(Self::Review),
            "sweep" => Some(Self::Sweep),
            "update" => Some(Self::Update),
            _ => None,
        }
    }
}

/// What the sweep should do with one work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    /// Nothing to do, or the work is already in the target state.
    None,
    /// There is no clock to judge; the work is reported, never marked.
    Unmeasured,
    /// `in_progress`/`hiatus` -> `abandoned`.
    Abandon,
    /// `abandoned` -> `in_progress`, because a story update followed the mark.
    Revive,
    /// A linked source says complete; the state follows the source.
    Conclude,
}

/// The facts the decision needs. `last_activity` is the maximum of the author
/// clocks, never `works.updated_at`.
#[derive(Debug, Clone, Copy)]
pub struct Reading<'a> {
    pub completion: &'a str,
    pub source_says_complete: bool,
    pub completion_marked_at: Option<OffsetDateTime>,
    pub last_activity: Option<OffsetDateTime>,
}

#[must_use]
pub fn decide(
    reading: Reading<'_>,
    now: OffsetDateTime,
    abandon_after_months: u32,
) -> LifecycleAction {
    // A source that says "complete" outranks both the clock and the old state.
    if reading.source_says_complete {
        return if reading.completion == "complete" {
            LifecycleAction::None
        } else {
            LifecycleAction::Conclude
        };
    }

    match reading.completion {
        // The author's completed work is never swept, however old the last
        // chapter is.
        "complete" => LifecycleAction::None,
        "abandoned" => match (reading.last_activity, reading.completion_marked_at) {
            // Only an update AFTER the mark revives. An update before it is
            // the reason it was marked abandoned in the first place.
            (Some(last), Some(marked)) if last > marked => LifecycleAction::Revive,
            _ => LifecycleAction::None,
        },
        "in_progress" | "hiatus" => match reading.last_activity {
            None => LifecycleAction::Unmeasured,
            Some(last) if is_stale(now, last, abandon_after_months) => LifecycleAction::Abandon,
            Some(_) => LifecycleAction::None,
        },
        // A value this build does not know is left alone; the vocabulary is
        // closed by the CHECK constraints, not guessed at here.
        _ => LifecycleAction::None,
    }
}

/// `X` months is `X * 30` days, the same arithmetic `updated:<=-120d` uses.
#[must_use]
pub fn is_stale(now: OffsetDateTime, last: OffsetDateTime, months: u32) -> bool {
    last + Duration::days(i64::from(months) * 30) <= now
}

/// Parse every shape the three clocks store.
///
/// `source_updated_at` is RFC 3339 with `Z`; `occurred_at` is RFC 3339 with
/// nanoseconds; `published_at` is naive and means UTC. A parser that accepted
/// only RFC 3339 would read the imported works with a `published_at` as
/// "never updated" and abandon them all on the first sweep.
#[must_use]
pub fn parse_activity_timestamp(raw: &str) -> Option<OffsetDateTime> {
    let raw = raw.trim();
    if let Ok(parsed) = OffsetDateTime::parse(raw, &Rfc3339) {
        return Some(parsed);
    }
    PrimitiveDateTime::parse(
        raw,
        &format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"),
    )
    .ok()
    .map(PrimitiveDateTime::assume_utc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(raw: &str) -> OffsetDateTime {
        parse_activity_timestamp(raw).expect("fixture timestamp")
    }

    fn reading<'a>(
        completion: &'a str,
        source_says_complete: bool,
        marked: Option<OffsetDateTime>,
        last: Option<OffsetDateTime>,
    ) -> Reading<'a> {
        Reading {
            completion,
            source_says_complete,
            completion_marked_at: marked,
            last_activity: last,
        }
    }

    #[test]
    fn four_months_is_the_boundary_and_a_day_short_is_not() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(
                reading("in_progress", false, None, Some(at("2026-07-27T00:00:00Z")),),
                now,
                4
            ),
            LifecycleAction::Abandon,
            "120 days is stale"
        );
        assert_eq!(
            decide(
                reading("in_progress", false, None, Some(at("2026-07-28T00:00:00Z")),),
                now,
                4
            ),
            LifecycleAction::None,
            "119 days is not"
        );
    }

    #[test]
    fn hiatus_is_swept_because_the_owner_chose_only_complete_as_the_exemption() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(
                reading("hiatus", false, None, Some(at("2026-01-01T00:00:00Z"))),
                now,
                4
            ),
            LifecycleAction::Abandon
        );
    }

    #[test]
    fn an_update_after_the_mark_revives_and_one_before_it_does_not() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(
                reading(
                    "abandoned",
                    false,
                    Some(at("2026-06-01T00:00:00Z")),
                    Some(at("2026-10-01T00:00:00Z")),
                ),
                now,
                4
            ),
            LifecycleAction::Revive
        );
        assert_eq!(
            decide(
                reading(
                    "abandoned",
                    false,
                    Some(at("2026-06-01T00:00:00Z")),
                    Some(at("2026-05-01T00:00:00Z")),
                ),
                now,
                4
            ),
            LifecycleAction::None
        );
    }

    #[test]
    fn a_source_that_says_complete_concludes_instead_of_abandoning() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(
                reading(
                    "abandoned",
                    true,
                    Some(at("2026-06-01T00:00:00Z")),
                    Some(at("2026-01-01T00:00:00Z")),
                ),
                now,
                4
            ),
            LifecycleAction::Conclude
        );
        assert_eq!(
            decide(
                reading("in_progress", true, None, Some(at("2026-01-01T00:00:00Z")),),
                now,
                4
            ),
            LifecycleAction::Conclude
        );
    }

    #[test]
    fn no_clock_is_unmeasured_not_abandoned() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(reading("in_progress", false, None, None), now, 4),
            LifecycleAction::Unmeasured
        );
    }

    #[test]
    fn all_three_stored_shapes_parse() {
        assert_eq!(
            parse_activity_timestamp("2026-08-01T00:00:05").unwrap(),
            at("2026-08-01T00:00:05Z")
        );
        assert_eq!(
            parse_activity_timestamp("2001-08-24T00:00:00Z").unwrap(),
            at("2001-08-24T00:00:00Z")
        );
        assert_eq!(
            parse_activity_timestamp("2026-09-20T08:42:00.771205161Z").unwrap(),
            at("2026-09-20T08:42:00.771205161Z")
        );
    }
}

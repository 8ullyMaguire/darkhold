//! §9.7.9 — reading goals: a private target a reader sets for themselves.
//!
//! The rules that shape the feature live here rather than in the route, because they are the part
//! that has to be true everywhere:
//!
//! - **A goal is opt-in by existing.** No row means no goal; there is no "created but disabled".
//! - **The period is computed, never stored.** A stored window needs a sweep to roll it over and
//!   still resets at the wrong hour for most readers; [`window`] derives it from a timestamp.
//! - **Progress is reported as a count and a target, never as a verdict.** There is no "missed" or
//!   "failed" in this module's vocabulary — §9.7.1 forbids loss-framed language and the cheapest way
//!   to keep a promise like that is to have no word for breaking it.
//! - **Nothing here pays.** A goal grants no credits, and §9.7.2's daily reading credit is
//!   unaffected by whether one exists.

/// How often the target resets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    /// The reader's UTC day.
    Daily,
    /// The ISO week, Monday 00:00 UTC.
    Weekly,
}

/// What the target counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Distinct chapters with reading progress in the window.
    Chapters,
    /// Words in those chapters, from each chapter's current revision.
    Words,
    /// Works marked finished in the window.
    Works,
}

/// The largest target the API accepts.
///
/// Not a product judgement about ambition: a target beyond this is a typo (a stray digit) and a ring
/// drawn to it is a ring that will never move. `1..=100_000` covers "a chapter a day" and
/// "a novel a week" with room to spare.
pub const MAX_TARGET: i64 = 100_000;

/// A goal as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goal {
    /// How often the target resets.
    pub period: Period,
    /// What the target counts.
    pub metric: Metric,
    /// The target itself, `1..=MAX_TARGET`.
    pub target: i64,
}

impl Period {
    /// The wire value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
        }
    }

    /// Parse the wire value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            _ => None,
        }
    }
}

impl Metric {
    /// The wire value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chapters => "chapters",
            Self::Words => "words",
            Self::Works => "works",
        }
    }

    /// Parse the wire value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "chapters" => Some(Self::Chapters),
            "words" => Some(Self::Words),
            "works" => Some(Self::Works),
            _ => None,
        }
    }

    /// Whether this metric counts the pseud's own reading or the account's shelf.
    ///
    /// `Works` is account-scoped because `reading_status` is (§9.6 keeps one shelf per account). The
    /// payload says so: a per-pseud goal whose third metric silently read another pseud's shelf
    /// would be a privacy bug with a tidy interface.
    #[must_use]
    pub fn scope(self) -> &'static str {
        match self {
            Self::Chapters | Self::Words => "pseud",
            Self::Works => "account",
        }
    }
}

impl Goal {
    /// Validate a requested goal, returning the field error to report.
    ///
    /// The message names the accepted values for the same reason the quick-reaction refusal does:
    /// "invalid period" leaves the caller guessing at a vocabulary it cannot see.
    pub fn check(period: &str, metric: &str, target: i64) -> Result<Self, (&'static str, String)> {
        let period = Period::parse(period).ok_or_else(|| {
            (
                "period",
                format!("Not one of the accepted periods: {}.", accepted_periods()),
            )
        })?;
        let metric = Metric::parse(metric).ok_or_else(|| {
            (
                "metric",
                format!("Not one of the accepted metrics: {}.", accepted_metrics()),
            )
        })?;
        if target <= 0 || target > MAX_TARGET {
            return Err((
                "target",
                format!("A target must be between 1 and {MAX_TARGET}."),
            ));
        }
        Ok(Self {
            period,
            metric,
            target,
        })
    }
}

/// The accepted periods, for a refusal that can be acted on.
#[must_use]
pub fn accepted_periods() -> String {
    "daily, weekly".to_owned()
}

/// The accepted metrics, for the same reason.
#[must_use]
pub fn accepted_metrics() -> String {
    "chapters, words, works".to_owned()
}

/// The half-open window `[start, end)` a goal's progress is counted over.
///
/// UTC, deliberately: the schema carries no per-pseud timezone, and adding one here would put a
/// setting in the reader's preferences that only this feature reads. The boundaries go back with the
/// payload so a client can label them instead of guessing.
#[must_use]
pub fn window(period: Period, now: time::OffsetDateTime) -> (time::OffsetDateTime, time::OffsetDateTime) {
    let now = now.to_offset(time::UtcOffset::UTC);
    // Infallible: replacing the time of day cannot move the offset, and the caller is already in
    // UTC, so there is no daylight-saving gap to land in.
    let day_start = now.replace_time(time::Time::MIDNIGHT);
    match period {
        Period::Daily => (day_start, day_start + time::Duration::days(1)),
        Period::Weekly => {
            // `number_days_from_monday` is the whole reason this is an ISO week: a "week" that
            // starts on Sunday is a different week for most of the world and would shift the
            // reader's own progress boundaries under them.
            let since_monday =
                time::Duration::days(i64::from(now.date().weekday().number_days_from_monday()));
            let start = day_start - since_monday;
            (start, start + time::Duration::days(7))
        }
    }
}

/// A goal with its progress in the current window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// The goal being measured.
    pub goal: Goal,
    /// The count reached in this window.
    pub current: i64,
    /// When the window opened, RFC 3339.
    pub window_start: String,
    /// When it closes, RFC 3339. Exclusive.
    pub window_end: String,
}

impl Progress {
    /// Whether the target has been met.
    #[must_use]
    pub fn met(&self) -> bool {
        self.current >= self.goal.target
    }

    /// How much is left, floored at zero.
    ///
    /// A count past the target reports zero left rather than a negative number: the ring is drawn
    /// from this, and "-3 to go" is the loss framing §9.7.1 forbids arriving through arithmetic.
    #[must_use]
    pub fn remaining(&self) -> i64 {
        (self.goal.target - self.current).max(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn a_daily_window_is_the_utc_day() {
        let now = datetime!(2026-11-02 13:45:00 UTC);
        let (start, end) = window(Period::Daily, now);
        assert_eq!(start, datetime!(2026-11-02 00:00:00 UTC));
        assert_eq!(end, datetime!(2026-11-03 00:00:00 UTC));
    }

    #[test]
    fn a_weekly_window_starts_on_monday() {
        // 2026-11-02 is a Monday; the window must contain it whole.
        let (start, end) = window(Period::Weekly, datetime!(2026-11-02 13:45:00 UTC));
        assert_eq!(start, datetime!(2026-11-02 00:00:00 UTC));
        assert_eq!(end, datetime!(2026-11-09 00:00:00 UTC));

        // A Sunday belongs to the week that began six days earlier, not to the one it precedes.
        let (start, end) = window(Period::Weekly, datetime!(2026-11-08 23:59:59 UTC));
        assert_eq!(start, datetime!(2026-11-02 00:00:00 UTC));
        assert_eq!(end, datetime!(2026-11-09 00:00:00 UTC));
    }

    #[test]
    fn a_window_is_half_open_so_midnight_belongs_to_the_new_day() {
        let (start, end) = window(Period::Daily, datetime!(2026-11-02 00:00:00 UTC));
        assert_eq!(start, datetime!(2026-11-02 00:00:00 UTC));
        assert_eq!(end, datetime!(2026-11-03 00:00:00 UTC));
    }

    #[test]
    fn a_target_must_be_positive_and_sane() {
        assert!(Goal::check("daily", "chapters", 1).is_ok());
        assert!(Goal::check("weekly", "words", MAX_TARGET).is_ok());

        let (field, message) = Goal::check("daily", "chapters", 0).expect_err("zero");
        assert_eq!(field, "target");
        assert!(message.contains("between 1"));

        let (field, _) = Goal::check("daily", "chapters", MAX_TARGET + 1).expect_err("too big");
        assert_eq!(field, "target");
    }

    #[test]
    fn an_unknown_period_or_metric_names_the_accepted_ones() {
        let (field, message) = Goal::check("monthly", "chapters", 10).expect_err("period");
        assert_eq!(field, "period");
        assert!(message.contains("daily") && message.contains("weekly"));

        let (field, message) = Goal::check("daily", "pages", 10).expect_err("metric");
        assert_eq!(field, "metric");
        assert!(message.contains("chapters") && message.contains("words"));
    }

    #[test]
    fn progress_never_reports_a_negative_remainder() {
        let progress = Progress {
            goal: Goal {
                period: Period::Daily,
                metric: Metric::Chapters,
                target: 2,
            },
            current: 5,
            window_start: String::new(),
            window_end: String::new(),
        };
        assert!(progress.met());
        assert_eq!(progress.remaining(), 0, "no '-3 to go'");
    }

    #[test]
    fn the_works_metric_says_it_is_account_scoped() {
        assert_eq!(Metric::Chapters.scope(), "pseud");
        assert_eq!(Metric::Words.scope(), "pseud");
        assert_eq!(Metric::Works.scope(), "account");
    }
}

//! M45-50 — the window a scheduled taste vector applies for.
//!
//! A season is a **half-open interval**: `starts_at <= at < ends_at`. The exclusivity is the point.
//! Two adjacent windows — a Christmas event on the 25th, a New Year one from the 26th — must not both
//! apply for an instant, and an event must stop without anybody editing a row at midnight.
//!
//! Timestamps are RFC 3339 with an offset. A local time without one is refused rather than assumed
//! UTC: the operator writing `2026-12-25T00:00:00` means local midnight, and guessing which zone is
//! how an event starts three hours early.

/// A window, as stored and as checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub starts_at: String,
    pub ends_at: String,
}

/// Parse and validate a window before anything is written.
///
/// Refused, each with its own sentence:
///
/// - a timestamp that is not RFC 3339 with an offset (a local time is ambiguous);
/// - `ends_at <= starts_at`, because a window that ends before it starts would only ever be
///   discovered by never applying.
pub fn parse_window(starts_at: &str, ends_at: &str) -> Result<Window, String> {
    let start = parse_instant(starts_at)?;
    let end = parse_instant(ends_at)?;
    if end <= start {
        return Err(format!(
            "the window ends at {ends_at}, which is not after it starts at {starts_at}: a schedule \
             that ends before it begins would be discovered by never applying"
        ));
    }
    Ok(Window {
        starts_at: starts_at.to_owned(),
        ends_at: ends_at.to_owned(),
    })
}

impl Window {
    /// Whether `at` falls inside: start inclusive, end exclusive.
    #[must_use]
    pub fn covers(&self, at: &str) -> bool {
        match (parse_instant(&self.starts_at), parse_instant(&self.ends_at), parse_instant(at)) {
            (Ok(start), Ok(end), Ok(at)) => start <= at && at < end,
            // An unparseable `at` covers nothing: a clock we cannot read is not a reason to apply a
            // seasonal vector to an instance.
            _ => false,
        }
    }
}

/// `true` when `at` is RFC 3339 with an offset, and the value.
fn parse_instant(raw: &str) -> Result<time::OffsetDateTime, String> {
    time::OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339).map_err(|error| {
        format!("{raw}: not an RFC 3339 timestamp with an offset ({error})")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Window {
        parse_window("2026-12-24T00:00:00Z", "2026-12-26T00:00:00Z").expect("valid window")
    }

    #[test]
    fn the_start_is_inside_and_the_end_is_not() {
        let window = window();
        assert!(window.covers("2026-12-24T00:00:00Z"), "the start is included");
        assert!(window.covers("2026-12-25T12:00:00Z"));
        assert!(
            !window.covers("2026-12-26T00:00:00Z"),
            "the end is the instant the gravity stops, not one more instant of it"
        );
        assert!(!window.covers("2026-12-23T23:59:59Z"));
    }

    #[test]
    fn two_adjacent_windows_never_both_apply() {
        let christmas = parse_window("2026-12-24T00:00:00Z", "2026-12-26T00:00:00Z").expect("valid");
        let new_year = parse_window("2026-12-26T00:00:00Z", "2027-01-02T00:00:00Z").expect("valid");
        for at in [
            "2026-12-25T23:59:59Z",
            "2026-12-26T00:00:00Z",
            "2026-12-31T00:00:00Z",
        ] {
            assert!(
                !(christmas.covers(at) && new_year.covers(at)),
                "both windows applied at {at}"
            );
        }
        assert!(new_year.covers("2026-12-26T00:00:00Z"), "the handover instant belongs to the second");
    }

    #[test]
    fn an_offset_is_required_because_a_bare_local_time_is_ambiguous() {
        let failure = parse_window("2026-12-24T00:00:00", "2026-12-26T00:00:00Z").expect_err("refused");
        assert!(failure.contains("RFC 3339"), "{failure}");

        // The same instant spelled with an offset is fine, and compares as an instant.
        let local = parse_window("2026-12-24T01:00:00+01:00", "2026-12-26T00:00:00Z").expect("valid");
        assert!(local.covers("2026-12-24T00:00:00Z"), "01:00+01:00 is 00:00Z");
    }

    #[test]
    fn a_window_that_ends_before_it_starts_is_refused_with_the_reason() {
        let failure = parse_window("2026-12-26T00:00:00Z", "2026-12-24T00:00:00Z").expect_err("refused");
        assert!(failure.contains("not after it starts"), "{failure}");
        // And an empty window is the same defect, not a separate one.
        assert!(parse_window("2026-12-24T00:00:00Z", "2026-12-24T00:00:00Z").is_err());
    }

    #[test]
    fn an_unreadable_instant_covers_nothing_rather_than_everything() {
        let window = window();
        assert!(!window.covers("not a timestamp"));
        assert!(!window.covers(""));
    }
}

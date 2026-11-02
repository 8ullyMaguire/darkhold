//! §11.11a — succession: validation, normalisation and staleness (M45-39).
//!
//! Spec: `docs/spec.md` §11.11a. Plan: `docs/plans/m45-39-succession.md`.
//!
//! Four rules, each of which is a refusal somewhere:
//!
//! - **An export is required.** A destination with no export URL is a forwarding address where the
//!   destination holds the only copy of the archive.
//! - **Staleness is computed.** `confirmed_at + review_days` against a supplied "now", so the answer
//!   cannot go stale in a column and the function is testable with a fixed clock.
//! - **A host is normalised before it is stored.** `HTTPS://Example.org:443` and `example.org` are the
//!   same host, and a registry that thinks otherwise has two rows for one place.
//! - **A path is a prefix with a leading slash.** `works/1` and `/works/1` must not be two entries.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::error::AppError;

/// The window bounds for re-confirmation, in days.
pub const MIN_REVIEW_DAYS: i64 = 30;
/// The window bounds for re-confirmation, in days.
pub const MAX_REVIEW_DAYS: i64 = 365;
/// What an operator gets when they do not choose.
pub const DEFAULT_REVIEW_DAYS: i64 = 90;

/// A succession plan as an operator sets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDraft {
    /// Where readers should go.
    pub destination: String,
    /// Where a full export can be fetched.
    pub export_url: String,
    /// How to reach the operator: `mailto:` or `https:`.
    pub contact: String,
    /// How often the plan must be re-confirmed.
    pub review_days: i64,
}

/// A stored plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Where readers should go.
    pub destination: String,
    /// Where a full export can be fetched.
    pub export_url: String,
    /// How to reach the operator.
    pub contact: String,
    /// How often the plan must be re-confirmed.
    pub review_days: i64,
    /// When it was last confirmed, RFC 3339.
    pub confirmed_at: String,
}

impl Plan {
    /// Whether the plan has outlived its review window at `now`.
    ///
    /// Computed, never stored: a stored flag needs something to flip it, and the case this exists
    /// for is the case where nothing is running.
    #[must_use]
    pub fn is_stale(&self, now: OffsetDateTime) -> bool {
        match parse_time(&self.confirmed_at) {
            Some(confirmed) => now - confirmed > time::Duration::days(self.review_days),
            // An unreadable timestamp is not evidence of liveness. Treat it as stale: the document
            // then reports itself as unverified rather than as fresh because the parse failed.
            None => true,
        }
    }

    /// The instant the plan stops being current, RFC 3339.
    #[must_use]
    pub fn review_due(&self) -> Option<String> {
        parse_time(&self.confirmed_at).and_then(|confirmed| {
            (confirmed + time::Duration::days(self.review_days))
                .format(&Rfc3339)
                .ok()
        })
    }
}

/// Parse an RFC 3339 timestamp, tolerating a naive one.
///
/// Timestamps in this tree are written by `now_rfc3339`, which always carries an offset; the naive
/// arm exists so a row written by hand does not read as "unreadable" when it is merely unqualified.
#[must_use]
pub fn parse_time(value: &str) -> Option<OffsetDateTime> {
    if let Ok(parsed) = OffsetDateTime::parse(value, &Rfc3339) {
        return Some(parsed);
    }
    // `time`'s Rfc3339 parser insists on an offset. A hand-written row without one means UTC; reading
    // it as unreadable would report a merely unqualified timestamp as "stale because it would not
    // parse", which is the wrong reason.
    let naive = time::PrimitiveDateTime::parse(
        value,
        &time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"),
    )
    .ok()?;
    Some(naive.assume_utc())
}

/// Check a draft and return it with its review window clamped to the allowed bounds.
///
/// Bounds rather than a refusal on the window: an operator who asks for ten days is optimising for
/// something real, and rounding them to the floor of thirty is the honest answer. Everything else is
/// a refusal, because a plan with no export or a non-https destination is a promise nobody can use.
pub fn validate(draft: PlanDraft) -> Result<PlanDraft, AppError> {
    let destination = draft.destination.trim();
    let export_url = draft.export_url.trim();
    let contact = draft.contact.trim();

    if !is_https(destination) {
        return Err(AppError::field(
            "destination",
            "A destination must be an https URL: readers will follow it off this instance.",
        ));
    }
    if export_url.is_empty() {
        return Err(AppError::field(
            "export_url",
            "A succession plan needs an export URL. Without one, a reader who follows the \
             destination is trusting it with the only copy.",
        ));
    }
    if !is_https(export_url) {
        return Err(AppError::field(
            "export_url",
            "The export URL must be https.",
        ));
    }
    if !is_https(contact) && !contact.starts_with("mailto:") {
        return Err(AppError::field(
            "contact",
            "Contact must be an https URL or a mailto: address.",
        ));
    }

    Ok(PlanDraft {
        destination: destination.to_owned(),
        export_url: export_url.to_owned(),
        contact: contact.to_owned(),
        review_days: draft.review_days.clamp(MIN_REVIEW_DAYS, MAX_REVIEW_DAYS),
    })
}

fn is_https(value: &str) -> bool {
    value.starts_with("https://") && value.len() > "https://".len()
}

/// Normalise a host for the registry: lowercase, no scheme, no port, no trailing dot.
///
/// Returns `None` for anything that is not a plausible host, so a caller cannot register a redirect
/// keyed on a path fragment by accident.
#[must_use]
pub fn normalise_host(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    // Drop a path, a query or a fragment: the registry keys on the host alone.
    let host_port = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = host_port
        .rsplit_once(':')
        .map_or(host_port, |(host, _maybe_port)| host)
        .trim_end_matches('.')
        .to_lowercase();
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return None;
    }
    Some(host)
}

/// Normalise a path prefix: a leading slash, no trailing slash unless it is the root.
///
/// The root is `/` and is a real prefix — "everything on that host" — which is why it keeps its slash
/// while every other prefix loses one, so `/works/1` and `/works/1/` do not become two rows.
#[must_use]
pub fn normalise_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some("/".to_owned());
    }
    let with_slash = if trimmed.starts_with('/') {
        trimmed.to_owned()
    } else {
        format!("/{trimmed}")
    };
    let without_trailing = with_slash.trim_end_matches('/');
    if without_trailing.is_empty() {
        return Some("/".to_owned());
    }
    Some(without_trailing.to_owned())
}

/// Whether `path` falls under the registered prefix `prefix`.
///
/// A prefix matches at a segment boundary, so `/works` matches `/works/1` but not `/workshop`.
#[must_use]
pub fn path_matches(prefix: &str, path: &str) -> bool {
    if prefix == "/" {
        return true;
    }
    if path == prefix {
        return true;
    }
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> PlanDraft {
        PlanDraft {
            destination: "https://elsewhere.example".to_owned(),
            export_url: "https://elsewhere.example/export".to_owned(),
            contact: "mailto:ops@elsewhere.example".to_owned(),
            review_days: 90,
        }
    }

    #[test]
    fn a_plan_without_an_export_is_refused() {
        let mut bad = draft();
        bad.export_url = "   ".to_owned();
        let error = validate(bad).expect_err("no export");
        assert!(error.field_errors().contains_key("export_url"), "{error:?}");
    }

    #[test]
    fn a_plain_http_destination_is_refused() {
        let mut bad = draft();
        bad.destination = "http://elsewhere.example".to_owned();
        assert!(validate(bad)
            .expect_err("http")
            .field_errors()
            .contains_key("destination"));
    }

    #[test]
    fn the_review_window_is_clamped_rather_than_refused() {
        let mut draft = draft();
        draft.review_days = 1;
        assert_eq!(
            validate(draft.clone()).unwrap().review_days,
            MIN_REVIEW_DAYS
        );
        draft.review_days = 5000;
        assert_eq!(validate(draft).unwrap().review_days, MAX_REVIEW_DAYS);
        let mut d = super::tests::draft();
        d.review_days = 120;
        assert_eq!(
            validate(d).unwrap().review_days,
            120,
            "inside the bounds, unchanged"
        );
    }

    #[test]
    fn staleness_is_computed_against_now() {
        let confirmed = "2026-01-01T00:00:00+00:00";
        let mut plan = Plan {
            destination: String::new(),
            export_url: String::new(),
            contact: String::new(),
            review_days: 90,
            confirmed_at: confirmed.to_owned(),
        };
        let inside = parse_time("2026-03-01T00:00:00+00:00").unwrap();
        let outside = parse_time("2026-04-05T00:00:00+00:00").unwrap();
        assert!(!plan.is_stale(inside), "59 days into a 90-day window");
        assert!(plan.is_stale(outside), "94 days into a 90-day window");
        plan.review_days = 365;
        assert!(!plan.is_stale(outside), "the same instant, a wider window");
    }

    #[test]
    fn an_unreadable_timestamp_reads_as_stale() {
        let plan = Plan {
            destination: String::new(),
            export_url: String::new(),
            contact: String::new(),
            review_days: 90,
            confirmed_at: "not a date".to_owned(),
        };
        // An unparseable row is not evidence of liveness.
        assert!(plan.is_stale(time::OffsetDateTime::now_utc()));
        assert!(plan.review_due().is_none());
    }

    #[test]
    fn hosts_are_normalised_to_one_key() {
        for raw in [
            "example.org",
            "EXAMPLE.org",
            "https://example.org",
            "http://example.org/works/1",
            "example.org:443",
            "https://example.org:443/",
            "example.org.",
        ] {
            assert_eq!(
                normalise_host(raw).as_deref(),
                Some("example.org"),
                "{raw} should normalise to example.org"
            );
        }
        assert!(
            normalise_host("localhost").is_none(),
            "not a host this registry keys on"
        );
        assert!(normalise_host("   ").is_none());
        assert!(normalise_host("exa mple.org").is_none());
    }

    #[test]
    fn paths_are_prefixes_with_one_shape() {
        assert_eq!(normalise_path("works/1").as_deref(), Some("/works/1"));
        assert_eq!(normalise_path("/works/1/").as_deref(), Some("/works/1"));
        assert_eq!(normalise_path("/").as_deref(), Some("/"));
        assert_eq!(normalise_path("").as_deref(), Some("/"));
        assert_eq!(normalise_path("///").as_deref(), Some("/"));
    }

    #[test]
    fn a_prefix_matches_at_a_segment_boundary() {
        assert!(path_matches("/works", "/works"));
        assert!(path_matches("/works", "/works/1"));
        assert!(path_matches("/", "/anything"));
        assert!(!path_matches("/works", "/workshop"));
        assert!(!path_matches("/works/1", "/works/10"));
    }
}

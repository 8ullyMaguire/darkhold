//! §21.6's gallery mechanics, as rules rather than as rows (M16-08).
//!
//! The gallery is where a reader decides whether a package is worth granting, so every rule here exists to keep one number or one line
//! honest. Four of them are worth stating up front, because each one has a plausible alternative that is wrong:
//!
//! * **An install is idempotent.** Installing something already installed is not an event: it neither adds a row nor moves a count. A
//!   count that a reader can advance by clicking twice is a count of clicks, not of installs.
//! * **One active installation per (scope, package).** The scope is the pseud, not the account, because an account may hold several
//!   identities and a package installed under one of them is not installed under the others. Version history lives in the row; a second
//!   active row for the same scope would be two answers to "what is running".
//! * **An install count is a census, not a total.** `installed − uninstalled` at this instant, never a running sum of install events, so
//!   a package cannot look popular because it was reinstalled during development.
//! * **A licence check is a compatibility question, not a lookup.** A copyleft source may be forked only under the same licence; a
//!   permissive one may be forked under anything that names it; an unrecognised licence is refused rather than assumed permissive, because
//!   the assumption is what a licence exists to prevent.
//!
//! # What is deliberately absent
//!
//! No per-account installation history is published anywhere — that is a reading-interest profile with a package's name on it. The
//! aggregate count is public; who is in it is not, and there is no function here that could answer the second question.

use std::fmt;

/// How a package follows its own releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdatePolicy {
    /// Stay where you are until the reader says otherwise.
    #[default]
    Pinned,
    /// Follow patch and minor releases of the pinned major.
    Minor,
    /// Follow every published version.
    Any,
}

impl UpdatePolicy {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pinned => "pinned",
            Self::Minor => "minor",
            Self::Any => "any",
        }
    }

    /// Parse the wire name.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Option<Self> {
        Some(match text {
            "pinned" => Self::Pinned,
            "minor" => Self::Minor,
            "any" => Self::Any,
            _ => return None,
        })
    }

    /// Whether `candidate` is one this policy would move to from `current`.
    ///
    /// Comparison is on the **major** field only, and only numerically: these are package versions, not semver ranges, and a policy that
    /// silently stopped matching because somebody wrote `1.0` instead of `1.0.0` would be a policy nobody could predict. A version that
    /// does not parse is never a candidate — an unreadable release is not a release.
    #[must_use]
    pub fn allows(self, current: &str, candidate: &str) -> bool {
        match self {
            Self::Pinned => candidate == current,
            Self::Any => parse_version(candidate).is_some(),
            Self::Minor => match (parse_version(current), parse_version(candidate)) {
                (Some((current_major, _, _)), Some((candidate_major, _, _))) => {
                    current_major == candidate_major
                }
                _ => false,
            },
        }
    }

    /// The newest candidate this policy allows, given the installed version and the published ones.
    ///
    /// `None` when nothing newer is allowed — which is the ordinary answer for a pinned install, not a failure.
    #[must_use]
    pub fn newest_allowed(self, current: &str, published: &[String]) -> Option<String> {
        let mut best: Option<(u64, u64, u64, &String)> = None;
        for candidate in published {
            if candidate == current || !self.allows(current, candidate) {
                continue;
            }
            let Some((major, minor, patch)) = parse_version(candidate) else {
                continue;
            };
            // Forward only. A policy says which releases an install follows, not that it may be rolled back by one; "the newest allowed"
            // that is older than what is installed is a downgrade wearing an update's name.
            if let Some((current_major, current_minor, current_patch)) = parse_version(current) {
                if (major, minor, patch) <= (current_major, current_minor, current_patch) {
                    continue;
                }
            }
            if best.is_none_or(|(b_major, b_minor, b_patch, _)| {
                (major, minor, patch) > (b_major, b_minor, b_patch)
            }) {
                best = Some((major, minor, patch, candidate));
            }
        }
        best.map(|(_, _, _, candidate)| candidate.clone())
    }
}

/// A three-field version, or nothing.
///
/// Every package version in this tree is `major.minor.patch`. A two-field version is accepted with `patch = 0` because that is the
/// spelling a human writes, and refusing it would make the manifest validator the only place a version could be written one way and read
/// another.
#[must_use]
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = match parts.next() {
        Some(patch) => patch.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// What an install request asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// A new active installation was created.
    Installed,
    /// One was already active for this scope; nothing changed, and the count did not move.
    AlreadyInstalled,
}

/// One installation, as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    pub manifest_id: String,
    pub version: String,
    pub policy: UpdatePolicy,
    pub active: bool,
}

/// Why an install, update or rating was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GalleryRefusal {
    /// The package does not exist, or is not published: there is nothing to install.
    NotPublished(String),
    /// The scope already runs a different version and the policy does not allow the change.
    PolicyRefuses {
        policy: &'static str,
        current: String,
        candidate: String,
    },
    /// The candidate version asks for capabilities the grant does not cover.
    ConsentRequired(Vec<String>),
    /// A rating outside 1–5. Zero is not "no opinion" — absence of a rating is.
    RatingOutOfRange(i64),
    /// A review longer than the cap.
    ReviewTooLong { length: usize, cap: usize },
    /// The scope has never installed the package, so it has nothing to have an opinion about.
    NotInstalled,
    /// The fork's licence is not compatible with its source's.
    IncompatibleLicence { source: String, derived: String },
    /// A licence this code has no rule for. Refused rather than assumed permissive.
    UnknownLicence(String),
}

impl GalleryRefusal {
    /// The sentence a caller may publish.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NotPublished(id) => {
                format!("`{id}` is not published, so there is nothing to install")
            }
            Self::PolicyRefuses {
                policy,
                current,
                candidate,
            } => format!(
                "this installation follows the `{policy}` update policy, which does not allow `{candidate}` from `{current}`"
            ),
            Self::ConsentRequired(new) => format!(
                "this version asks for {} the reader has not granted: {}",
                if new.len() == 1 { "a capability" } else { "capabilities" },
                new.join(", ")
            ),
            Self::RatingOutOfRange(value) => {
                format!("a rating is 1 to 5; {value} is not a rating")
            }
            Self::ReviewTooLong { length, cap } => {
                format!("a review may be {cap} characters; this one is {length}")
            }
            Self::NotInstalled => {
                "only a scope that has installed this package may rate it".to_owned()
            }
            Self::IncompatibleLicence { source, derived } => format!(
                "`{source}` may not be redistributed as `{derived}`; a copyleft licence requires the fork to keep it"
            ),
            Self::UnknownLicence(licence) => format!(
                "`{licence}` is not a licence this host has a rule for, so the fork is refused rather than assumed compatible"
            ),
        }
    }
}

/// A rating with an optional written review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rating {
    /// 1–5 inclusive.
    pub stars: i64,
    /// The written review, if the reader wrote one.
    pub review: Option<String>,
}

/// The longest review this host stores.
///
/// A cap rather than a page limit because the store is the thing that runs out first, and a review is not the place a reader discovers a
/// length rule after typing it: the route checks it before the write.
pub const REVIEW_CAP: usize = 4_000;

impl Rating {
    /// Validate a rating.
    ///
    /// `stars` is checked for range and the review for length; nothing else here decides anything, because "is this a good review" is a
    /// moderation question and not this type's.
    pub fn validate(stars: i64, review: Option<&str>) -> Result<Self, GalleryRefusal> {
        if !(1..=5).contains(&stars) {
            return Err(GalleryRefusal::RatingOutOfRange(stars));
        }
        if let Some(text) = review {
            if text.chars().count() > REVIEW_CAP {
                return Err(GalleryRefusal::ReviewTooLong {
                    length: text.chars().count(),
                    cap: REVIEW_CAP,
                });
            }
        }
        Ok(Self {
            stars,
            review: review.map(str::to_owned),
        })
    }
}

/// What a licence permits a fork to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenceClass {
    /// The work may be reused under any licence that names it.
    Permissive,
    /// A fork must carry the same licence.
    Copyleft,
}

/// Classify a licence identifier.
///
/// The list is short and explicit rather than prefix-matching: `CC-BY-4.0` is permissive and `CC-BY-SA-4.0` is copyleft, and a prefix rule
/// on `CC-BY` would classify both the same way — which would let a share-alike work be forked under a proprietary licence by a string that
/// merely started the same way.
#[must_use]
pub fn classify(licence: &str) -> Option<LicenceClass> {
    let normalised = licence.trim();
    Some(match normalised {
        "CC0-1.0" | "CC-BY-3.0" | "CC-BY-4.0" | "MIT" | "Apache-2.0" | "BSD-2-Clause"
        | "BSD-3-Clause" | "Unlicense" => LicenceClass::Permissive,
        "CC-BY-SA-4.0" | "CC-BY-SA-3.0" | "GPL-3.0-only" | "GPL-3.0-or-later" | "AGPL-3.0-only"
        | "AGPL-3.0-or-later" => LicenceClass::Copyleft,
        _ => return None,
    })
}

/// Whether `derived` may be published as a fork of `source`.
pub fn compatible(source: &str, derived: &str) -> Result<(), GalleryRefusal> {
    let source_class =
        classify(source).ok_or_else(|| GalleryRefusal::UnknownLicence(source.to_owned()))?;
    classify(derived).ok_or_else(|| GalleryRefusal::UnknownLicence(derived.to_owned()))?;
    match source_class {
        LicenceClass::Permissive => Ok(()),
        LicenceClass::Copyleft => {
            if source.trim() == derived.trim() {
                Ok(())
            } else {
                Err(GalleryRefusal::IncompatibleLicence {
                    source: source.to_owned(),
                    derived: derived.to_owned(),
                })
            }
        }
    }
}

/// The attribution a fork must carry, as text a page can print.
///
/// `derived_from` is the parent's id and the parent's version is not part of it: the parent may publish a new version and the fork's debt
/// is to the version it was taken from, which is why the caller passes that one in and this prints both.
#[must_use]
pub fn attribution(source_id: &str, source_version: &str, source_license: &str) -> String {
    format!("based on {source_id} {source_version} ({source_license})")
}

/// A package's public score.
///
/// `average` is `None` when nobody has rated: an unrated package has no score, and `0.0` would be indistinguishable from a package every
/// reader gave one star. `count` is always the real number so a caller can say "no ratings yet" without a second call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    pub average: Option<f64>,
    pub count: u64,
}

impl Score {
    /// Build a score from a sum and a count.
    #[must_use]
    pub fn from_sum(sum: i64, count: u64) -> Self {
        Self {
            average: if count == 0 {
                None
            } else {
                Some(sum as f64 / count as f64)
            },
            count,
        }
    }

    /// The rendered form: one decimal, or the word a page shows instead of a number.
    #[must_use]
    pub fn render(&self) -> String {
        match self.average {
            Some(average) => format!("{average:.1}"),
            None => "no ratings yet".to_owned(),
        }
    }
}

impl fmt::Display for Score {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinned_install_only_moves_to_its_own_version() {
        assert!(UpdatePolicy::Pinned.allows("1.0.0", "1.0.0"));
        assert!(!UpdatePolicy::Pinned.allows("1.0.0", "1.0.1"));
        assert_eq!(
            UpdatePolicy::Pinned.newest_allowed("1.0.0", &["1.0.1".into(), "2.0.0".into()]),
            None,
            "pinned means pinned, not 'newest that happens to exist'"
        );
    }

    #[test]
    fn a_minor_policy_stops_at_the_major_boundary() {
        assert_eq!(
            UpdatePolicy::Minor
                .newest_allowed("1.0.0", &["1.0.1".into(), "1.4.0".into(), "2.0.0".into()]),
            Some("1.4.0".to_owned())
        );
        assert_eq!(
            UpdatePolicy::Minor.newest_allowed("1.4.0", &["1.0.1".into()]),
            None,
            "no downgrade: an update policy follows releases forward"
        );
    }

    #[test]
    fn an_any_policy_takes_the_newest_readable_version() {
        assert_eq!(
            UpdatePolicy::Any.newest_allowed(
                "1.0.0",
                &["1.2.0".into(), "2.0.1".into(), "not-a-version".into()]
            ),
            Some("2.0.1".to_owned())
        );
    }

    #[test]
    fn a_two_field_version_is_readable_and_a_four_field_one_is_not() {
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.x.0"), None);
    }

    #[test]
    fn a_rating_outside_one_to_five_is_refused_and_zero_is_not_an_abstention() {
        assert!(Rating::validate(0, None).is_err());
        assert!(Rating::validate(6, None).is_err());
        assert!(Rating::validate(5, Some("fine")).is_ok());
    }

    #[test]
    fn a_review_longer_than_the_cap_is_refused_with_the_length() {
        let long = "x".repeat(REVIEW_CAP + 1);
        let refusal = Rating::validate(3, Some(&long)).expect_err("too long");
        assert_eq!(
            refusal,
            GalleryRefusal::ReviewTooLong {
                length: REVIEW_CAP + 1,
                cap: REVIEW_CAP
            }
        );
    }

    #[test]
    fn a_copyleft_source_will_not_be_relabelled_by_a_fork() {
        assert!(compatible("CC-BY-SA-4.0", "CC-BY-SA-4.0").is_ok());
        let refusal = compatible("CC-BY-SA-4.0", "MIT").expect_err("copyleft");
        assert_eq!(
            refusal,
            GalleryRefusal::IncompatibleLicence {
                source: "CC-BY-SA-4.0".to_owned(),
                derived: "MIT".to_owned()
            }
        );
        // A permissive source may be forked under a copyleft licence, which is what share-alike means.
        assert!(compatible("MIT", "GPL-3.0-only").is_ok());
    }

    #[test]
    fn an_unrecognised_licence_is_refused_rather_than_assumed_permissive() {
        let refusal = compatible("Proprietary-1.0", "MIT").expect_err("unknown");
        assert_eq!(
            refusal,
            GalleryRefusal::UnknownLicence("Proprietary-1.0".to_owned())
        );
    }

    #[test]
    fn a_cc_by_prefix_does_not_make_share_alike_permissive() {
        // The reason `classify` is a list and not a prefix rule.
        assert_eq!(classify("CC-BY-4.0"), Some(LicenceClass::Permissive));
        assert_eq!(classify("CC-BY-SA-4.0"), Some(LicenceClass::Copyleft));
    }

    #[test]
    fn an_unrated_package_has_no_average_rather_than_zero() {
        let score = Score::from_sum(0, 0);
        assert_eq!(score.average, None);
        assert_eq!(score.render(), "no ratings yet");
        let rated = Score::from_sum(9, 3);
        assert_eq!(rated.render(), "3.0");
    }

    #[test]
    fn attribution_names_the_version_the_fork_was_taken_from() {
        assert_eq!(
            attribution("shelf-sorter", "1.0.0", "MIT"),
            "based on shelf-sorter 1.0.0 (MIT)"
        );
    }
}

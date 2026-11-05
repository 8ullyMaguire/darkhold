//! M70-08 — step-up authentication, decided from the issuer's `acr` rather than invented here.
//!
//! # The three states, which are not two
//!
//! A session is **satisfied** (its assurance meets what the operation needs), **step-up required** (the session has
//! an assurance and it is too low), or **unknown** (the session carries no `acr` at all).
//!
//! The third state is the one that decides this module's honesty. A password session has no issuer-asserted
//! assurance: not "level 0", not "unverified" — nothing was claimed. Treating that as *low* would lock every operator
//! on a site without single sign-on out of their own admin tools; treating it as *high* would be a lie. So `unknown`
//! is its own answer, and the caller decides — the guard in the app admits it only where step-up is not configured,
//! and refuses it where it is.
//!
//! # Why parse `acr` at all instead of comparing strings
//!
//! `acr` values are not a closed vocabulary. The RFC 8176 forms (`urn:…:aal1`…`aal3`) are common, bare digits are
//! common, and an issuer may use its own string. A string comparison would silently treat `aal2` and `2` as
//! different security levels, so the two known families are parsed to a rank and anything else is `unknown` — which
//! fails closed rather than guessing.

/// What an operation needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// Ordinary authenticated work.
    Session,
    /// Re-authentication with a second factor: credential changes, session revocation, destructive admin actions.
    SecondFactor,
}

impl Requirement {
    /// The lowest assurance that satisfies it.
    #[must_use]
    pub fn level(self) -> u8 {
        match self {
            Self::Session => 1,
            Self::SecondFactor => 2,
        }
    }
}

/// What the guard learned about a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Carry on.
    Satisfied,
    /// The session has an assurance and it is below what the operation needs.
    StepUp {
        /// The level the session has.
        have: u8,
        /// The level the operation needs.
        need: u8,
    },
    /// The session carries no `acr`: no issuer asserted anything about how it was authenticated.
    Unknown {
        /// The level the operation would need if it could be established.
        need: u8,
    },
}

/// Parse an `acr` into a comparable rank.
///
/// Two families, because those are the two that mean something across issuers: bare digits (`"1"`, `"2"`, `"3"`)
/// and RFC 8176 (`urn:mace:incommon:iap:silver` style is *not* ranked — the AAL forms are). Anything else, including
/// an empty string, is `None` and the caller must treat it as unknown rather than as zero.
#[must_use]
pub fn level(acr: &str) -> Option<u8> {
    let trimmed = acr.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = trimmed.parse::<u8>() {
        return (value <= 4).then_some(value);
    }
    // `urn:…:aal1|aal2|aal3` — the tail after the last colon, case-insensitively.
    let tail = trimmed.rsplit(':').next()?.to_ascii_lowercase();
    match tail.as_str() {
        "aal1" => Some(1),
        "aal2" => Some(2),
        "aal3" => Some(3),
        _ => None,
    }
}

/// Decide whether a session's `acr` satisfies a requirement.
#[must_use]
pub fn check(acr: Option<&str>, requirement: Requirement) -> Verdict {
    let need = requirement.level();
    match acr.and_then(level) {
        Some(have) if have >= need => Verdict::Satisfied,
        Some(have) => Verdict::StepUp { have, need },
        None => Verdict::Unknown { need },
    }
}

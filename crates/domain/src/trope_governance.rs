//! Trope proposal rules (M45-77). No I/O, no trust lookups, no timestamps.

/// The only actions a proposal may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TropeAction {
    Create,
    Update,
    Retire,
}

impl TropeAction {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Retire => "retire",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            "retire" => Some(Self::Retire),
            _ => None,
        }
    }
}

/// The proposer is taken from the session; a body cannot claim authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposerRole {
    Curator,
    Trusted,
    Operator,
}

/// Reuse the tested category-governance tally rather than a second rule.
#[must_use]
pub fn proposal_decided(yes: u32, no: u32, quorum: u32) -> Option<bool> {
    crate::category_governance::proposal_decided(yes, no, quorum)
}

/// Effective status before a sweep writes `expired`. Closed slugs use
/// lexicographically ordered `YYYY-MM-DDTHH:MM:SSZ` timestamps on both engines.
#[must_use]
pub fn effective_status(stored: &str, closes_at: &str, now: &str) -> String {
    if stored == "proposed" && now >= closes_at {
        "expired".to_owned()
    } else {
        stored.to_owned()
    }
}

/// Slug grammar. Deliberately closed: a slug becomes a query token.
#[must_use]
pub fn valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=40).contains(&b.len())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

//! When a moderator is due.
//!
//! This module began as a three-level scale (TL1/TL2/TL3) computed from accepted counts. Decision (c) retired that scale: the
//! instance already has a trust ladder with six levels, four evidence axes and an audit trail, and a second scale whose "2" and "3"
//! looked like the first one's would have overwritten that evidence on every promotion. What survives is the single question this
//! module is actually qualified to answer — **at which level is a moderator claim due** — and it is answered by one number.

/// The level at which `role:moderator:<site>` is due.
///
/// **Four, because the ladder already names four "moderator".** `TrustLadder::default_ladder()` reads: 2 member (40 points),
/// 3 trusted (120), 4 moderator (240). I picked five by hand, reasoning from "six is the operator", and measured on a copy of the
/// store an account with 400 accepted proposals plateaus at **level 4**: five would have been a threshold nobody could reach, and the
/// role would never have been minted. The product had already answered the question, in the level's own name.
pub const MODERATOR_LEVEL: u8 = 4;

/// Whether a level is high enough that a moderator claim is due.
pub const fn role_due(level: u8) -> bool {
    level >= MODERATOR_LEVEL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moderator_claim_is_due_from_level_four() {
        assert!(!role_due(3), "three is \"trusted\", not a moderator");
        assert!(role_due(4), "four is named \"moderator\" in the ladder");
        assert!(role_due(5), "and everything above it");
        assert!(
            role_due(6),
            "the operator designated by the instance owner outranks the ladder, not the other way round"
        );
    }

    #[test]
    fn the_boundary_is_inclusive_on_one_side_only() {
        assert!(!role_due(MODERATOR_LEVEL - 1));
        assert!(role_due(MODERATOR_LEVEL));
    }
}

//! When a moderator is due.
//!
//! This module began as a three-level scale (TL1/TL2/TL3) computed from accepted counts. Decision (c) retired that scale: the
//! instance already has a trust ladder with six levels, four evidence axes and an audit trail, and a second scale whose "2" and "3"
//! looked like the first one's would have overwritten that evidence on every promotion. What survives is the single question this
//! module is actually qualified to answer — **at which level is a moderator claim due** — and it is answered by one number.

/// The level at which `role:moderator:<site>` is due.
///
/// Level 6 is the operator (`trust_levels.basis` reads "operator: designated by the instance owner" for the one account holding it),
/// so five is the highest level a person can *earn*. A moderator claim due there sits on the top earned rung and still below the
/// account the owner appointed by hand.
pub const MODERATOR_LEVEL: u8 = 5;

/// Whether a level is high enough that a moderator claim is due.
pub const fn role_due(level: u8) -> bool {
    level >= MODERATOR_LEVEL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moderator_claim_is_due_from_level_five() {
        assert!(!role_due(4), "four is not a moderator");
        assert!(
            role_due(5),
            "five is the highest earned level, and the rung a moderator sits on"
        );
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

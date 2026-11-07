//! When a level is earned (W7 task 4).
//!
//! The wave's ladder: TL1 is everyone, TL2 at 20 accepted contributions, TL3 at 100 accepted **and no upheld report against
//! the person**. This module is deliberately pure — no database, no clock, no job handle — because a rule about who may
//! moderate is the one thing here that should be testable without a fixture, and because a threshold that cannot be read at
//! its boundary (19 vs 20) is a threshold nobody has verified.
//!
//! What it does **not** decide: what a level permits. `darkhold_domain::trust` owns that (`Privilege`, `privilege_check`,
//! elastic allocation for small sites). The ladder decides when a level is reached and what that mints — a `role:moderator:<site>`
//! claim in `polaris-id` — so the admin door stays `require_admin_of` and there is one answer to "may this person moderate".

/// The counts a level is derived from. `accepted` is contributions the community kept; `upheld_against` is reports against
/// this person that a moderator upheld.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub accepted: u32,
    pub upheld_against: u32,
}

/// TL1 by default; TL2 at 20 accepted; TL3 at 100 accepted with no upheld report against the person.
///
/// The veto is not a subtraction — a person with 400 accepted contributions and one upheld report is TL2, not TL1 and not
/// TL3. A veto that scaled with volume would let a prolific contributor outweigh a finding against them.
pub fn level_for(counts: Counts) -> u8 {
    if counts.accepted >= 100 && counts.upheld_against == 0 {
        return 3;
    }
    if counts.accepted >= 20 {
        return 2;
    }
    1
}

/// The evidence behind a level, stored with it so the number can be re-derived later without re-running history.
pub fn basis_for(counts: Counts) -> String {
    format!(
        "accepted={}, upheld_against={}",
        counts.accepted, counts.upheld_against
    )
}

/// Whether a transition from `from` to `to` should mint the moderator claim. Minting happens on crossing into TL3, and
/// revoking on leaving it — not on every sweep, or the entitlements table would collect a row per run.
pub fn mints_role(from: u8, to: u8) -> bool {
    from < 3 && to >= 3
}

/// Whether a transition should revoke a previously minted claim.
pub fn revokes_role(from: u8, to: u8) -> bool {
    from >= 3 && to < 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(accepted: u32, upheld: u32) -> Counts {
        Counts {
            accepted,
            upheld_against: upheld,
        }
    }

    #[test]
    fn the_second_threshold_is_twenty_not_nineteen() {
        assert_eq!(level_for(counts(19, 0)), 1);
        assert_eq!(level_for(counts(20, 0)), 2);
    }

    #[test]
    fn the_third_threshold_is_a_hundred_not_ninety_nine() {
        assert_eq!(level_for(counts(99, 0)), 2);
        assert_eq!(level_for(counts(100, 0)), 3);
    }

    #[test]
    fn one_upheld_report_vetoes_the_third_level_and_only_that_one() {
        assert_eq!(level_for(counts(100, 1)), 2);
        assert_eq!(level_for(counts(100, 0)), 3);
        // The veto does not push a person below what their accepted work earned.
        assert_eq!(level_for(counts(20, 1)), 2);
        assert_eq!(level_for(counts(5, 9)), 1);
    }

    #[test]
    fn volume_never_outweighs_a_finding() {
        // 10,000 accepted contributions and one upheld report: still TL2. If this ever returns 3, a prolific contributor has
        // bought their way past a moderator's finding, which is the failure this rule exists to prevent.
        assert_eq!(level_for(counts(10_000, 1)), 2);
    }

    #[test]
    fn the_role_is_minted_on_the_crossing_only() {
        assert!(mints_role(2, 3));
        assert!(!mints_role(3, 3), "a sweep that changes nothing mints nothing");
        assert!(!mints_role(1, 2));
        assert!(revokes_role(3, 2));
        assert!(!revokes_role(2, 3));
        assert!(!revokes_role(0, 3), "no previous level is not a demotion");
    }

    #[test]
    fn basis_records_both_numbers() {
        let basis = basis_for(counts(104, 0));
        assert!(basis.contains("accepted=104"), "{basis}");
        assert!(basis.contains("upheld_against=0"), "{basis}");
    }
}

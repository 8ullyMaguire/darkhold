//! M70-01 — orphaning: leaving is not deleting.
//!
//! A creator who wants out has two ways to go. Deleting takes the work with them, and readers who bookmarked,
//! cited or translated it lose it. Orphaning keeps the work and detaches the person: the account link goes, the
//! name stays as a frozen string, and nothing in the product can give the work back.
//!
//! # Why this is a state and not a workflow
//!
//! The temptation is a table with steps (requested, confirmed, applied). It buys nothing: orphaning has one
//! transition and it is irreversible, so the only facts worth storing are *who owned it* (nobody) and *what the
//! author line says* (a frozen label). A workflow would also suggest a way back, and there is deliberately none —
//! `may_reclaim` exists to answer that question with a `false` rather than with a missing route.
//!
//! # The label is frozen, not recomputed
//!
//! AO3 shows `orphan_account` as the author once a work is orphaned. This module freezes the *original* pseud into
//! the label, because a reader who followed the work needs to recognise it, and because recomputing the label
//! later would reattach the work to a person the author asked to be detached from.

/// Appended to the last known pseud when a work is orphaned.
pub const ORPHAN_SUFFIX: &str = " (orphaned)";

/// The author line of an orphaned work, frozen at the moment of orphaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenLabel(String);

impl FrozenLabel {
    /// The text to render where an author name usually goes.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a work cannot be orphaned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// It is already orphaned. Orphaning twice would append the suffix twice and imply a second owner to detach.
    AlreadyAnOrphan,
    /// There is no owner to detach — the work was imported, or it is already detached.
    NoOwner,
    /// The label would be empty, and an author line that reads nothing is worse than the pseud it replaced.
    EmptyPseud,
}

/// Freeze the author line for a work being orphaned.
///
/// `pseud` is the display name the work is published under, not the account name: a reader knows the pseud, and
/// orphaning an account with five pseudonyms must not relabel all five works to the account's login.
pub fn freeze(
    pseud: &str,
    has_owner: bool,
    already_orphaned: bool,
) -> Result<FrozenLabel, Refusal> {
    if already_orphaned {
        return Err(Refusal::AlreadyAnOrphan);
    }
    if !has_owner {
        return Err(Refusal::NoOwner);
    }
    let trimmed = pseud.trim();
    if trimmed.is_empty() {
        return Err(Refusal::EmptyPseud);
    }
    Ok(FrozenLabel(format!("{trimmed}{ORPHAN_SUFFIX}")))
}

/// Whether a work is orphaned. The owner link is the whole state — there is no flag to disagree with it.
#[must_use]
pub fn is_orphaned(owner_account_id: Option<&str>) -> bool {
    owner_account_id.is_none()
}

/// Whether an orphaned work can be claimed back.
///
/// Always `false`, and total rather than absent: a caller asks the rule instead of inventing a route that answers
/// it, and a future "adoption" feature has to change this function — a visible place — to make reclaiming real.
#[must_use]
pub fn may_reclaim(_orphaned: bool) -> bool {
    false
}

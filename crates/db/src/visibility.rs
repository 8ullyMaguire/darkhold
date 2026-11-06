//! Which `works` rows a surface may show, and which it may learn from.
//!
//! `restricted` is a *listed but locked* tier (M70-04, decision of 2026-11-05): a work's record — title, summary, tags,
//! author — is public, while its text and comments need a session. That one sentence splits every query in this crate
//! into two classes, and the split is a rule rather than a preference:
//!
//! * **LIST** surfaces show the work. Catalogue and freshness feeds, search, tag and fandom browse, collections, "more
//!   by this author", "new in your fandoms". The predicate is `visibility IN ('public', 'restricted')`. A stranger may
//!   find a locked work and be told why they cannot read it; that is the whole point of the tier, and a refusal they
//!   never reach is a refusal they cannot act on.
//!
//! * **INFER** surfaces must not. Taste vectors, tasting candidate pools, the recommendation ladder, blind dates and
//!   "surprise me", personalised and public recommendations, "is this pseud discoverable", arena pools. The predicate
//!   stays `visibility = 'public'`, and the reason is not secrecy but *evidence*: nobody could have read a locked work
//!   without signing in, so an edge drawn from it says two readers share a taste for text neither of them was shown,
//!   and a kudos or reader count drawn from it is a number nobody could have contributed to. Inference from a locked
//!   work is not a leak of the work; it is a claim about people made from a non-event.
//!
//! The distinction is not "public vs private" and it is not "anonymous vs signed in": a signed-in reader may also be
//! refused a locked work (by audience, rating or block), so an inference built on it would be wrong for them too. The
//! line is whether the surface *shows* the work or *concludes* from it.
//!
//! Unlisted works are invisible to both classes, and `private`-style words that this build does not recognise are
//! refused as absent (see `routes::works::parse_visibility`), because an unknown state must never become the most
//! open one.
//!
//! When adding a query, the question is not "which filter is stricter" but "does this surface show the work, or does
//! it conclude from it". `crates/db/tests/m70_12_visibility_surfaces.rs` asserts one of each in both directions, with an
//! unlisted work as the control that must stay invisible everywhere.

/// Whether a stored visibility word makes a work **listed**.
///
/// The same rule as the SQL predicates above, in Rust, for the call sites that reason about a transition rather than
/// query a table (the deindex on a visibility change). An unrecognised word is not listable: the state nobody
/// understands must never be the open one.
#[must_use]
pub fn is_listable(stored: &str) -> bool {
    matches!(stored, "public" | "restricted")
}

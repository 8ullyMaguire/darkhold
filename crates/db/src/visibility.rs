//! Which `works` rows a surface may enumerate, and which it may learn from.
//!
//! **Restricted is not listed.** This module exists because that sentence was briefly wrong here, and the correction is
//! worth keeping next to the code rather than only in a commit message.
//!
//! `restricted` withholds the record from enumeration. Existence is itself a disclosure (spec §8.9), so a stranger is
//! not told the work exists at all: search, tag and fandom browse, freshness feeds, collection contents, "more by this
//! author" and the catalogue all filter `visibility = 'public'`. Spec §4814 is explicit — "private, unlisted and
//! restricted works are never included" in indexing — and §5888 requires that a restricted work render no card and
//! answer exactly as a non-existent one, "so the endpoint is not an existence oracle".
//!
//! The one door is a direct link plus the operator's opt-in. `works.gated_metadata_public` (default **off**) is what may
//! let a restricted work's record render with `content_withheld`; the body still needs a session (M70-04). With the
//! default configuration a restricted work answers `404` to a stranger — which is why listing it would hand the reader a
//! link that dies on click, the failure §8.13 names.
//!
//! The second rule is unchanged and was always right: **no surface infers from a restricted work.** Taste vectors,
//! tasting pools, the recommendation ladder, blind dates and "surprise me", public and personalised recommendations,
//! arena pools and the works count on a pseud's row all stay `visibility = 'public'`. The reason is evidence, not
//! secrecy: nobody could have read a work they were never shown, so a similarity edge, a kudos count or a "3 works"
//! line built on it is a claim about people made from a non-event. A signed-in reader can also be refused a restricted
//! work (by audience, rating or block), so "the work exists" never implies "the work was read".
//!
//! Unlisted works are invisible to both classes. An unrecognised visibility word is refused as absent rather than
//! treated as the most open state (see `routes::works::parse_visibility`).
//!
//! `crates/db/tests/m70_12_visibility_surfaces.rs` asserts both directions with an unlisted work as the control, and
//! records what the earlier version of this module claimed, so the same mistake is not re-derived as a good idea.

/// Whether a stored visibility word makes a work **listable**.
///
/// Only `public`. Call sites that reason about a transition rather than query a table (the deindex on a visibility
/// change) use this so that "listed" has one definition in Rust and one in SQL. An unrecognised word is not listable:
/// the state nobody understands must never be the open one.
#[must_use]
pub fn is_listable(stored: &str) -> bool {
    stored == "public"
}

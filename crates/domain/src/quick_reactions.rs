//! §9.4 — the labelled one-click reaction bar.
//!
//! Six labels, no typing, pre-classified positive. They are **not** §35.1's typed votes
//! (`well_written`, `insightful`, …) and must not share their storage: §9.4 counts one row per
//! reader per *label*, so a reader may hold several at once on the same work, which a table keyed
//! on `(work, pseud)` cannot express.

/// The vocabulary, as `(slug, display label)`.
///
/// The slug is the key in `quick_reaction` and the path segment of the toggle route; the display
/// label is the spec's own wording, which carries spaces and `!` and therefore cannot be a URL
/// segment. Two strings rather than a `Display` impl because the frontend needs the display text
/// without a round trip to the server.
pub const QUICK_REACTIONS: &[(&str, &str)] = &[
    ("made_me_cry", "made me cry"),
    ("banter", "the banter!"),
    ("need_more", "I need more of this"),
    ("this_scene", "this scene!"),
    ("comforting", "comforting"),
    ("worldbuilding", "worldbuilding!"),
];

/// Whether a label may be recorded.
///
/// Refused rather than stored: unlike `work_reactions.vote_type`, this column is never free text,
/// so a typo cannot become a label that counts once for ever.
#[must_use]
pub fn is_known(label: &str) -> bool {
    QUICK_REACTIONS.iter().any(|(slug, _)| *slug == label)
}

/// Every slug, in vocabulary order.
#[must_use]
pub fn slugs() -> Vec<&'static str> {
    QUICK_REACTIONS.iter().map(|(slug, _)| *slug).collect()
}

/// The display label for a slug, if it is one.
#[must_use]
pub fn display_label(label: &str) -> Option<&'static str> {
    QUICK_REACTIONS
        .iter()
        .find(|(slug, _)| *slug == label)
        .map(|(_, label)| *label)
}

/// The message a 422 gives, naming what is allowed.
///
/// A refusal that only says "unknown label" leaves the reader to guess the vocabulary; this lists
/// it, which is the same courtesy the query parser pays for a mistyped field.
#[must_use]
pub fn refusal() -> String {
    format!("Not one of the reaction labels: {}", slugs().join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_specs_six_labels_are_all_here() {
        assert_eq!(slugs().len(), 6);
        for (slug, label) in QUICK_REACTIONS {
            assert!(is_known(slug), "{slug} is known");
            assert_eq!(display_label(slug), Some(*label));
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn a_typed_vote_is_not_a_quick_reaction() {
        // The two vocabularies are distinct on purpose: `insightful` belongs to §35.1's bar, and
        // accepting it here would put one vote in two tallies.
        assert!(!is_known("insightful"));
        assert!(!is_known("well_written"));
        assert!(!is_known(""));
        assert!(!is_known("Made_Me_Cry"), "slugs are exact");
    }

    #[test]
    fn the_refusal_names_the_vocabulary() {
        let message = refusal();
        assert!(message.contains("made_me_cry"));
        assert!(message.contains("worldbuilding"));
    }
}

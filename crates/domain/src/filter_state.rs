//! One state for the query string and the facet panel (M71-15).
//!
//! A search page has two controls that describe the same thing: the editable string and the panel of chips. Presenting the string as *the*
//! state is what this module exists to prevent — the grammar has no field for a reader's content filters (or for provenance and source, when
//! those become filters), so "the string is the state" means those filters vanish the moment someone types, and nothing tells them.
//!
//! So the state is a value that renders **twice**:
//!
//! * [`FilterState::query_string`] is the grammar's spelling of everything the grammar can carry, in a fixed order, so the same state always
//!   produces the same string (a canonical form is what lets a client compare two URLs, and what makes a round trip testable);
//! * [`FilterState::chips`] is every part of the state as a chip, each saying whether the string can express it and whether it may be removed.
//!
//! A chip the grammar cannot carry is **not removable**: the panel must render it pinned, with the reason, because the alternative is a reader
//! editing the string and silently losing a filter they can no longer see the effect of (§8.14's named degradation, §8.2's closed
//! vocabulary).
//!
//! Everything here is pure: no database, no clock, no I/O.

use crate::query::{parse_query, QueryAst};

/// A filter the grammar cannot spell, carried alongside the string.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PinnedFilter {
    /// The filter's family, in the filter system's own words: a content filter's type (`tag`, `mood`, `warning`), `provenance`, `source`,
    /// `collection`. Owned because the family is the producer's vocabulary, not a closed set this module can enumerate.
    pub kind: String,
    /// The value, as the filter system stores it.
    pub value: String,
    /// The words the panel shows. Written by the producer so two surfaces cannot label one filter differently.
    pub label: String,
    /// Why the query string cannot carry it. Printed next to the pinned chip.
    pub reason: &'static str,
}

/// The whole search state: what the reader typed, the panel's values, and what has no field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FilterState {
    /// The free-text query, as typed.
    pub text: String,
    /// Inclusive lower bound on words.
    pub min_words: Option<i64>,
    /// Inclusive upper bound on words.
    pub max_words: Option<i64>,
    /// Highest rating shown, by the catalogue's own order.
    pub max_rating: Option<String>,
    /// The sort in force, when it is not the surface default.
    pub sort: Option<String>,
    /// Filters with no field in the grammar.
    pub pinned: Vec<PinnedFilter>,
}

impl FilterState {
    /// The grammar's spelling of this state, in canonical order.
    ///
    /// The order is fixed — text, words, rating, sort — because two strings for one state is how a client decides the state changed when it
    /// did not, and how a round-trip test would have to accept either spelling and prove nothing.
    #[must_use]
    pub fn query_string(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let text = self.text.trim();
        if !text.is_empty() {
            // Quoted when the text would otherwise be read as grammar: a bare word containing a space or a colon is a different query.
            let needs_quotes = text.contains(' ') || text.contains(':');
            let escaped = text.replace('"', "\\\"");
            if needs_quotes {
                parts.push(format!("\"{escaped}\""));
            } else {
                parts.push(escaped);
            }
        }
        if let Some(min) = self.min_words {
            parts.push(format!("words:>={min}"));
        }
        if let Some(max) = self.max_words {
            parts.push(format!("words:<={max}"));
        }
        if let Some(rating) = &self.max_rating {
            parts.push(format!("rating:<={rating}"));
        }
        // Deliberately no `sort:` term: the grammar has no such field, so writing one would produce a string that no longer parses —
        // the exact failure this module exists to prevent. Sort is panel state and is reported as a chip below.
        parts.join(" ")
    }

    /// Every part of the state, as the panel should draw it.
    ///
    /// Expressible parts are `editable_in_query: true` and removable: dropping their token *is* how a reader removes them. Pinned parts are
    /// neither, and carry the reason — which is the whole point of the row: a filter with no field must not be droppable by editing.
    #[must_use]
    pub fn chips(&self) -> Vec<Chip> {
        let mut chips: Vec<Chip> = Vec::new();
        if let Some(min) = self.min_words {
            chips.push(Chip::expressible(
                "min_words",
                &min.to_string(),
                &format!("{min}+ words"),
            ));
        }
        if let Some(max) = self.max_words {
            chips.push(Chip::expressible(
                "max_words",
                &max.to_string(),
                &format!("at most {max} words"),
            ));
        }
        if let Some(rating) = &self.max_rating {
            chips.push(Chip::expressible(
                "max_rating",
                rating,
                &format!("up to {rating}"),
            ));
        }
        if let Some(sort) = &self.sort {
            // The panel owns the ordering, so it can remove it — but the string cannot express it, and a chip that claimed otherwise
            // would send a reader to edit a token that does not exist.
            chips.push(Chip {
                kind: "sort".to_owned(),
                value: sort.clone(),
                label: format!("sorted by {sort}"),
                editable_in_query: false,
                removable: true,
                reason: Some("the query grammar has no sort field; the ordering control owns it"),
            });
        }
        for pinned in &self.pinned {
            chips.push(Chip {
                kind: pinned.kind.clone(),
                value: pinned.value.clone(),
                label: pinned.label.clone(),
                editable_in_query: false,
                // Never removable: the panel is the only place this filter exists, so "remove it from the string" is not an option a page
                // may offer — and a chip that claimed otherwise would be a control that appears to work and silently does nothing.
                removable: false,
                reason: Some(pinned.reason),
            });
        }
        chips
    }

    /// The words a page prints under the string when some of the state is not in it.
    ///
    /// `None` when everything is expressible, so a page does not render an apology for a state that has nothing to apologise for.
    #[must_use]
    pub fn unexpressed_note(&self) -> Option<String> {
        if self.pinned.is_empty() {
            return None;
        }
        Some(format!(
            "{} of this search's filters {} not written in the query: {}.",
            self.pinned.len(),
            if self.pinned.len() == 1 { "is" } else { "are" },
            self.pinned
                .iter()
                .map(|filter| filter.label.as_str())
                .collect::<Vec<&str>>()
                .join(", ")
        ))
    }

    /// Read a string back into a state's expressible half.
    ///
    /// The pinned half cannot come from a string by definition, so a caller that has pinned filters keeps them: this returns the state the
    /// string describes, and the caller decides whether it still applies. Returning a state that silently dropped them would be the very bug
    /// this module is named after.
    #[must_use]
    pub fn from_query_string(input: &str) -> Self {
        let mut state = FilterState::default();
        let Ok(ast) = parse_query(input) else {
            // An unparseable string is the reader's half-typed state. Reporting it as empty would replace what they are typing with nothing;
            // keeping it as text at least round-trips, and the search route is what refuses it with a position.
            state.text = input.trim().to_owned();
            return state;
        };
        let mut text = Vec::new();
        let mut comparisons = Vec::new();
        collect(&ast, &mut text, &mut comparisons);
        state.text = text.join(" ");
        for (field, op, value) in comparisons {
            match (field.as_str(), op.as_str()) {
                ("words", ">=") | ("words", ">") => state.min_words = value.parse().ok(),
                ("words", "<=") | ("words", "<") => state.max_words = value.parse().ok(),
                ("rating", "<=") | ("rating", "<") => state.max_rating = Some(value),
                _ => {}
            }
        }
        state
    }
}

/// One chip in the panel.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Chip {
    pub kind: String,
    pub value: String,
    pub label: String,
    /// Whether removing the chip's token from the string would remove the filter.
    pub editable_in_query: bool,
    /// Whether the panel may offer a remove control at all.
    pub removable: bool,
    /// Why the string cannot carry it; `None` for the expressible ones.
    pub reason: Option<&'static str>,
}

impl Chip {
    fn expressible(kind: &str, value: &str, label: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            value: value.to_owned(),
            label: label.to_owned(),
            editable_in_query: true,
            removable: true,
            reason: None,
        }
    }
}

/// Walk an AST, splitting it into the free text it contains and its comparisons.
///
/// The AST keeps text, fielded terms and comparisons as distinct nodes, so the walk is where "what is this query saying in free text" is
/// decided — in one place, rather than in every caller that wants to prefill a search box.
fn collect(
    ast: &QueryAst,
    text: &mut Vec<String>,
    comparisons: &mut Vec<(String, String, String)>,
) {
    match ast {
        QueryAst::Text(value) | QueryAst::Phrase(value) => text.push(value.trim().to_owned()),
        QueryAst::Comparison(field, op, value) => comparisons.push((
            field.as_str().to_owned(),
            op.as_str().to_owned(),
            value.clone(),
        )),
        // A fielded term is text *and* structure. It stays in the text half so an edit preserves it verbatim;
        // turning it into a panel value would need a field-by-field mapping that the panel does not have.
        QueryAst::Fielded(field, value) => {
            text.push(format!("{}:{}", field.as_str(), quote_if_needed(value)));
        }
        QueryAst::And(parts) | QueryAst::Or(parts) | QueryAst::MinMatch { terms: parts, .. } => {
            for part in parts {
                collect(part, text, comparisons);
            }
        }
        QueryAst::Not(inner) => collect(inner, text, comparisons),
        // Assertions and expansions have no panel representation and no plain-text spelling that would survive a round trip;
        // they are left out of both halves and stay in the string the reader is editing.
        QueryAst::ExistsCharacter(_)
        | QueryAst::ExistsRelationship(_)
        | QueryAst::Scoped(_)
        | QueryAst::Expand { .. } => {}
    }
}

fn quote_if_needed(value: &str) -> String {
    if value.contains(' ') || value.contains(':') {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> FilterState {
        FilterState {
            text: "slow burn".to_owned(),
            min_words: Some(1_000),
            max_words: Some(50_000),
            max_rating: Some("mature".to_owned()),
            sort: Some("kudos".to_owned()),
            pinned: Vec::new(),
        }
    }

    #[test]
    fn the_query_string_is_canonical_and_carries_every_expressible_filter() {
        let query = state().query_string();
        assert_eq!(
            query,
            "\"slow burn\" words:>=1000 words:<=50000 rating:<=mature"
        );
        // Twice, to prove the order is the state's and not an artefact of how the string was built.
        assert_eq!(query, state().query_string());
    }

    #[test]
    fn a_round_trip_recovers_the_filters_the_grammar_can_carry() {
        let round = FilterState::from_query_string(&state().query_string());
        assert_eq!(round.text, "slow burn");
        assert_eq!(round.min_words, Some(1_000));
        assert_eq!(round.max_words, Some(50_000));
        assert_eq!(round.max_rating.as_deref(), Some("mature"));
        // Sort is not in the string, because the grammar has no field for it: a `sort:kudos` token would be read back as free text and
        // the round trip would then be measured against a string that means something else.
        assert!(
            !state().query_string().contains("sort:"),
            "a token the parser does not know is not a filter: {}",
            state().query_string()
        );
        assert!(round.sort.is_none(), "{round:?}");
        // The rating ceiling is the facet that motivated the grammar extension: before `rating:<=mature` parsed, this assertion could not
        // be written at all, and a shared search link silently lost the ceiling.
        assert_eq!(
            round.max_rating.as_deref(),
            Some("mature"),
            "a ceiling survives the string: {round:?}"
        );
    }

    #[test]
    fn every_expressible_filter_is_a_removable_chip() {
        let chips = state().chips();
        assert_eq!(chips.len(), 4, "three filters and the ordering: {chips:?}");
        let expressible: Vec<&Chip> = chips.iter().filter(|chip| chip.editable_in_query).collect();
        assert_eq!(
            expressible.len(),
            3,
            "the word window and the rating ceiling — sort is panel state: {chips:?}"
        );
        for chip in expressible {
            assert!(chip.removable, "{chip:?}");
            assert_eq!(chip.reason, None, "{chip:?}");
        }
        assert_eq!(chips[0].label, "1000+ words");
        let sort = chips.last().expect("sort chip");
        assert_eq!(sort.kind, "sort", "{sort:?}");
        assert!(!sort.editable_in_query && sort.removable, "{sort:?}");
        assert!(
            sort.reason.is_some(),
            "it says why it is not in the string: {sort:?}"
        );
    }

    #[test]
    fn a_filter_with_no_field_is_a_pinned_chip_and_says_why() {
        let mut with_pinned = state();
        with_pinned.pinned = vec![
            PinnedFilter {
                kind: "warning".to_owned(),
                value: "graphic violence".to_owned(),
                label: "filtering graphic violence".to_owned(),
                reason: "the query grammar has no field for this filter",
            },
            PinnedFilter {
                kind: "provenance".to_owned(),
                value: "hosted".to_owned(),
                label: "hosted works only".to_owned(),
                reason: "the query grammar has no field for provenance",
            },
        ];
        let chips = with_pinned.chips();
        let pinned: Vec<&Chip> = chips
            .iter()
            .filter(|chip| !chip.editable_in_query && chip.kind != "sort")
            .collect();
        assert_eq!(pinned.len(), 2, "{chips:?}");
        for chip in pinned {
            assert!(
                !chip.removable,
                "a chip the string cannot express must not be droppable by editing it: {chip:?}"
            );
            assert!(chip.reason.is_some(), "{chip:?}");
        }
        // Removing every token from the string leaves the pinned filters in the state, which is the property the row is about.
        let edited = FilterState::from_query_string("");
        assert!(
            edited.pinned.is_empty(),
            "the string cannot carry them, by definition"
        );
        assert_eq!(
            with_pinned.pinned.len(),
            2,
            "the state keeps them while the string is edited"
        );

        let note = with_pinned.unexpressed_note().expect("a note");
        assert!(
            note.contains("2 of this search's filters are not written in the query")
                && note.contains("filtering graphic violence")
                && note.contains("hosted works only"),
            "the note names them: {note}"
        );
        assert_eq!(state().unexpressed_note(), None, "nothing to apologise for");
    }

    #[test]
    fn a_half_typed_string_is_kept_as_text_rather_than_replaced_by_nothing() {
        let state = FilterState::from_query_string("words:>=");
        assert_eq!(
            state.text, "words:>=",
            "an unparseable string is the reader's state, not the empty state"
        );
    }
}

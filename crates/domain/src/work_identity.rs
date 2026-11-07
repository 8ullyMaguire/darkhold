//! M69-04 — what makes two copies "the same work", so a takedown cannot be outrun by re-importing from another host.
//!
//! # Why not the URL
//!
//! A takedown keyed by URL stops exactly one import. The same work is served by the original archive, by three mirrors, by
//! a bundle export, and by whatever scraper comes next; the author's complaint is about *their story*, not about a string.
//! So the key is derived from what the work is: its title, its author, and its language.
//!
//! # Why not the text
//!
//! A body digest would be the strongest key available — and the weakest thing to rely on. Imports arrive without bodies (a
//! preview, a metadata-only fetch), a body legitimately changes (the author edits a chapter), and two mirrors of one story
//! rarely carry byte-identical text once whitespace and encoding are involved. A tombstone that only matches the exact bytes
//! would miss the case a takedown exists to stop. The honest hierarchy is: title, author, language now — and a body digest as
//! a *later* second key for the same tombstone, which is why [`key`] and [`text_key`] are separate functions rather than one.
//!
//! # What normalisation removes, and what it deliberately does not
//!
//! Case, punctuation, spacing, and combining marks (a reader copying `Á` and an importer emitting `A` are the same letter).
//! It does **not** remove words, reorder them, or strip suffixes: "The Storm, part 2" and "The Storm" are different works,
//! and a key that fuzzy-matched them would take down a sequel nobody complained about. The cost is stated plainly in the
//! ledger: a retitled copy escapes. That is the right failure — an under-matching tombstone asks an operator to file a second
//! one; an over-matching one removes somebody's work because their title resembles a complaint.
//!
//! Author is compared the same way, and an empty author is **not** a wildcard: `("The Storm", "")` and `("The Storm", "ada")`
//! are different keys. Treating a missing author as "any author" would tombstone every work with that title, which is exactly
//! the over-match this module refuses everywhere else.

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// Normalise a piece of text for identity: lowercase, letters and digits only, single spaces between words.
///
/// Combining marks are dropped *after* lowercasing, so `Á` and `A` meet, and non-Latin scripts survive untouched (the
/// alphanumeric test is Unicode-aware, which is what makes this usable outside English-language fandoms).
#[must_use]
pub fn normalise(text: &str) -> String {
    // Decompose first (NFKD), then lowercase, then drop the marks: `Á` written as one code point and as `A` + `´` both
    // reduce to `a`. Dropping marks without decomposing was the first draft of this function, and it split the two spellings
    // of the same name -- which is the whole failure a takedown cannot afford, because the importer emits whichever spelling
    // the source served.
    let lowered: String = text.nfkd().collect::<String>().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut previous_space = true;
    for character in lowered.chars() {
        // Combining marks: drop, so a decomposed `a` + `´` matches a composed `á`.
        if matches!(
            character,
            '\u{0300}'..='\u{036F}' | '\u{1AB0}'..='\u{1AFF}' | '\u{20D0}'..='\u{20FF}' | '\u{FE20}'..='\u{FE2F}'
        ) {
            continue;
        }
        if character.is_alphanumeric() {
            out.push(character);
            previous_space = false;
        } else if !previous_space {
            out.push(' ');
            previous_space = true;
        }
    }
    out.trim_end().to_owned()
}

/// The identity key for a work, or `None` when there is nothing to key on.
///
/// `None` rather than a hash of the empty string: a work with no title is not "the work whose title is empty", and a
/// tombstone that matched every untitled import would be a tombstone that takes the instance down.
#[must_use]
pub fn key(title: &str, author: &str, language: Option<&str>) -> Option<String> {
    let title = normalise(title);
    if title.is_empty() {
        return None;
    }
    let author = normalise(author);
    let language = language.map(normalise).unwrap_or_default();
    let mut hasher = Sha256::new();
    // A separator that cannot appear in the normalised parts (`\u{1f}` is stripped by `normalise`), so `("a b", "c")` and
    // `("a", "b c")` cannot collide into the same key.
    hasher.update(title.as_bytes());
    hasher.update([0x1f]);
    hasher.update(author.as_bytes());
    hasher.update([0x1f]);
    hasher.update(language.as_bytes());
    Some(hex::encode(hasher.finalize())[..32].to_owned())
}

/// The key of a body text, for the second half of a tombstone that has one.
///
/// The whole body, hashed after normalisation: no windowing, because a window invites the question "which window", and the
/// answer would differ between two implementations of the same idea.
#[must_use]
pub fn text_key(body: &str) -> Option<String> {
    let normalised = normalise(body);
    if normalised.is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(normalised.as_bytes());
    Some(hex::encode(hasher.finalize())[..32].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_work_from_another_host_has_the_same_key() {
        let original = key("The Storm", "ada", Some("en")).expect("key");
        let mirror = key("  the   STORM! ", "Ada", Some("EN")).expect("key");
        assert_eq!(original, mirror, "case, punctuation and spacing are not identity");
    }

    #[test]
    fn a_composed_and_a_decomposed_accent_are_the_same_letter() {
        // `Á` as one code point, and `A` + combining acute.
        let composed = key("Ámbar", "ada", None).expect("key");
        let decomposed = key("A\u{0301}mbar", "ada", None).expect("key");
        assert_eq!(composed, decomposed);
    }

    #[test]
    fn a_different_author_or_language_is_a_different_work() {
        let base = key("The Storm", "ada", Some("en"));
        assert_ne!(base, key("The Storm", "bea", Some("en")));
        assert_ne!(base, key("The Storm", "ada", Some("fr")));
        assert_ne!(
            base,
            key("The Storm", "ada", None),
            "an unknown language is not the same claim as a stated one"
        );
    }

    #[test]
    fn an_absent_author_is_not_a_wildcard() {
        let untitled_author = key("The Storm", "", None).expect("key");
        let named = key("The Storm", "ada", None).expect("key");
        assert_ne!(
            untitled_author, named,
            "an empty author must not match every other work with that title"
        );
    }

    #[test]
    fn a_sequel_is_a_different_work() {
        // The normalisation strips punctuation, and `part 2` is a word: this pair must stay distinct or a takedown of the
        // first removes the second.
        assert_ne!(
            key("The Storm, part 2", "ada", None),
            key("The Storm", "ada", None)
        );
    }

    #[test]
    fn nothing_to_key_on_is_none() {
        assert_eq!(key("", "ada", None), None);
        assert_eq!(key("   !!!  ", "ada", None), None);
        assert_eq!(text_key(" \n\t "), None);
    }

    #[test]
    fn the_separator_cannot_be_reintroduced_by_the_parts() {
        // `normalise` turns the unit separator into a space, so these two tuples cannot collide.
        assert_ne!(key("a b", "c", None), key("a", "b c", None));
    }

    #[test]
    fn non_latin_titles_normalise_to_something_usable() {
        let japanese = key("嵐の夜", "ada", None).expect("key");
        assert_eq!(japanese, key("嵐の夜", "ada", None).expect("key"));
        assert_ne!(japanese, key("嵐の朝", "ada", None).expect("key"));
    }

    #[test]
    fn a_text_key_ignores_formatting_and_not_words() {
        let first = text_key("It was a dark and stormy night.").expect("key");
        let formatted = text_key("it was a DARK and stormy   night\n").expect("key");
        assert_eq!(first, formatted);
        assert_ne!(first, text_key("It was a bright and stormy night.").expect("key"));
    }
}

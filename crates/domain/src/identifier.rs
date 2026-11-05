//! M45-37 — stable identifiers for media: **identity over location**.
//!
//! A URL is where a thing was; an identifier is which thing it is. Two catalogue rows that carry the same ISBN
//! are the same book however many times it was imported from however many sites, and a track's ISRC survives
//! every re-upload of the file it lives in.
//!
//! # What this module owns, and what it refuses
//!
//! * **Canonical form, not raw input.** `978-0-306-40615-7`, `9780306406157` and `978 0 306 40615 7` are one
//!   identifier, and the store keys on the canonical string. Normalising on the way in is the difference
//!   between "the same book" and "eleven copies of the same book".
//! * **Check digits are checked.** An ISBN that fails its own checksum is refused rather than stored: an
//!   identifier that is wrong is worse than no identifier, because it will match something else.
//! * **An ambiguous identifier is refused.** A bare TMDB number does not say whether it is a film or a series,
//!   and guessing produces a link that quietly points at the wrong page. `movie:603` and `tv:1399` are the
//!   accepted shapes; `603` is a refusal with that sentence.
//! * **No fifth scheme by accident.** The five are named in [`Scheme`], and the store's CHECK constraint repeats
//!   them, so a caller cannot invent one by writing it into a row.

use std::fmt;

/// The identifier schemes this build knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scheme {
    /// MusicBrainz release-group MBID — a UUID.
    Musicbrainz,
    /// International Standard Recording Code — 12 characters.
    Isrc,
    /// International Standard Book Number — 10 or 13 digits, with a check digit.
    Isbn,
    /// The Movie Database identifier, which is only meaningful with its kind.
    Tmdb,
    /// Wikidata item, `Q` followed by digits.
    Wikidata,
}

impl Scheme {
    /// The wire word, which is also the column's CHECK constraint.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Musicbrainz => "musicbrainz",
            Self::Isrc => "isrc",
            Self::Isbn => "isbn",
            Self::Tmdb => "tmdb",
            Self::Wikidata => "wikidata",
        }
    }

    /// Parse a wire word.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "musicbrainz" | "mbid" => Some(Self::Musicbrainz),
            "isrc" => Some(Self::Isrc),
            "isbn" | "isbn10" | "isbn13" => Some(Self::Isbn),
            "tmdb" => Some(Self::Tmdb),
            "wikidata" | "wd" => Some(Self::Wikidata),
            _ => None,
        }
    }

    /// Every scheme, in the order an interface lists them.
    #[must_use]
    pub fn all() -> [Self; 5] {
        [
            Self::Isbn,
            Self::Tmdb,
            Self::Musicbrainz,
            Self::Isrc,
            Self::Wikidata,
        ]
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A canonical identifier, or the reason the input is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identified {
    /// Canonical form, ready to store and compare.
    Canonical(String),
    /// Refused, with a sentence naming what is wrong.
    Refused(String),
}

impl Identified {
    /// The canonical value, when there is one.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Canonical(value) => Some(value),
            Self::Refused(_) => None,
        }
    }

    /// The refusal, when there is one.
    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        match self {
            Self::Canonical(_) => None,
            Self::Refused(reason) => Some(reason),
        }
    }
}

/// Canonicalise `raw` as `scheme`.
///
/// # Examples
///
/// ```
/// use darkhold_domain::identifier::{canonical, Identified, Scheme};
/// let parsed = canonical(Scheme::Isbn, "978-0-306-40615-7");
/// assert_eq!(parsed.value(), Some("9780306406157"));
/// ```
#[must_use]
pub fn canonical(scheme: Scheme, raw: &str) -> Identified {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Identified::Refused("an empty identifier is not an identifier".to_owned());
    }
    match scheme {
        Scheme::Isbn => isbn(trimmed),
        Scheme::Isrc => isrc(trimmed),
        Scheme::Musicbrainz => mbid(trimmed),
        Scheme::Tmdb => tmdb(trimmed),
        Scheme::Wikidata => wikidata(trimmed),
    }
}

fn digits_only(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-')
        .collect()
}

fn isbn(raw: &str) -> Identified {
    let cleaned = digits_only(raw);
    match cleaned.len() {
        10 => {
            let body = &cleaned[..9];
            if !body.chars().all(|ch| ch.is_ascii_digit()) {
                return Identified::Refused(format!(
                    "{raw} is not an ISBN: the first nine characters must be digits"
                ));
            }
            let Some(check) = cleaned.chars().nth(9) else {
                return Identified::Refused("an empty identifier is not an identifier".to_owned());
            };
            let check_value = match check {
                'X' | 'x' => 10,
                ch if ch.is_ascii_digit() => ch.to_digit(10).unwrap_or(0),
                _ => {
                    return Identified::Refused(format!(
                        "{raw} is not an ISBN: the check character must be a digit or X"
                    ))
                }
            };
            let sum: u32 = body
                .chars()
                .enumerate()
                .map(|(index, ch)| {
                    let value = ch.to_digit(10).unwrap_or(0);
                    (10 - index as u32) * value
                })
                .sum::<u32>()
                + check_value;
            if sum % 11 != 0 {
                return Identified::Refused(format!(
                    "{raw} fails its ISBN-10 check digit — an identifier that is wrong will match the wrong book"
                ));
            }
            Identified::Canonical(cleaned.to_uppercase())
        }
        13 => {
            if !cleaned.chars().all(|ch| ch.is_ascii_digit()) {
                return Identified::Refused(format!(
                    "{raw} is not an ISBN-13: every character must be a digit"
                ));
            }
            let sum: u32 = cleaned
                .chars()
                .enumerate()
                .map(|(index, ch)| {
                    let value = ch.to_digit(10).unwrap_or(0);
                    // Weights alternate 1, 3 from the left; the last digit is the check digit and is added
                    // unweighted, which is the same as folding it in at weight 1.
                    match index % 2 {
                        0 => value,
                        _ => value * 3,
                    }
                })
                .sum();
            if sum % 10 != 0 {
                return Identified::Refused(format!(
                    "{raw} fails its ISBN-13 check digit — an identifier that is wrong will match the wrong book"
                ));
            }
            Identified::Canonical(cleaned)
        }
        _ => Identified::Refused(format!(
            "{raw} is not an ISBN: 10 or 13 digits after removing hyphens, found {}",
            cleaned.len()
        )),
    }
}

fn isrc(raw: &str) -> Identified {
    let cleaned: String = raw
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-')
        .collect::<String>()
        .to_uppercase();
    if cleaned.len() != 12 {
        return Identified::Refused(format!(
            "{raw} is not an ISRC: 12 characters (country, registrant, year, designation), found {}",
            cleaned.len()
        ));
    }
    let bytes = cleaned.as_bytes();
    let country_ok = bytes[..2].iter().all(u8::is_ascii_alphabetic);
    let registrant_ok = bytes[2..5].iter().all(u8::is_ascii_alphanumeric);
    let serial_ok = bytes[5..].iter().all(u8::is_ascii_digit);
    if !(country_ok && registrant_ok && serial_ok) {
        return Identified::Refused(format!(
            "{raw} is not an ISRC: two letters, three alphanumerics, then seven digits"
        ));
    }
    Identified::Canonical(cleaned)
}

fn mbid(raw: &str) -> Identified {
    match uuid::Uuid::parse_str(raw.trim()) {
        // Lowercase, hyphenated: the form MusicBrainz itself prints, so two spellings of the same id are one
        // key rather than two rows that never compare equal.
        Ok(parsed) => Identified::Canonical(parsed.hyphenated().to_string()),
        Err(_) => Identified::Refused(format!(
            "{raw} is not a MusicBrainz identifier: a release group is named by a UUID"
        )),
    }
}

fn tmdb(raw: &str) -> Identified {
    let trimmed = raw.trim().to_ascii_lowercase();
    let Some((kind, number)) = trimmed.split_once(':') else {
        return Identified::Refused(format!(
            "{raw} does not say whether it is a film or a series — write movie:{raw} or tv:{raw}"
        ));
    };
    let kind = kind.trim();
    if kind != "movie" && kind != "tv" {
        return Identified::Refused(format!("{raw}: the kind must be movie or tv, not {kind}"));
    }
    let number = number.trim();
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        return Identified::Refused(format!(
            "{raw}: the identifier after {kind}: must be a number"
        ));
    }
    // TMDB ids are positive integers; a leading zero is a different string for the same page, so it is
    // normalised away rather than stored.
    let normalised = number.trim_start_matches('0');
    if normalised.is_empty() {
        return Identified::Refused(format!("{raw}: TMDB identifiers start at 1"));
    }
    Identified::Canonical(format!("{kind}:{normalised}"))
}

fn wikidata(raw: &str) -> Identified {
    let trimmed = raw.trim().to_uppercase();
    let mut chars = trimmed.chars();
    if chars.next() != Some('Q') || !chars.all(|ch| ch.is_ascii_digit()) || trimmed.len() < 2 {
        return Identified::Refused(format!(
            "{raw} is not a Wikidata item: Q followed by digits, e.g. Q42"
        ));
    }
    Identified::Canonical(trimmed)
}

/// Whether two (scheme, value) pairs name the same thing — the question the whole row exists to answer.
#[must_use]
pub fn same(scheme: Scheme, left: &str, right: &str) -> bool {
    match (canonical(scheme, left), canonical(scheme, right)) {
        (Identified::Canonical(a), Identified::Canonical(b)) => a == b,
        _ => false,
    }
}

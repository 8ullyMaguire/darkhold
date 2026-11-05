//! The HTTP side of a feed: which format, what `Content-Type`, what `ETag`.
//!
//! # Why this module exists rather than living in the domain crate
//!
//! The estate has one feed renderer — `commons_feed` — and darkhold had three of its own before this:
//! `darkhold_domain::feed` (a careful renderer with a 304 invariant, used by the saved-view feed) and
//! `darkhold_domain::feeds` (`build_rss`/`build_atom`, used by **one test and no production code**). Two
//! implementations of one document, one of them alive only because a test called it.
//!
//! What is left here is not rendering: it is the small set of decisions that belong to *this* product's HTTP
//! surface and that a shared renderer must not have an opinion about — an `ETag` computed from the bytes, the
//! `If-None-Match` comparison, and the format a client names in a query string. `commons_feed` takes no
//! position on caching; that is why it can serve four products with four cache policies.

use axum::http::header::HeaderValue;
use commons_feed::{FeedEntry, FeedMeta};

/// The three shapes a reader can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Atom,
    Rss,
    JsonFeed,
}

impl Format {
    /// `?format=` value, or the extension a reader guesses from.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "atom" | "xml" => Some(Self::Atom),
            "rss" | "rss2" => Some(Self::Rss),
            "json" | "jsonfeed" | "json-feed" => Some(Self::JsonFeed),
            _ => None,
        }
    }

    /// The `Content-Type` a client needs to recognise the document.
    #[must_use]
    pub fn content_type(self) -> &'static str {
        match self {
            // `application/feed+json` is what the JSON Feed spec asks for; the charset is spelled because a
            // feed with an author name in it is text.
            Self::JsonFeed => "application/feed+json; charset=utf-8",
            Self::Atom => "application/atom+xml; charset=utf-8",
            Self::Rss => "application/rss+xml; charset=utf-8",
        }
    }

    /// The file extension a static export would use.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Atom => "atom",
            Self::Rss => "rss",
            Self::JsonFeed => "json",
        }
    }
}

/// Render through the estate's one renderer.
#[must_use]
pub fn render(meta: &FeedMeta, entries: &[FeedEntry], format: Format) -> String {
    match format {
        Format::Atom => commons_feed::atom(meta, entries),
        Format::Rss => commons_feed::rss2(meta, entries),
        Format::JsonFeed => commons_feed::json_feed(meta, entries),
    }
}

/// The feed's strong `ETag`.
///
/// Computed from the rendered bytes, so it changes when *anything* a reader would see changes and does not
/// change when they poll again. Quoted, because `If-None-Match` compares quoted strings.
///
/// Sixteen bytes is enough to make an accidental collision absurd and short enough that the header stays
/// readable in a proxy log.
#[must_use]
pub fn etag(body: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    let digest = hasher.finalize();
    let mut text = String::from("\"");
    for byte in &digest[..16] {
        text.push_str(&format!("{byte:02x}"));
    }
    text.push('"');
    text
}

/// Whether an `If-None-Match` header names this tag.
///
/// Handles a list, the `W/` weak prefix (a reader that sends a weak tag is still telling us it has the
/// bytes), and `*`.
#[must_use]
pub fn matches(header: &str, tag: &str) -> bool {
    let header = header.trim();
    if header == "*" {
        return true;
    }
    header.split(',').any(|candidate| {
        let candidate = candidate.trim();
        let candidate = candidate.strip_prefix("W/").unwrap_or(candidate);
        candidate == tag
    })
}

/// A header value, or nothing at all — a tag that cannot be a header is worse than no tag.
#[must_use]
pub fn header_value(text: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_304_can_be_recognised_in_every_shape_a_reader_sends() {
        let tag = "\"abc\"";
        assert!(matches("\"abc\"", tag));
        assert!(matches("W/\"abc\"", tag));
        assert!(matches("\"zzz\", \"abc\"", tag));
        assert!(matches("*", tag));
        assert!(!matches("\"def\"", tag));
    }

    #[test]
    fn the_tag_is_quoted_and_stable() {
        let one = etag("<rss/>");
        assert_eq!(one, etag("<rss/>"));
        assert!(one.starts_with('"') && one.ends_with('"'));
        assert_ne!(one, etag("<rss />"));
    }
}

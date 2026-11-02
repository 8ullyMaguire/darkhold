//! M45-38 — author-note links rot too, so they go through the same monitor (§32.7, gaps review D3).
//!
//! A link in a note is the same object as a link in an availability list: a URL that answers today
//! and may not tomorrow. The difference is only where it lives. So this module owns the part that is
//! specific to notes — **finding the URLs** and **naming their host** — and nothing else; the status
//! vocabulary is [`crate::media_resilience::LinkStatus`] and the checking is the same pipeline, so
//! there is no second definition of what "dead" means.
//!
//! ## What counting as a link means, and the three decisions behind it
//!
//! 1. **Only `http` and `https`.** `mailto:` cannot rot in the way this monitors, a `javascript:` URL
//!    is an attack rather than a reference, and a bare `example.com` is a guess about the reader's
//!    intent. A note that wants a link writes one.
//! 2. **Trailing punctuation is not part of the URL.** Notes are prose: *"(see https://x.test/meta)."*
//!    yields `https://x.test/meta`, not `…/meta).` — the naive split produces a URL that 404s, and a
//!    monitor that marks it dead is a monitor that lies about a link which works.
//! 3. **A cap, because a note is not a sitemap.** Twenty per note is more than anyone writes in good
//!    faith and bounds what a single save can enqueue. The cap is stated in the refusal rather than
//!    silently applied, so an author with thirty links sees the first twenty and knows why.
//!
//! The scan is deliberately not a regex crate: the pattern is one character class and adding a
//! dependency to this workspace to find `http` in a sentence is not a trade worth making.

use crate::media_resilience::LinkProvider;

/// The most links one note may register, per save.
pub const MAX_LINKS_PER_NOTE: usize = 20;

/// Prefixes a link may start with. Everything else is prose or an attack.
const SCHEMES: [&str; 2] = ["https://", "http://"];

/// Characters that end a URL when they appear at its end, because prose put them there.
///
/// `)` is in the list and is a judgement: a URL may legitimately end in a closing parenthesis —
/// Wikipedia's `/wiki/Foo_(bar)` does — but a note that writes `(https://x.test/meta)` is far more
/// common than a note that pastes a balanced URL inside parentheses. The rule chosen is the one that
/// is right most of the time and legible when wrong: punctuation is stripped, and an author who needs
/// the parenthesis encodes it or leaves the link outside brackets.
const TRAILING_JUNK: [char; 7] = ['.', ',', ';', ':', '!', '?', ')'];

/// Every link in a note, in the order they appear, deduplicated, capped at [`MAX_LINKS_PER_NOTE`].
///
/// Empty for `None`, for an empty note, and for a note that mentions no URL — which is most of them.
#[must_use]
pub fn links_in(note: Option<&str>) -> Vec<String> {
    let Some(text) = note else {
        return Vec::new();
    };
    let lowered = text.to_ascii_lowercase();
    let mut found: Vec<String> = Vec::new();
    let mut from = 0usize;
    while let Some(offset) = next_scheme(&lowered, from) {
        let rest = &text[offset..];
        // A URL ends at whitespace, at a quote, or at an angle bracket — none of which can appear
        // inside one. Everything else is carried and trimmed below.
        let end = rest
            .find(|ch: char| {
                ch.is_whitespace() || ch == '"' || ch == '\'' || ch == '<' || ch == '>'
            })
            .unwrap_or(rest.len());
        let candidate = trim_junk(&rest[..end]);
        if is_navigable(candidate) && !found.iter().any(|seen| seen == candidate) {
            found.push(candidate.to_owned());
        }
        if found.len() >= MAX_LINKS_PER_NOTE {
            break;
        }
        from = offset + end.max(1);
    }
    found
}

/// The offset of the next scheme at or after `from`, matched case-insensitively.
fn next_scheme(lowered: &str, from: usize) -> Option<usize> {
    SCHEMES
        .iter()
        .filter_map(|scheme| lowered[from..].find(scheme).map(|at| from + at))
        .min()
}

/// Drop the punctuation prose left attached to a URL.
fn trim_junk(url: &str) -> &str {
    url.trim_end_matches(TRAILING_JUNK)
}

/// Whether a trimmed candidate is a URL a monitor can actually check.
///
/// A scheme alone (`https://`) has no host: it would be stored, checked, and reported dead, which is
/// a false accusation against a note that wrote a broken link in a way this cannot repair. Refusing
/// it here means the note registers no link and the author's text is untouched.
#[must_use]
pub fn is_navigable(url: &str) -> bool {
    let Some(after) = SCHEMES.iter().find_map(|scheme| url.strip_prefix(scheme)) else {
        return false;
    };
    let host = after.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty() && host.contains('.') && !host.starts_with('.') && !host.ends_with('.')
}

/// The host of a link, without the scheme, port or path. `None` when there is no host.
#[must_use]
pub fn host_of(url: &str) -> Option<&str> {
    let after = SCHEMES.iter().find_map(|scheme| url.strip_prefix(scheme))?;
    let authority = after.split(['/', '?', '#']).next()?;
    if authority.is_empty() {
        return None;
    }
    // A userinfo segment is not part of the host, and a port is not either: the monitor asks the
    // site, not the port, whether the link answers.
    let after_at = authority.rsplit('@').next().unwrap_or(authority);
    let host = after_at.split(':').next().unwrap_or(after_at);
    (!host.is_empty()).then_some(host)
}

/// Which provider a host belongs to, in the existing [`LinkProvider`] vocabulary.
///
/// Suffix matching, not substring: `nottumblr.example` is not Tumblr, and a monitor that files it
/// there would apply Tumblr's rules to a site that has none.
#[must_use]
pub fn provider_of(host: &str) -> LinkProvider {
    let host = host.trim_start_matches("www.");
    let is = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    if is("tumblr.com") {
        LinkProvider::Tumblr
    } else if is("pinterest.com") {
        LinkProvider::Pinterest
    } else if is("imgur.com") {
        LinkProvider::Imgur
    } else if is("archive.org") {
        LinkProvider::InternetArchive
    } else if is("spotify.com") {
        LinkProvider::Spotify
    } else if is("twitter.com") || is("x.com") {
        LinkProvider::Twitter
    } else if is("instagram.com") {
        LinkProvider::Instagram
    } else if host == "localhost" || host.starts_with("127.") || host.starts_with("192.168.") {
        // A local address is a mirror on this instance, which is what `LocalMirror` means (§32.7).
        LinkProvider::LocalMirror
    } else {
        LinkProvider::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_link_is_found() {
        assert_eq!(
            links_in(Some(
                "Thanks to https://example.test/meta for the beta read."
            )),
            vec!["https://example.test/meta"]
        );
    }

    #[test]
    fn prose_punctuation_is_not_part_of_the_url() {
        // The naive split produces `…/meta).` and the monitor then reports a working link dead.
        assert_eq!(
            links_in(Some("(see https://example.test/meta).")),
            vec!["https://example.test/meta"]
        );
        assert_eq!(
            links_in(Some("Source: https://example.test/a,")),
            vec!["https://example.test/a"]
        );
    }

    #[test]
    fn only_http_and_https_count() {
        assert!(links_in(Some("mailto:someone@example.test")).is_empty());
        assert!(links_in(Some("javascript:alert(1)")).is_empty());
        assert!(links_in(Some("ftp://example.test/file")).is_empty());
        // A bare domain is a guess about intent, not a link.
        assert!(links_in(Some("see example.test/meta")).is_empty());
    }

    #[test]
    fn a_scheme_with_no_host_is_refused() {
        assert!(links_in(Some("https:// is not a link")).is_empty());
        assert!(links_in(Some("http://.test/x")).is_empty());
    }

    #[test]
    fn duplicates_collapse_and_order_is_written_order() {
        assert_eq!(
            links_in(Some(
                "https://b.test/2 then https://a.test/1 then https://b.test/2"
            )),
            vec!["https://b.test/2", "https://a.test/1"]
        );
    }

    #[test]
    fn the_cap_is_kept_and_is_twenty() {
        let note: String = (0..30)
            .map(|index| format!("https://link{index}.test/x "))
            .collect();
        let found = links_in(Some(&note));
        assert_eq!(found.len(), 20);
        assert_eq!(found[0], "https://link0.test/x");
    }

    #[test]
    fn host_strips_userinfo_and_port() {
        assert_eq!(
            host_of("https://user:pw@example.test:8080/path"),
            Some("example.test")
        );
        assert_eq!(host_of("https://example.test"), Some("example.test"));
        assert_eq!(host_of("not a url"), None);
    }

    #[test]
    fn provider_matching_is_by_suffix_not_substring() {
        assert_eq!(provider_of("tumblr.com"), LinkProvider::Tumblr);
        assert_eq!(provider_of("blog.tumblr.com"), LinkProvider::Tumblr);
        // `nottumblr.example` ends with "tumblr.example", not ".tumblr.com".
        assert_eq!(provider_of("nottumblr.example"), LinkProvider::Other);
        assert_eq!(provider_of("www.x.com"), LinkProvider::Twitter);
        assert_eq!(provider_of("127.0.0.1"), LinkProvider::LocalMirror);
    }
}

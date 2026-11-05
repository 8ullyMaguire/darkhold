//! M45-67 — reading a pasted line for what it can honestly be.
//!
//! A reader arrives with a list: some lines are works this instance already has, some are URLs to
//! archives it does not read, some are bare titles. This module decides **what a line is**, never
//! whether a work exists: existence is a database question and is answered there. Keeping the
//! classification pure means the refusals are unit-testable without a catalogue, and means the
//! rule "a title is evidence, not a row" is enforced in one place.
//!
//! The three answers are exhaustive on purpose: every line either points at something the instance
//! can resolve, or becomes evidence it can use, or is refused **with a reason a person can act on**
//! ("too long to be a title" tells a reader the line got mangled; "empty" tells them nothing).

/// What a pasted line is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// Nothing but whitespace.
    Blank,
    /// Longer than a title can be: almost always a paste accident or a whole chapter.
    TooLong {
        /// How many characters it carries once trimmed.
        chars: usize,
    },
    /// A pointer this instance can try to resolve: a URL, or a bare title.
    Pointer {
        /// The spec to hand the resolver — the URL, or the title as typed.
        spec: String,
        /// The archive the pointer names, or `unknown` for a bare title.
        source_key: String,
        /// The URL, when the line was one.
        source_url: Option<String>,
    },
}

/// The longest line still treated as a title. A work's title can be long — light novels run to
/// forty words — but a hundred characters past that is a paste, and silently storing it would put a
/// paragraph into a table whose every row claims to be a title.
pub const MAX_TITLE_CHARS: usize = 300;

/// Read one line.
#[must_use]
pub fn classify(line: &str) -> Line {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Line::Blank;
    }
    let chars = trimmed.chars().count();
    if chars > MAX_TITLE_CHARS {
        return Line::TooLong { chars };
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        let host = host_of(&lower).unwrap_or_default();
        return Line::Pointer {
            spec: trimmed.to_owned(),
            source_key: source_for_host(&host).to_owned(),
            source_url: Some(trimmed.to_owned()),
        };
    }
    // A bare AO3 work id (`works/123456` or `123456`) is a pointer too, and a common way people paste
    // them; the resolver decides whether the number is known here.
    if let Some(id) = ao3_id(trimmed) {
        return Line::Pointer {
            spec: id,
            source_key: "ao3".to_owned(),
            source_url: None,
        };
    }
    Line::Pointer {
        spec: trimmed.to_owned(),
        source_key: "unknown".to_owned(),
        source_url: None,
    }
}

/// The host of a URL, without the scheme or the leading `www.`.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.split('@').next_back()?;
    let host = host.split(':').next()?;
    Some(host.trim_start_matches("www.").to_owned())
}

/// The archive a host names. Unknown hosts stay `unknown` rather than being guessed at: a wrong
/// `source_key` is worse than a missing one, because it invites a client to believe a provenance
/// claim the instance cannot support.
fn source_for_host(host: &str) -> &'static str {
    match host {
        h if h.ends_with("archiveofourown.org") => "ao3",
        h if h.ends_with("questionablequesting.com") => "qq",
        h if h.ends_with("spacebattles.com") => "sb",
        h if h.ends_with("sufficientvelocity.com") => "sv",
        h if h.ends_with("webnovel.com") => "wn",
        h if h.ends_with("royalroad.com") => "rr",
        h if h.ends_with("fanfiction.net") => "ffn",
        h if h.ends_with("wattpad.com") => "wattpad",
        h if h.ends_with("scribblehub.com") => "sh",
        _ => "unknown",
    }
}

/// `works/123`, `chapters/123` or a bare number: the AO3 id, when the line carries one.
fn ao3_id(line: &str) -> Option<String> {
    let digits: String = line
        .rsplit(['/', '='])
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty() && digits.len() >= 4).then_some(digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_is_blank_and_a_paragraph_is_refused_with_its_length() {
        assert_eq!(classify("   \n\t "), Line::Blank);
        let long = "a".repeat(MAX_TITLE_CHARS + 1);
        assert_eq!(
            classify(&long),
            Line::TooLong {
                chars: MAX_TITLE_CHARS + 1
            }
        );
        // Exactly at the limit is still a title: the bound is inclusive, so a 300-character light
        // novel title is not refused while a 301-character paste is.
        assert!(matches!(
            classify(&"a".repeat(MAX_TITLE_CHARS)),
            Line::Pointer { .. }
        ));
    }

    #[test]
    fn a_url_keeps_its_host_as_provenance() {
        match classify("https://archiveofourown.org/works/123456/chapters/1") {
            Line::Pointer {
                source_key,
                source_url,
                ..
            } => {
                assert_eq!(source_key, "ao3");
                assert_eq!(
                    source_url.as_deref(),
                    Some("https://archiveofourown.org/works/123456/chapters/1")
                );
            }
            other => panic!("expected a pointer, got {other:?}"),
        }
        match classify("http://www.questionablequesting.com/threads/x.1/") {
            Line::Pointer { source_key, .. } => assert_eq!(source_key, "qq", "www. is stripped"),
            other => panic!("expected a pointer, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_host_is_not_guessed_at() {
        match classify("https://example.invalid/story/9") {
            Line::Pointer { source_key, .. } => assert_eq!(
                source_key, "unknown",
                "a wrong source is worse than a missing one: it invites belief in provenance \
                 the instance cannot support"
            ),
            other => panic!("expected a pointer, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_ao3_id_is_a_pointer_and_a_bare_title_is_unknown_but_usable() {
        match classify("works/123456") {
            Line::Pointer {
                spec,
                source_key,
                source_url,
            } => {
                assert_eq!(spec, "123456");
                assert_eq!(source_key, "ao3");
                assert_eq!(source_url, None);
            }
            other => panic!("expected a pointer, got {other:?}"),
        }
        match classify("  A Gamer's Guide to Necromancy  ") {
            Line::Pointer {
                spec, source_key, ..
            } => {
                assert_eq!(spec, "A Gamer's Guide to Necromancy", "trimmed as typed");
                assert_eq!(source_key, "unknown");
            }
            other => panic!("expected a pointer, got {other:?}"),
        }
        // A short number is a title, not an id: `1984` is a work, and a four-digit floor keeps
        // chapter numbers and years out of the id path.
        assert!(matches!(classify("198"), Line::Pointer { .. }));
    }
}

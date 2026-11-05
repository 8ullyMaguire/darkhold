//! M70-06 — reader skins: the rules a stylesheet must satisfy before it is stored.
//!
//! A skin is the one place a reader supplies markup-shaped input that is then served as CSS to their own browser.
//! That makes it the only XSS-adjacent surface a reader controls, and the rules below are the whole defence.
//!
//! # What a skin may not do, and why each one is a real attack
//!
//! * **`@import`** pulls a stylesheet from another origin. The imported sheet is arbitrary CSS the instance never
//!   reviewed, and it can carry `url()` references of its own — so allowing `@import` would make every other rule
//!   here advisory.
//! * **`url()` to anything but a `data:` URI** is both an exfiltration channel and a tracking pixel: the browser
//!   fetches it with the reader's IP, at the moment the reader opens a page, and the skin's author learns when and
//!   from where. `data:` is allowed because it is self-contained — the bytes travel with the skin.
//! * **`javascript:`**, **`expression()`**, **`behavior:`** and **`-moz-binding`** are the historical code paths in
//!   stylesheets. Modern browsers refuse most of them, which is exactly why they must be refused *here*: a rule
//!   that only holds on current browsers is a rule that silently weakens.
//! * **`</style`** matters even though the skin is served as its own document: the moment any surface inlines a
//!   reader's skin into a page — a preview, an export, a future "share your theme" page — that sequence ends the
//!   style element and the rest becomes markup.
//!
//! # What is not here
//!
//! No parsing. A CSS parser would be larger than the feature and would still need these decisions; a scanner that
//! refuses the known-bad constructs, in a document that is also size-capped, is the smaller promise — and the
//! sizes below are what keeps the promise true.

/// The largest skin accepted, in bytes. Bigger than any hand-written skin and small enough that a page cannot be
/// shipped one.
pub const MAX_BYTES: usize = 64 * 1024;

/// Why a skin was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Over [`MAX_BYTES`].
    TooLarge {
        /// Length supplied.
        bytes: usize,
    },
    /// The document is empty or whitespace.
    Empty,
    /// Contains an `@import`.
    Import,
    /// Contains a `url()` that is not a `data:` URI.
    RemoteUrl,
    /// Contains a construct that executes code in a stylesheet.
    Scriptish {
        /// The construct found, so the reader can be told what to remove.
        construct: &'static str,
    },
    /// Contains `</style`, which would end the element the skin is served in.
    StyleClose,
}

impl Refusal {
    /// A sentence a reader can act on.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::TooLarge { bytes } => format!(
                "this skin is {bytes} bytes; the limit is {MAX_BYTES} (a stylesheet, not an archive)"
            ),
            Self::Empty => "this skin is empty".to_owned(),
            Self::Import => "remove the @import: a skin may not load another stylesheet".to_owned(),
            Self::RemoteUrl => {
                "remove the url(...): a skin may not fetch anything from another origin (data: URIs are allowed)"
                    .to_owned()
            }
            Self::Scriptish { construct } => format!("remove the {construct}: a stylesheet may not run code"),
            Self::StyleClose => "remove the </style sequence".to_owned(),
        }
    }
}

/// A skin that passed the rules, as the text to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skin(String);

impl Skin {
    /// The stylesheet, exactly as it will be served.
    #[must_use]
    pub fn css(&self) -> &str {
        &self.0
    }
}

/// Check a skin, returning it ready to serve or the first refusal.
///
/// The first refusal, not all of them: a reader fixes one thing at a time, and a list of nine problems for a
/// 60 KB paste is noise rather than help.
pub fn check(css: &str) -> Result<Skin, Refusal> {
    if css.trim().is_empty() {
        return Err(Refusal::Empty);
    }
    if css.len() > MAX_BYTES {
        return Err(Refusal::TooLarge { bytes: css.len() });
    }
    // Lowercased once, scanned as text: CSS keywords are case-insensitive, so `URL(` must be refused exactly as
    // `url(` is, and a scanner that only looked for the lowercase spelling would be a scanner with a bypass.
    let haystack = css.to_ascii_lowercase();

    if haystack.contains("@import") {
        return Err(Refusal::Import);
    }
    if haystack.contains("</style") {
        return Err(Refusal::StyleClose);
    }
    for (needle, construct) in [
        ("javascript:", "javascript: URL"),
        ("expression(", "expression()"),
        ("behavior:", "behavior: property"),
        ("-moz-binding", "-moz-binding property"),
        ("vbscript:", "vbscript: URL"),
    ] {
        if haystack.contains(needle) {
            return Err(Refusal::Scriptish { construct });
        }
    }
    // Every `url(` must close with a `data:` payload. `url()` may be quoted or not, and may carry whitespace
    // before the quote, so the check looks at what follows the opening parenthesis rather than trusting a form.
    let mut rest = haystack.as_str();
    while let Some(position) = rest.find("url(") {
        let after = &rest[position + 4..];
        let payload = after
            .trim_start()
            .trim_start_matches(['"', '\''])
            .trim_start();
        if !payload.starts_with("data:") {
            return Err(Refusal::RemoteUrl);
        }
        rest = after;
    }

    Ok(Skin(css.to_owned()))
}

//! M70-09 — the extension document: what one reader can hand to another.
//!
//! A document describes an arrangement of things the product already has — which views are saved, which widgets
//! occupy which slots, which colours the interface uses. It is deliberately not a program: no conditions, no
//! callbacks, nothing the server evaluates on an author's behalf. That is what makes it safe to import a stranger's
//! document, and it is the whole reason this module can exist without an executor (ADR 0005, M16-06).
//!
//! # Three rules, each of which buys something specific
//!
//! * **An unknown field is refused.** Not ignored. `Manifest` learned this for manifests (§21.1): a reader is about
//!   to accept a document they did not write, so the document must mean exactly what it says. If a newer instance
//!   writes a field this one does not understand, refusing is honest — silently dropping it produces two instances
//!   holding "the same" document that arrange the interface differently.
//! * **Import runs the same validator as local authoring.** Otherwise a shared document becomes a way to reach a
//!   state the API refuses, and the validator's guarantees hold only for people who typed their own.
//! * **The body is a closed vocabulary, not free-form JSON.** `filters` is scalars, `widget` is a known name, a
//!   token value is a colour-or-length with no CSS break-out — because each of those is a place where "just JSON"
//!   would hand an importer a way to express something this product does not do.

use std::collections::BTreeSet;
use std::fmt;

use serde_json::{Map, Value};

/// The only document version this instance writes or accepts.
pub const VERSION: u32 = 1;

/// The largest document accepted, in bytes. A layout is smaller than a novel.
pub const MAX_BYTES: usize = 32 * 1024;

/// Upper bounds on what a document may contain.
pub mod limits {
    /// Saved views in one bundle.
    pub const VIEWS: usize = 20;
    /// Characters in a view's name.
    pub const VIEW_NAME: usize = 80;
    /// Slots in one layout.
    pub const SLOTS: usize = 12;
    /// Design tokens in one theme.
    pub const TOKENS: usize = 64;
    /// Characters in a token value.
    pub const TOKEN_VALUE: usize = 64;
    /// Characters in a document's name.
    pub const NAME: usize = 80;
}

/// The closed set of widgets a layout slot may name.
///
/// A layout is an arrangement of things the product has. If the list were open, a document could name a widget
/// that does not exist and the interface would render a hole — which is a failure mode that looks like a bug in
/// this product rather than a bug in the document.
pub const WIDGETS: [&str; 8] = [
    "recommended",
    "continue_reading",
    "recent_updates",
    "bookmarks",
    "marked_for_later",
    "inbox",
    "trends",
    "announcements",
];

/// What kind of arrangement a document describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A bundle of saved views.
    SavedViews,
    /// A dashboard arrangement.
    Layout,
    /// A set of design tokens.
    ThemeTokens,
}

impl Kind {
    /// The slug stored in the document and shown in a browse list.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SavedViews => "saved_views",
            Self::Layout => "layout",
            Self::ThemeTokens => "theme_tokens",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "saved_views" => Some(Self::SavedViews),
            "layout" => Some(Self::Layout),
            "theme_tokens" => Some(Self::ThemeTokens),
            _ => None,
        }
    }
}

/// Why a document was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Larger than [`MAX_BYTES`].
    TooLarge {
        /// Length supplied.
        bytes: usize,
    },
    /// Not JSON.
    BadJson {
        /// Parser complaint, which is the most useful thing to show an author.
        detail: String,
    },
    /// JSON, but not an object.
    NotAnObject,
    /// The object has no such key.
    Missing {
        /// Key.
        field: &'static str,
    },
    /// The object has a key this version does not define.
    Unknown {
        /// Key, owned because it came from the document.
        field: String,
    },
    /// A `version` this instance cannot read.
    UnsupportedVersion {
        /// Version supplied.
        version: u64,
    },
    /// A `kind` this instance does not define.
    UnknownKind {
        /// Kind supplied.
        kind: String,
    },
    /// A name that is empty, too long, or not a string.
    BadName,
    /// The body does not satisfy its kind's rules.
    BadBody {
        /// What is wrong, in words an author can act on.
        reason: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes } => write!(f, "document is {bytes} bytes; the limit is {MAX_BYTES}"),
            Self::BadJson { detail } => write!(f, "not valid JSON: {detail}"),
            Self::NotAnObject => write!(f, "a document is a JSON object"),
            Self::Missing { field } => write!(f, "missing field `{field}`"),
            Self::Unknown { field } => write!(f, "unknown field `{field}` (this instance refuses fields it does not understand rather than ignoring them)"),
            Self::UnsupportedVersion { version } => {
                write!(f, "version {version} is not supported; this instance writes and reads version {VERSION}")
            }
            Self::UnknownKind { kind } => write!(f, "unknown kind `{kind}`"),
            Self::BadName => write!(f, "name must be 1..={} characters", limits::NAME),
            Self::BadBody { reason } => write!(f, "{reason}"),
        }
    }
}

/// A validated document, ready to store, export or render.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// What it arranges.
    pub kind: Kind,
    /// The reader's name for it.
    pub name: String,
    /// The body, already checked against its kind.
    pub body: Value,
}

impl Document {
    /// The canonical export: the bytes one reader hands to another.
    ///
    /// `serde_json`'s maps are ordered, so the same document exports byte-identically wherever it is written —
    /// which is what makes "the document they sent" and "the document I imported" comparable.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut root = Map::new();
        root.insert("version".to_owned(), Value::from(VERSION));
        root.insert("kind".to_owned(), Value::from(self.kind.as_str()));
        root.insert("name".to_owned(), Value::from(self.name.clone()));
        root.insert("body".to_owned(), self.body.clone());
        Value::Object(root).to_string()
    }
}

const TOP_LEVEL: [&str; 4] = ["version", "kind", "name", "body"];

/// Validate a document.
pub fn parse(raw: &str) -> Result<Document, Refusal> {
    if raw.len() > MAX_BYTES {
        return Err(Refusal::TooLarge { bytes: raw.len() });
    }
    let value: Value = serde_json::from_str(raw).map_err(|error| Refusal::BadJson {
        detail: error.to_string(),
    })?;
    let Value::Object(map) = value else {
        return Err(Refusal::NotAnObject);
    };

    for key in map.keys() {
        if !TOP_LEVEL.contains(&key.as_str()) {
            return Err(Refusal::Unknown { field: key.clone() });
        }
    }

    let version = map
        .get("version")
        .ok_or(Refusal::Missing { field: "version" })?
        .as_u64()
        .ok_or(Refusal::UnsupportedVersion { version: 0 })?;
    if version != u64::from(VERSION) {
        return Err(Refusal::UnsupportedVersion { version });
    }

    let kind_raw = map
        .get("kind")
        .ok_or(Refusal::Missing { field: "kind" })?
        .as_str()
        .ok_or_else(|| Refusal::UnknownKind {
            kind: String::new(),
        })?;
    let kind = Kind::parse(kind_raw).ok_or_else(|| Refusal::UnknownKind {
        kind: kind_raw.to_owned(),
    })?;

    let name = map
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= limits::NAME)
        .ok_or(Refusal::BadName)?
        .to_owned();

    let body = map.get("body").ok_or(Refusal::Missing { field: "body" })?;
    check_body(kind, body)?;

    Ok(Document {
        kind,
        name,
        body: body.clone(),
    })
}

fn check_body(kind: Kind, body: &Value) -> Result<(), Refusal> {
    let Value::Object(map) = body else {
        return Err(bad("body must be an object"));
    };
    match kind {
        Kind::SavedViews => {
            let views = array(map, "views", "body")?;
            if views.is_empty() || views.len() > limits::VIEWS {
                return Err(bad(format!(
                    "a bundle holds 1..={} views, this one holds {}",
                    limits::VIEWS,
                    views.len()
                )));
            }
            for view in views {
                check_view(view)?;
            }
            Ok(())
        }
        Kind::Layout => {
            let slots = array(map, "slots", "body")?;
            if slots.is_empty() || slots.len() > limits::SLOTS {
                return Err(bad(format!(
                    "a layout holds 1..={} slots, this one holds {}",
                    limits::SLOTS,
                    slots.len()
                )));
            }
            for slot in slots {
                check_slot(slot)?;
            }
            Ok(())
        }
        Kind::ThemeTokens => {
            let tokens = map
                .get("tokens")
                .ok_or(Refusal::Missing { field: "tokens" })?;
            let Value::Object(tokens) = tokens else {
                return Err(bad("`tokens` must be an object of token name to value"));
            };
            if tokens.is_empty() || tokens.len() > limits::TOKENS {
                return Err(bad(format!(
                    "a theme holds 1..={} tokens, this one holds {}",
                    limits::TOKENS,
                    tokens.len()
                )));
            }
            for (name, value) in tokens {
                check_token(name, value)?;
            }
            Ok(())
        }
    }
}

fn bad(reason: impl Into<String>) -> Refusal {
    Refusal::BadBody {
        reason: reason.into(),
    }
}

fn array<'a>(
    map: &'a Map<String, Value>,
    key: &'static str,
    owner: &str,
) -> Result<&'a Vec<Value>, Refusal> {
    match map.get(key) {
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(bad(format!("`{key}` in {owner} must be an array"))),
        None => Err(Refusal::Missing { field: key }),
    }
}

fn check_view(view: &Value) -> Result<(), Refusal> {
    let Value::Object(view) = view else {
        return Err(bad("each view must be an object"));
    };
    for key in view.keys() {
        if key != "name" && key != "filters" {
            return Err(bad(format!("a view has `name` and `filters`, not `{key}`")));
        }
    }
    let name = view
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= limits::VIEW_NAME)
        .ok_or_else(|| {
            bad(format!(
                "a view name must be 1..={} characters",
                limits::VIEW_NAME
            ))
        })?;
    let _ = name;
    let Value::Object(filters) = view
        .get("filters")
        .ok_or_else(|| bad("a view needs `filters`"))?
    else {
        return Err(bad("`filters` must be an object"));
    };
    if filters.len() > 16 {
        return Err(bad("a view may set at most 16 filters"));
    }
    for (key, value) in filters {
        if !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            || key.is_empty()
            || key.len() > 32
        {
            return Err(bad(format!("`{key}` is not a filter name")));
        }
        // A bundle is an arrangement of filters the product has, not a query language: a nested object here would
        // be a way to express a condition this product does not offer.
        if !(value.is_string() || value.is_number() || value.is_boolean()) {
            return Err(bad(format!("`{key}` must be a string, number or boolean")));
        }
    }
    Ok(())
}

fn check_slot(slot: &Value) -> Result<(), Refusal> {
    let Value::Object(slot) = slot else {
        return Err(bad("each slot must be an object"));
    };
    for key in slot.keys() {
        if key != "widget" && key != "span" {
            return Err(bad(format!("a slot has `widget` and `span`, not `{key}`")));
        }
    }
    let widget = slot
        .get("widget")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("a slot needs `widget`"))?;
    let known: BTreeSet<&str> = WIDGETS.into_iter().collect();
    if !known.contains(widget) {
        return Err(bad(format!(
            "`{widget}` is not a widget this instance has (known: {})",
            WIDGETS.join(", ")
        )));
    }
    if let Some(span) = slot.get("span") {
        let Some(span) = span.as_u64() else {
            return Err(bad("`span` must be a number"));
        };
        if !(1..=12).contains(&span) {
            return Err(bad("`span` must be 1..=12"));
        }
    }
    Ok(())
}

fn check_token(name: &str, value: &Value) -> Result<(), Refusal> {
    let well_named = name.starts_with("--")
        && name.len() <= 42
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !well_named {
        return Err(bad(format!(
            "`{name}` is not a design token (a token is `--` and lowercase letters, digits, `-` or `_`)"
        )));
    }
    let Some(value) = value.as_str() else {
        return Err(bad(format!("`{name}` must be a string value")));
    };
    if value.is_empty() || value.chars().count() > limits::TOKEN_VALUE {
        return Err(bad(format!(
            "`{name}` must be 1..={} characters",
            limits::TOKEN_VALUE
        )));
    }
    // A token is a value, never a fragment: these five are the ways a value stops being a value.
    for bad_part in ["url(", "@import", "javascript:", "expression(", ";", "}"] {
        if value.to_ascii_lowercase().contains(bad_part) {
            return Err(bad(format!(
                "`{name}` contains `{bad_part}`, which a token value may not"
            )));
        }
    }
    Ok(())
}

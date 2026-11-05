//! M45-72 — shelf documents: the shelves this instance keeps, and the query behind each.
//!
//! Saved views and their runs already exist. Nothing rendered them into one page, so a shelf was a row in
//! a table: the query that made it was invisible, and a query nobody publishes is a taste nobody can argue
//! with. This module is the page.
//!
//! **The query is the point.** A shelf named "grimdark" could mean anything; `tag:dark AND tag:progression`
//! is an editorial decision someone can disagree with. The brief (M45-71) says what a shelf *is*; this says
//! what a shelf *does*.
//!
//! **Two shapes, rendered honestly.** A catalogue view stores the canonical query string, so it is printed
//! as stored. A library view stores structured filters, so they are printed as `key: value` pairs — and a
//! view this build cannot read says so, because "cannot render" and "no filters" are different facts.

use serde_json::Value as JsonValue;

/// One shelf, as the document lists it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Shelf {
    pub name: String,
    /// `library` or `public` — whether the view may carry private filters at all.
    pub scope: String,
    pub pinned: bool,
    /// The query, in words a reader can paste or argue with.
    pub query_text: String,
    pub last_changed: String,
    /// The last recorded run, when the view has one.
    pub last_run: Option<Run>,
}

/// The last time the shelf was run, and how much it found.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Run {
    pub ran_at: String,
    pub seen: i64,
}

/// Render a stored view document as text.
///
/// `None` when the document is not understandable — an older or newer document version. The caller says so
/// in the page rather than printing an empty query, because an empty query would claim the shelf has no
/// filters.
#[must_use]
pub fn query_text(document: &str) -> Option<String> {
    let doc: JsonValue = serde_json::from_str(document).ok()?;
    match doc.get("kind").and_then(JsonValue::as_str) {
        // A catalogue view is stored as the canonical query string, so it is published as-is: the reader
        // can paste it into `dh q` and get the same shelf.
        Some("catalogue") => doc
            .get("params")
            .and_then(JsonValue::as_str)
            .filter(|params| !params.trim().is_empty())
            .map(|params| params.trim().to_owned()),
        _ => {
            let query = doc.get("query")?;
            let mut parts = Vec::new();
            for (key, value) in [
                ("shelves", query.get("shelves")),
                ("tags", query.get("tags")),
                ("statuses", query.get("statuses")),
            ] {
                if let Some(list) = value.and_then(JsonValue::as_array) {
                    let names: Vec<String> = list
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect();
                    if !names.is_empty() {
                        parts.push(format!("{key}: {}", names.join(", ")));
                    }
                }
            }
            for (key, value) in [
                ("source", query.get("source")),
                ("updated since", query.get("updated_since")),
            ] {
                if let Some(text) = value.and_then(JsonValue::as_str) {
                    if !text.trim().is_empty() {
                        parts.push(format!("{key}: {text}"));
                    }
                }
            }
            // Ordering is not a filter: a view that only sorts is "everything, in this order", so the sort
            // is printed only when there is something for it to order. A page that called `sort: recent` a
            // filter would claim an editorial decision the reader never made.
            if parts.is_empty() {
                return Some("everything on this shelf (no filters)".to_owned());
            }
            if let Some(sort) = query.get("sort").and_then(JsonValue::as_str) {
                parts.push(format!("sort: {sort}"));
            }
            Some(parts.join(" · "))
        }
    }
}

/// The document.
///
/// Deterministic in everything except the timestamps, which are the data: `last changed` and `last run` come
/// from the rows, so two renders of an unchanged library are identical.
#[must_use]
pub fn render(instance: &str, shelves: &[Shelf]) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Shelves at {instance}\n\n"));
    if shelves.is_empty() {
        out.push_str("_This account keeps no saved views._\n");
        return out;
    }
    // `shelf` → `shelves`, not `shelfs`: the page is read by people, and a plural that says "shelfs"
    // is the kind of thing that makes a document look generated.
    out.push_str(&format!(
        "{} {}. Each entry is the query that makes it, so the shelf can be disagreed with.\n",
        shelves.len(),
        if shelves.len() == 1 { "shelf" } else { "shelves" }
    ));
    for shelf in shelves {
        out.push_str(&format!("\n## {}\n\n", shelf.name));
        out.push_str(&format!("- scope: {}\n", shelf.scope));
        if shelf.pinned {
            out.push_str("- pinned: yes\n");
        }
        out.push_str(&format!("- query: `{}`\n", shelf.query_text));
        out.push_str(&format!("- last changed: {}\n", shelf.last_changed));
        match &shelf.last_run {
            Some(run) => out.push_str(&format!(
                "- last run: {} — {} item{}\n",
                run.ran_at,
                run.seen,
                if run.seen == 1 { "" } else { "s" }
            )),
            None => out.push_str("- last run: never\n"),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shelf(name: &str, query: &str) -> Shelf {
        Shelf {
            name: name.to_owned(),
            scope: "public".to_owned(),
            pinned: false,
            query_text: query.to_owned(),
            last_changed: "2026-11-01T00:00:00Z".to_owned(),
            last_run: None,
        }
    }

    #[test]
    fn a_catalogue_view_publishes_the_query_string_verbatim() {
        let doc = r#"{"version":1,"kind":"catalogue","params":"tag:dark AND trope:system-grind"}"#;
        assert_eq!(
            query_text(doc).as_deref(),
            Some("tag:dark AND trope:system-grind"),
            "a reader must be able to paste it into dh q and get the same shelf"
        );
    }

    #[test]
    fn a_library_view_renders_its_filters_and_says_when_it_has_none() {
        let doc = r#"{"version":1,"kind":"library","query":{"shelves":["grimdark"],"tags":["dark"],
                      "statuses":["finished"],"source":"ao3","updated_since":null,"sort":"recent"}}"#;
        let text = query_text(doc).expect("renderable");
        assert!(text.contains("shelves: grimdark"), "{text}");
        assert!(text.contains("tags: dark"), "{text}");
        assert!(text.contains("statuses: finished"), "{text}");
        assert!(text.contains("source: ao3"), "{text}");
        assert!(text.contains("sort: recent"), "{text}");
        assert!(
            !text.contains("updated since"),
            "an absent bound is not a filter: {text}"
        );

        let bare = r#"{"version":1,"kind":"library","query":{"shelves":[],"tags":[],"statuses":[],
                      "source":null,"updated_since":null,"sort":"recent"}}"#;
        assert_eq!(
            query_text(bare).as_deref(),
            Some("everything on this shelf (no filters)")
        );
    }

    #[test]
    fn a_document_this_build_cannot_read_renders_as_none_not_as_an_empty_query() {
        assert_eq!(query_text("not json"), None);
        assert_eq!(query_text(r#"{"version":99,"kind":"something_new"}"#), None);
        // A catalogue view with no params is unreadable too: it claims to be a query string and has none.
        assert_eq!(query_text(r#"{"kind":"catalogue","params":null}"#), None);
    }

    #[test]
    fn the_page_lists_every_shelf_with_its_run_or_the_word_never() {
        let mut one = shelf("grimdark", "tag:dark");
        one.last_run = Some(Run {
            ran_at: "2026-11-02T00:00:00Z".to_owned(),
            seen: 12,
        });
        let doc = render(
            "https://darkhold.example",
            &[one, shelf("cozy", "tag:fluff")],
        );
        assert!(doc.starts_with("# Shelves at https://darkhold.example"));
        assert!(doc.contains("## grimdark"));
        assert!(doc.contains("- last run: 2026-11-02T00:00:00Z — 12 items"));
        assert!(
            doc.contains("- last run: never"),
            "a shelf that never ran says so"
        );
        assert!(doc.contains("- query: `tag:fluff`"));
        assert!(doc.contains("2 shelves"), "the plural is `shelves`: {doc}");
    }

    #[test]
    fn an_empty_document_says_there_are_no_shelves_instead_of_just_a_heading() {
        let doc = render("https://darkhold.example", &[]);
        assert!(doc.contains("_This account keeps no saved views._"));
    }
}

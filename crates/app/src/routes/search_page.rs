//! Search results, rendered as a document (ADR 0006, M71-20).
//!
//! An anonymous reader asking `/search?q=…` gets HTML before any script: a real GET form, the same hits the JSON door
//! returns from `search_works_ast_scoped`, and one link per hit to the work document. A signed-in reader keeps the SPA,
//! because saving a search and adding a work to the shelf are application actions and belong to the reader who has a
//! session. Nothing here invents a second query language or a second visibility rule: the document calls the same
//! engine as `/api/v1/search` with no viewer and applies the same cap.

use axum::extract::{OriginalUri, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use darkhold_db::search::{search_works_ast_scoped, SearchFilters};
use serde::Deserialize;

use crate::assets;
use crate::auth::MaybeSession;
use crate::html::Html;
use crate::routes::document as docs;
use crate::state::AppState;

/// The document's hard cap. A longer page is a denial of service against a crawler and an unreadable wall against a
/// reader; saying "at most 50" is honest where a total would have to run the query twice.
const MAX_HITS: i64 = 50;

/// The closed sort vocabulary shown by the document form. Anything else is named, never silently replaced.
const SORTS: [&str; 8] = [
    "for-you", "new", "updated", "top", "trending", "best-match", "az", "random",
];

pub fn router() -> Router<AppState> {
    Router::new().route("/search", get(search_page))
}

#[derive(Debug, Deserialize)]
struct SearchPageQuery {
    #[serde(default)]
    q: String,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    fandom: String,
    #[serde(default)]
    relationship: String,
    #[serde(default)]
    completion: String,
    #[serde(default)]
    length: String,
    #[serde(default)]
    warning: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    availability: String,
    #[serde(default)]
    reading_status: String,
    #[serde(default)]
    rating: String,
    #[serde(default)]
    sort: String,
}

/// Every active constraint, in the order the recovery message removes them from.
fn active_constraints(params: &SearchPageQuery) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for (name, value) in [
        ("q", params.q.trim()),
        ("fandom", params.fandom.trim()),
        ("relationship", params.relationship.trim()),
        ("completion", params.completion.trim()),
        ("length", params.length.trim()),
        ("warning", params.warning.trim()),
        ("source", params.source.trim()),
        ("language", params.language.trim()),
        ("availability", params.availability.trim()),
        ("reading_status", params.reading_status.trim()),
        ("rating", params.rating.trim()),
        ("sort", params.sort.trim()),
    ] {
        if !value.is_empty() {
            out.push((name, value.to_string()));
        }
    }
    out
}

/// The query string stays the source of truth. Fields the grammar already owns are appended as
/// terms; the length field is threaded through `SearchFilters` below, not invented as a new grammar.
fn effective_query(params: &SearchPageQuery) -> String {
    let mut terms = vec![params.q.trim().to_string()];
    for (field, value) in [
        ("fandom", params.fandom.trim()),
        ("relationship", params.relationship.trim()),
        ("completion", params.completion.trim()),
        ("language", params.language.trim()),
        ("source", params.source.trim()),
    ] {
        if !value.is_empty() {
            terms.push(format!("{field}:\"{}\"", value.replace('"', "")));
        }
    }
    terms.join(" AND ")
}

fn length_bounds(raw: &str) -> (Option<i64>, Option<i64>) {
    let raw = raw.trim();
    if let Some(value) = raw.strip_prefix('>') {
        (value.trim().parse().ok(), None)
    } else if let Some(value) = raw.strip_prefix('<') {
        (None, value.trim().parse().ok())
    } else if let Some((low, high)) = raw.split_once('-') {
        (low.trim().parse().ok(), high.trim().parse().ok())
    } else {
        (None, None)
    }
}

fn text_field(page: &mut Html, name: &str, label: &str, value: &str) {
    page.open("label", &[]).text(label).close("label");
    page.void("input", &[("name", name), ("value", value), ("aria-label", label)]);
}

fn default_limit() -> i64 {
    MAX_HITS
}

async fn search_page(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    OriginalUri(uri): OriginalUri,
    Query(params): Query<SearchPageQuery>,
) -> Response {
    // ADR 0006: the member keeps the application. The document exists for the reader who has no session and for the
    // crawler that never will have one; replacing the SPA wholesale would take away actions nobody asked to migrate.
    if session.is_some() {
        let shell = assets::serve(State(state.clone()), uri).await;
        if shell.status() == StatusCode::OK {
            return shell;
        }
    }

    let query = params.q.trim();
    let stylesheet = assets::app_stylesheet(&state).await;
    let site = state.config().site.name.clone();
    let mut page = docs::shell_active(
        &site,
        &if query.is_empty() {
            format!("Search — {site}")
        } else {
            format!("Search: {query} — {site}")
        },
        stylesheet.as_deref(),
        Some("/search"),
        None,
    );

    page.open("main", &[("class", "doc")]);
    page.open("h1", &[]).text("Search").close("h1");
    page.open(
        "form",
        &[
            ("action", "/search"),
            ("method", "get"),
            ("class", "search-form"),
        ],
    );
    page.open(
        "input",
        &[
            ("type", "search"),
            ("name", "q"),
            ("value", query),
            ("placeholder", "title, author or fandom"),
        ],
    );
    page.open("button", &[("type", "submit")])
        .text("Search")
        .close("button");
    page.open("details", &[("class", "constraints")]);
    page.open("summary", &[]).text("Constraints").close("summary");
    for (name, label, value) in [
        ("fandom", "Fandom", params.fandom.as_str()),
        ("relationship", "Relationship", params.relationship.as_str()),
        ("completion", "Completion", params.completion.as_str()),
        ("length", "Length (>30000, <5000 or 10000-30000)", params.length.as_str()),
        ("warning", "Warning", params.warning.as_str()),
        ("source", "Source", params.source.as_str()),
        ("language", "Language", params.language.as_str()),
        ("availability", "Availability", params.availability.as_str()),
        ("reading_status", "Reading status", params.reading_status.as_str()),
        ("rating", "Rating ceiling", params.rating.as_str()),
    ] {
        text_field(&mut page, name, label, value);
    }
    page.open("label", &[]).text("Sort").close("label");
    page.open("select", &[("name", "sort"), ("aria-label", "Sort")]);
    for value in SORTS {
        page.open_opt(
            "option",
            &[
                ("value", Some(value)),
                ("selected", (params.sort == value).then_some("selected")),
            ],
        )
        .text(value)
        .close("option");
    }
    page.close("select");
    page.open("button", &[("type", "submit"), ("class", "apply-constraints")])
        .text("Apply constraints")
        .close("button");
    page.close("details");
    page.close("form");

    if !params.sort.is_empty() && !SORTS.contains(&params.sort.as_str()) {
        page.open("p", &[("class", "status")])
            .text(&format!(
                "Unsupported sort: {}. Choose one of: {}.",
                params.sort,
                SORTS.join(" | ")
            ))
            .close("p");
        page.close("main");
        page.close("body");
        page.close("html");
        let mut response = docs::page_response(page);
        *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
        return response;
    }

    let unsupported = [
        ("warning", params.warning.trim()),
        ("availability", params.availability.trim()),
        ("reading_status", params.reading_status.trim()),
    ]
    .into_iter()
    .filter(|(_, value)| !value.is_empty())
    .map(|(name, value)| format!("{name}={value}"))
    .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        page.open("p", &[("class", "status")])
            .text(&format!(
                "These constraints are not applied by the document search: {}.",
                unsupported.join(", ")
            ))
            .close("p");
    }

    if query.is_empty() {
        page.open("p", &[])
            .text("Enter a title, an author or a fandom to search the archive.")
            .close("p");
    } else {
        let limit = params.limit.clamp(1, MAX_HITS);
        let (min_words, max_words) = length_bounds(&params.length);
        let sort = match params.sort.as_str() {
            "" => None,
            "new" | "updated" => Some("newest"),
            "az" => Some("title"),
            "for-you" | "top" | "trending" | "best-match" | "random" => Some("popularity"),
            other => Some(other),
        };
        let filters = SearchFilters {
            min_words,
            max_words,
            max_rating: (!params.rating.trim().is_empty()).then(|| params.rating.clone()),
            sort: sort.map(str::to_string),
        };
        match search_works_ast_scoped(
            state.db(),
            &effective_query(&params),
            None,
            limit,
            &[] as &[(String, String)],
            &filters,
        )
        .await
        {
            Ok(hits) => {
                page.open("p", &[("class", "counts")]);
                if hits.is_empty() {
                    let constraints = active_constraints(&params);
                    let narrowest = constraints
                        .last()
                        .map(|(name, value)| format!("{name}={value}"))
                        .unwrap_or_else(|| "q".to_string());
                    page.text("No works match these constraints. Remove the last constraint ")
                        .text(&format!("({narrowest})."));
                } else {
                    page.text(&format!(
                        "{} published work(s) matched “{query}”.",
                        hits.len()
                    ));
                    if hits.len() as i64 == limit {
                        page.text(&format!(" Showing at most {limit}."));
                    }
                }
                page.close("p");

                if !hits.is_empty() {
                    page.open("ul", &[("class", "results")]);
                    for hit in &hits {
                        page.open("li", &[("class", "result")]);
                        page.open("a", &[("href", &format!("/works/{}", hit.work_id))])
                            .text(&hit.title)
                            .close("a");
                        page.open("p", &[("class", "byline")])
                            .text(&format!("by {}", hit.author_handle))
                            .close("p");
                        page.open("p", &[("class", "metrics")])
                            .text(&format!("{} words · {} points", hit.word_count, hit.score))
                            .close("p");
                        page.open("p", &[("class", "status")])
                            .text(&format!("Status: {}", hit.status))
                            .close("p");
                        let mut source = if hit.source_key.trim().is_empty() {
                            "source unmeasured".to_string()
                        } else {
                            format!("Source: {}", hit.source_key)
                        };
                        if !hit.source_updated_at.trim().is_empty() {
                            source.push_str(&format!(" · updated {}", hit.source_updated_at));
                        }
                        page.open("p", &[("class", "source")])
                            .text(&source)
                            .close("p");
                        page.close("li");
                    }
                    page.close("ul");
                }
            }
            Err(error) => {
                page.open("p", &[("class", "status")])
                    .text(&format!("That search could not be understood: {error}"))
                    .close("p");
            }
        }
    }

    page.close("main");
    page.close("body");
    page.close("html");
    docs::page_response(page)
}

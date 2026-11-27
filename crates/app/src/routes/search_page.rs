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
use crate::routes::document as docs;
use crate::state::AppState;

/// The document's hard cap. A longer page is a denial of service against a crawler and an unreadable wall against a
/// reader; saying "at most 50" is honest where a total would have to run the query twice.
const MAX_HITS: i64 = 50;

pub fn router() -> Router<AppState> {
    Router::new().route("/search", get(search_page))
}

#[derive(Debug, Deserialize)]
struct SearchPageQuery {
    #[serde(default)]
    q: String,
    #[serde(default = "default_limit")]
    limit: i64,
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
    page.close("form");

    if query.is_empty() {
        page.open("p", &[])
            .text("Enter a title, an author or a fandom to search the archive.")
            .close("p");
    } else {
        let limit = params.limit.clamp(1, MAX_HITS);
        match search_works_ast_scoped(
            state.db(),
            query,
            None,
            limit,
            &[] as &[(String, String)],
            &SearchFilters::default(),
        )
        .await
        {
            Ok(hits) => {
                page.open("p", &[("class", "counts")]);
                if hits.is_empty() {
                    page.text(&format!("No published works matched “{query}”."));
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

//! An author's page, rendered as a document (ADR 0006, M71-20).
//!
//! Work-page bylines already link to `/people/{handle}`; before this route that link landed on the SPA fallback. The
//! page reuses the same `search_works_ast_scoped` engine as search with `author:"handle"`, so it cannot drift into a
//! second visibility rule: a private, draft or restricted work stays off a public author page because the engine filters
//! it before the page sees it. No new store query, no new query grammar and no auction over whose guess about authorship
//! wins — the pseud is the archive's author, as ADR 0006 says.

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use darkhold_db::search::{search_works_ast_scoped, SearchFilters};

use crate::assets;
use crate::auth::MaybeSession;
use crate::routes::document as docs;
use crate::state::AppState;

/// One page, not a fragment. A longer author page is unreadable and slow for crawlers; the cap is printed so the reader
/// is never told that "50" is the author's whole library.
const MAX_WORKS: i64 = 50;

pub fn router() -> Router<AppState> {
    Router::new().route("/people/{handle}", get(author_page))
}

/// Handles in the archive are ASCII slugs. Rejecting anything else here keeps the AST query below from being a place a
/// URL can inject a second search field; a handle that cannot be named is simply not a page.
fn safe_handle(handle: &str) -> bool {
    !handle.is_empty()
        && handle.len() <= 64
        && handle
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

async fn author_page(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Path(handle): Path<String>,
) -> Response {
    let site = state.config().site.name.clone();
    let stylesheet = assets::app_stylesheet(&state).await;
    if !safe_handle(&handle) {
        return docs::not_found(&site, "That author does not exist.", stylesheet.as_deref());
    }

    // The pseud is the author's name in the archive. Checking it first makes "no such person" and "a person with no
    // reachable works" the same honest 404 instead of an empty page that promises a library.
    let Ok(Some(_pseud)) = darkhold_db::identity::find_pseud_by_handle(state.db(), &handle).await
    else {
        return docs::not_found(&site, "That author does not exist.", stylesheet.as_deref());
    };

    // `author:` resolves against `pseuds.handle` in the same grouped query every search uses. Quoting the value keeps a
    // hyphen or underscore literal; `safe_handle` already excluded quotes and backslashes.
    let query = format!("author:\"{handle}\"");
    let Ok(works) = search_works_ast_scoped(
        state.db(),
        &query,
        None,
        MAX_WORKS + 1,
        &[] as &[(String, String)],
        &SearchFilters::default(),
    )
    .await
    else {
        return docs::not_found(&site, "That author's works are not available.", stylesheet.as_deref());
    };
    if works.is_empty() {
        return docs::not_found(&site, "That author has no published works yet.", stylesheet.as_deref());
    }
    let more = works.len() as i64 > MAX_WORKS;
    let works = &works[..(MAX_WORKS as usize).min(works.len())];

    let mut page = docs::shell(
        &site,
        &format!("Works by {handle} — {site}"),
        stylesheet.as_deref(),
    );
    page.open("main", &[("class", "doc")]);
    page.open("h1", &[]).text(&format!("Works by {handle}")).close("h1");
    page.open("p", &[("class", "counts")]);
    page.text(&format!(
        "{} published public work(s) by {handle}.",
        works.len()
    ));
    if more {
        page.text(&format!(" Showing at most {MAX_WORKS}."));
    }
    page.close("p");

    page.open("ul", &[("class", "results")]);
    for work in works {
        page.open("li", &[("class", "result")]);
        page.open("a", &[("href", &format!("/works/{}", work.work_id))])
            .text(&work.title)
            .close("a");
        page.open("p", &[("class", "byline")])
            .text(&format!("by {}", work.author_handle))
            .close("p");
        page.open("p", &[("class", "metrics")])
            .text(&format!("{} words · {} points", work.word_count, work.score))
            .close("p");
        page.open("p", &[("class", "status")])
            .text(&format!("Status: {}", work.status))
            .close("p");
        page.close("li");
    }
    page.close("ul");
    page.close("main");
    page.close("body");
    page.close("html");
    docs::page_response(page)
}

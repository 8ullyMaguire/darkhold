//! A collection's page, rendered as a document (ADR 0006, M71-20).
//!
//! Collections are linkable archive objects: a reader arrives from a shelf, a recommendation or a shared URL. Before
//! this route those arrivals got the SPA shell. The page uses the events collection model — the one `collection_items`
//! writes — and reuses `content::find_work` for the member rows, so no second collection model and no new store query is
//! introduced. A private collection is a 404 (§8.9): a teaser would disclose that it exists.

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;

use crate::assets;
use crate::auth::MaybeSession;
use crate::routes::document as docs;
use crate::state::AppState;

/// A shelf is a page, not a firehose. A collection with more rows than this says so rather than ending abruptly.
const MAX_ITEMS: usize = 50;

pub fn router() -> Router<AppState> {
    Router::new().route("/collections/{id}", get(collection_page))
}

async fn collection_page(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Path(id): Path<String>,
) -> Response {
    let site = state.config().site.name.clone();
    let stylesheet = assets::app_stylesheet(&state).await;
    let not_found = |message: &str| docs::not_found(&site, message, stylesheet.as_deref());

    let Ok(Some(collection)) = darkhold_db::events::get_collection(state.db(), &id).await else {
        return not_found("That collection does not exist.");
    };
    if !collection.is_public {
        return not_found("That collection does not exist.");
    }

    let Ok(items) = darkhold_db::events::list_collection_items(state.db(), &id).await else {
        return not_found("That collection's contents are not available.");
    };
    let more = items.len() > MAX_ITEMS;

    // One lookup per item is deliberate: a collection is short, and `find_work` is the same gate the work page uses.
    // Checking the stored visibility and lifecycle here keeps a private or deleted work off a public shelf even when a
    // stale `collection_items` row still names it.
    let mut rows: Vec<(String, String)> = Vec::new();
    for item in items.into_iter().take(MAX_ITEMS) {
        let Ok(work_id) = item.work_id.parse() else {
            continue;
        };
        let Ok(Some(work)) = darkhold_db::content::find_work(state.db(), work_id).await else {
            continue;
        };
        if work.visibility != "public" || work.lifecycle != "published" {
            continue;
        }
        rows.push((work.id.to_string(), work.title));
    }
    if rows.is_empty() {
        return not_found("That collection has no published works yet.");
    }

    let coverage = darkhold_db::events::collection_coverage(state.db(), &id)
        .await
        .ok()
        .map(|c| c.sentence)
        .unwrap_or_default();

    let mut page = docs::shell(
        &site,
        &format!("{} — {site}", collection.name),
        stylesheet.as_deref(),
    );
    page.open("main", &[("class", "doc")]);
    page.open("h1", &[]).text(&collection.name).close("h1");
    if let Some(description) = collection.description {
        page.open("p", &[("class", "counts")])
            .text(&description)
            .close("p");
    }
    if !coverage.is_empty() {
        page.open("p", &[("class", "counts")])
            .text(&coverage)
            .close("p");
    }
    if more {
        page.open("p", &[("class", "counts")])
            .text(&format!("Showing at most {MAX_ITEMS} works."))
            .close("p");
    }

    page.open("ul", &[("class", "results")]);
    for (work_id, title) in &rows {
        page.open("li", &[("class", "result")]);
        page.open("a", &[("href", &format!("/works/{work_id}"))])
            .text(title)
            .close("a");
        page.open("p", &[("class", "status")])
            .text("Status: live")
            .close("p");
        page.close("li");
    }
    page.close("ul");
    page.close("main");
    page.close("body");
    page.close("html");
    docs::page_response(page)
}

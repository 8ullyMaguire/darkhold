//! A tag's page, rendered as a document (ADR 0006, M71-20).
//!
//! Browsing by tag is the archive's second door after the work page: it is where a reader arrives from a link, a search
//! result or a friend's recommendation, and where a crawler learns that the archive has a shape. It rendered as the SPA
//! shell, which meant none of those could read it.
//!
//! What it renders: the tag, how many works it actually returned (and whether that is everything), and the works
//! themselves in catalogue order with their author, each linking to the work page — itself a document, so a reader with
//! JavaScript off can walk from a tag to a work to its first chapter without a single round trip through the application.
//!
//! What it does not render: a tag with nothing behind it. That is a 404 page, not an empty list, because a tag page with
//! no works is an empty promise (§8.11). The listing itself comes from `works_by_tag`, which filters to published,
//! public works in the query — so a draft, a private work or a restricted one cannot appear here even if a row says so.

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;

use crate::assets;
use crate::auth::MaybeSession;
use crate::routes::document as docs;
use crate::state::AppState;

/// How many works one page lists. There is no pagination yet, so the number is chosen to be a page rather than a
/// fragment, and the page says when it is showing a subset instead of pretending the archive ends here.
const PAGE: i64 = 50;

pub fn router() -> Router<AppState> {
    Router::new().route("/tags/{tag}", get(tag_page))
}

/// `MaybeSession` even though nothing on the page varies by viewer yet: the route table's contract is that a public
/// door names its extractor, so "anonymous may call this" is a claim the gate can check instead of an assumption.
async fn tag_page(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Path(raw): Path<String>,
) -> Response {
    let site = state.config().site.name.clone();
    let stylesheet = assets::app_stylesheet(&state).await;
    let shown = raw.trim();
    let norm = darkhold_domain::exchange::normalise(shown);
    if norm.is_empty() {
        return docs::not_found(&site, "That tag does not exist.", stylesheet.as_deref());
    }

    // One over the page size: the extra row answers "is there more" without a second count query, and the count of a
    // tag is the count of what the reader can actually reach (§10.3).
    let Ok(mut rows) =
        darkhold_db::taxonomy::works_by_tag(state.db(), &norm, PAGE + 1, 0).await
    else {
        return docs::not_found(&site, "That tag does not exist.", stylesheet.as_deref());
    };
    if rows.is_empty() {
        return docs::not_found(&site, "That tag does not exist.", stylesheet.as_deref());
    }
    let more = rows.len() as i64 > PAGE;
    rows.truncate(PAGE as usize);

    let mut page = docs::shell(&site, &format!("{shown} — {site}"), stylesheet.as_deref());
    page.open("main", &[]);
    page.open("h1", &[]).text(shown).close("h1");
    page.open("p", &[("class", "counts")])
        .text(&if more {
            format!("more than {} works, most recently updated first", PAGE)
        } else if rows.len() == 1 {
            "1 work".to_owned()
        } else {
            format!("{} works", rows.len())
        })
        .close("p");

    page.open("ul", &[("class", "entries")]);
    for (id, title, handle, updated) in &rows {
        page.open("li", &[]);
        page.open("a", &[("href", &format!("/works/{id}"))])
            .text(title)
            .close("a");
        page.open("div", &[("class", "meta")])
            .text(&format!("by {handle} · updated {}", &updated[..10.min(updated.len())]))
            .close("div");
        page.close("li");
    }
    page.close("ul");

    page.open("p", &[])
        .open("a", &[("href", "/tags")])
        .text("All tags")
        .close("a")
        .close("p");
    page.close("main");
    page.close("body");
    page.close("html");

    docs::page_response(page)
}

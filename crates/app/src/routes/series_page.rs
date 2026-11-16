//! A series' page, rendered as a document (ADR 0006, M71-20).
//!
//! A series is an ordered set of works with a stable slug. Before this route a shared series URL landed on the SPA
//! shell; now a reader with no script gets the series title, its description and its works in the author's order, each
//! linking to the work document. The read comes from `series_recs::page_by_slug`, which filters to published public
//! undeleted works in the same statement, so the page cannot invent a visibility rule of its own.

use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;

use crate::assets;
use crate::auth::MaybeSession;
use crate::routes::document as docs;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/series/{slug}", get(series_page))
}

async fn series_page(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Path(slug): Path<String>,
) -> Response {
    let site = state.config().site.name.clone();
    let stylesheet = assets::app_stylesheet(&state).await;
    let not_found = |message: &str| docs::not_found(&site, message, stylesheet.as_deref());
    if slug.is_empty() || slug.len() > 64 {
        return not_found("That series does not exist.");
    }

    let Ok(Some(series)) = darkhold_db::series_recs::page_by_slug(state.db(), &slug).await else {
        return not_found("That series does not exist.");
    };

    let mut page = docs::shell(
        &site,
        &format!("{} — {site}", series.title),
        stylesheet.as_deref(),
    );
    page.open("main", &[("class", "doc")]);
    page.open("h1", &[]).text(&series.title).close("h1");
    if !series.description.is_empty() {
        page.open("p", &[("class", "counts")])
            .text(&series.description)
            .close("p");
    }
    page.open("p", &[("class", "counts")]);
    page.text(&format!(
        "{} published work(s) in this series.",
        series.entries.len()
    ));
    page.close("p");

    page.open("ol", &[("class", "results")]);
    for entry in &series.entries {
        page.open("li", &[("class", "result")]);
        page.open("a", &[("href", &format!("/works/{}", entry.work_id))])
            .text(&entry.title)
            .close("a");
        page.open("p", &[("class", "status")])
            .text(&format!("Position {} · Status: live", entry.position))
            .close("p");
        page.close("li");
    }
    page.close("ol");
    page.close("main");
    page.close("body");
    page.close("html");
    docs::page_response(page)
}

//! M71-07 — the operator overview is a document.
//!
//! The console's own front door was missing: four admin SPA pages existed, but nothing rendered every group from live
//! counts or named the red states in one place. This page uses the store doors the other admin views already use; it
//! does not add an admin API. Non-operators get the same 404 the API's `require_operator` gives them, so the page's
//! existence is not disclosed. The alert bar reproduces the `AlertBar.svelte` contract in HTML because a server
//! document cannot mount a Svelte component: role=alert, a plain-language label and a fix link for every red state.

use axum::extract::State;
use axum::response::Response;

use darkhold_domain::AppError;

use crate::assets;
use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::routes::document as docs;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new().route("/admin", axum::routing::get(overview))
}

fn internal(error: anyhow::Error) -> ApiError {
    ApiError(AppError::Internal(error))
}

async fn overview(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Response> {
    // One refusal, same predicate and same status as the admin API. The session extractor answers "who are you" first
    // (401 for anonymous); an authenticated non-operator gets 404 here, so the surface's existence stays undisclosed.
    crate::routes::discovery::require_operator(&state, &user)?;

    let site = state.config().site.name.clone();
    let stylesheet = assets::app_stylesheet(&state).await;
    let db = state.db();
    let job_counts = darkhold_db::jobs::counts_by_state(db)
        .await
        .map_err(internal)?;
    let sources = darkhold_db::imports::list_sources(db)
        .await
        .map_err(internal)?;
    // The same threshold the media-health dashboard reports. It is a display constant here, not a second policy.
    const MIN_HEALTHY: i64 = 3;
    let mirrored = darkhold_db::media_resilience::count_well_mirrored(db, MIN_HEALTHY)
        .await
        .map_err(internal)?;
    let needs_rescue =
        darkhold_db::media_resilience::count_references_below_threshold(db, MIN_HEALTHY)
            .await
            .map_err(internal)?;

    let mut alerts: Vec<(String, &'static str)> = Vec::new();
    for (state_word, count) in &job_counts {
        if state_word == "failed" && *count > 0 {
            alerts.push((format!("{count} job(s) failed"), "/admin/jobs"));
        }
    }
    for source in &sources {
        if source.health != "healthy" && source.health != "unknown" {
            alerts.push((
                format!("Source {} is {}", source.key, source.health),
                "/admin/media-health",
            ));
        }
    }
    if needs_rescue > 0 {
        alerts.push((
            format!("{needs_rescue} media reference(s) need rescue"),
            "/admin/media-health",
        ));
    }

    let mut page = docs::shell(
        &site,
        &format!("Operator overview — {site}"),
        stylesheet.as_deref(),
    );
    page.open("main", &[("class", "doc")]);
    page.open("h1", &[]).text("Operator overview").close("h1");
    page.open("p", &[("class", "counts")])
        .text("Live counts from the store. Red states name themselves and carry a fix link.")
        .close("p");

    if !alerts.is_empty() {
        page.open("section", &[("role", "alert"), ("class", "alerts")]);
        page.open("h2", &[]).text("Needs attention").close("h2");
        page.open("ul", &[]);
        for (label, href) in &alerts {
            page.open("li", &[]);
            page.open("span", &[]).text(label).close("span");
            page.open("a", &[("href", href)])
                .text(&format!("Fix {label}"))
                .close("a");
            page.close("li");
        }
        page.close("ul");
        page.close("section");
    }

    page.open("section", &[]);
    page.open("h2", &[]).text("Queue").close("h2");
    page.open("ul", &[]);
    for (state_word, count) in &job_counts {
        page.open("li", &[]);
        page.text(&format!("{state_word}: {count}"));
        if state_word == "failed" {
            page.text(" red");
        } else if state_word == "succeeded" {
            page.text(" green");
        } else {
            page.text(" amber");
        }
        page.close("li");
    }
    if job_counts.is_empty() {
        page.open("li", &[]).text("no jobs recorded").close("li");
    }
    page.close("ul");
    page.close("section");

    page.open("section", &[]);
    page.open("h2", &[]).text("Sources").close("h2");
    page.open("ul", &[]);
    for source in &sources {
        page.open("li", &[]);
        page.text(&format!("{}: {}", source.key, source.health));
        if source.health == "healthy" {
            page.text(" green");
        } else if source.health == "unknown" {
            page.text(" amber");
        } else {
            page.text(" red");
        }
        page.close("li");
    }
    if sources.is_empty() {
        page.open("li", &[])
            .text("no sources configured")
            .close("li");
    }
    page.close("ul");
    page.close("section");

    page.open("section", &[]);
    page.open("h2", &[]).text("Media").close("h2");
    page.open("ul", &[]);
    page.open("li", &[])
        .text(&format!("well mirrored: {mirrored} green"))
        .close("li");
    page.open("li", &[])
        .text(&format!("need rescue: {needs_rescue} red"))
        .close("li");
    page.close("ul");
    page.close("section");

    page.close("main");
    page.close("body");
    page.close("html");
    Ok(docs::page_response(page))
}

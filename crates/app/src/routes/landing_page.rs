//! The landing page, rendered as a document.
//!
//! ADR 0006: one binary, two renderers. This is the first page of the migration — the one a stranger, a crawler, a
//! link preview and an e-reader client reach first, and the one that has no session state to lose (a signed-in reader
//! is redirected to `/discover`, as the SPA always did).
//!
//! What it renders, and what it refuses to render:
//!
//! * the instance's name and tagline, because the instance has a voice (spec §12.1);
//! * the size of the catalogue, from the catalogue itself, and the number of sources behind it;
//! * the published taste, as the tags the instance theme actually weights — never the operator's private profile;
//! * up to six works a reader can begin *here*: a work is linked only when its first readable unit exists, so a
//!   no-JavaScript reader never follows a link into an application that will not render for them (§8.13, "never link
//!   a dead location" — the same reason applies to a location that cannot render);
//! * nothing where there is nothing: an empty catalogue renders no heading at all (§8.11).
//!
//! The page carries **no script tag**. That is the assertion its test makes, and it is the difference between this and
//! the shell it replaces.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::assets;
use crate::auth::MaybeSession;
use crate::html::Html;
use crate::state::AppState;

/// How many works the landing page offers to start from.
const STARTERS: i64 = 6;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(landing))
}

/// One work, as the landing page needs it.
struct Starter {
    title: String,
    href: Option<String>,
    author: String,
    words: i64,
    completion: String,
    rating: String,
}

async fn landing(State(state): State<AppState>, MaybeSession(session): MaybeSession) -> Response {
    // A signed-in reader has a discovery surface and a profile; the landing page is for the stranger. This is the
    // behaviour the SPA had, moved to the server where it also saves the round trip.
    if session.is_some() {
        return axum::response::Redirect::to("/discover").into_response();
    }

    let config = state.config();
    let name = config.site.name.clone();
    let tagline = config.site.tagline.clone();

    // Every fact on this page is best-effort: an instance whose catalogue query fails still has a name, a tagline and
    // a sign-in link, and saying "the catalogue could not be read" is more honest than a 500 page (§8.12).
    let total = darkhold_db::catalogue::total_rows(state.db()).await.ok();
    let sources = darkhold_db::catalogue::sources(state.db())
        .await
        .map(|rows| rows.len())
        .ok();

    let starters = starters(&state).await;
    let theme_tags = published_theme_tags(&state).await;

    let stylesheet = assets::app_stylesheet(&state).await;

    let body = render(
        &name,
        &tagline,
        total,
        sources,
        &starters,
        &theme_tags,
        stylesheet.as_deref(),
    );

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

/// The works the page offers, in catalogue freshness order, each linked only if it can actually be read here.
async fn starters(state: &AppState) -> Vec<Starter> {
    let Ok(ids) = darkhold_db::content::list_public_works_by_freshness(
        state.db(),
        darkhold_db::content::FreshnessSort::New,
        STARTERS,
    )
    .await
    else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let Ok(Some(work)) = darkhold_db::content::find_work(state.db(), id).await else {
            continue;
        };
        if work.lifecycle != "published" {
            continue;
        }
        let units = darkhold_db::content::units_for_work(state.db(), id)
            .await
            .unwrap_or_default();
        let first_readable = units
            .iter()
            .find(|unit| unit.current_revision_id.is_some());
        let words: i64 = units
            .iter()
            .filter(|unit| unit.current_revision_id.is_some())
            .map(|unit| unit.word_count)
            .sum();
        let author = darkhold_db::identity::find_pseud(state.db(), work.owner_pseud_id)
            .await
            .ok()
            .flatten()
            .map(|pseud| pseud.handle)
            .unwrap_or_default();

        out.push(Starter {
            title: work.title,
            // The reader's URL is a document; the work page is still the application. Link the document.
            // The reader's URL takes the unit's id, not its order key: the route is
            // `/works/{work_id}/units/{unit_id}` and `parse_unit_id` refuses anything that is not a unit id.
            href: first_readable.map(|unit| format!("/works/{}/units/{}", work.id, unit.id)),
            author,
            words,
            completion: work.completion,
            rating: work.rating,
        });
    }
    out
}

/// The instance's published taste, as its heaviest tags. Empty unless the theme is published: the operator's taste is
/// theirs to publish or withhold, and an unpublished theme must not leak through a summary line (§12.1, §8.4).
async fn published_theme_tags(state: &AppState) -> Vec<String> {
    let instance_id = state.config().site.base_url.clone();
    let Ok(Some(theme)) = darkhold_db::instance_theme::get_local_theme(state.db(), &instance_id).await
    else {
        return Vec::new();
    };
    if !theme.public {
        return Vec::new();
    }
    let Some(map) = theme.theme_vector.as_object() else {
        return Vec::new();
    };
    let mut weighted: Vec<(String, f64)> = map
        .iter()
        .filter_map(|(tag, weight)| weight.as_f64().map(|w| (tag.clone(), w)))
        .collect();
    weighted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    weighted
        .into_iter()
        .take(6)
        .map(|(tag, _)| tag)
        .collect()
}

fn render(
    name: &str,
    tagline: &str,
    total: Option<i64>,
    sources: Option<usize>,
    starters: &[Starter],
    theme_tags: &[String],
    stylesheet: Option<&str>,
) -> String {
    let mut page = Html::new();
    page.raw("<!DOCTYPE html>");
    page.open("html", &[("lang", "en")]);
    page.open("head", &[]);
    page.void("meta", &[("charset", "utf-8")]);
    page.void(
        "meta",
        &[
            ("name", "viewport"),
            ("content", "width=device-width, initial-scale=1"),
        ],
    );
    page.void(
        "meta",
        &[("name", "description"), ("content", tagline)],
    );
    page.open("title", &[]).text(name).close("title");
    if let Some(href) = stylesheet {
        page.void("link", &[("rel", "stylesheet"), ("href", href)]);
    }
    // The page's own spacing and rhythm, in the site's tokens. Scoped to `.landing` so nothing here can reach the
    // application's components, and written inline because the server writes this document (§8.5).
    page.open("style", &[])
        .raw(
            ".landing{max-width:46rem;margin:0 auto;padding:var(--space-6) var(--space-4) var(--space-8);\
             font-family:var(--font-interface);color:var(--text-primary);line-height:var(--leading-normal)}\
             .landing h1{font-family:var(--font-heading);font-size:var(--text-3xl);line-height:var(--leading-tight);\
             margin:0 0 var(--space-2)}\
             .landing .tagline{font-size:var(--text-lg);color:var(--text-secondary);margin:0 0 var(--space-5)}\
             .landing h2{font-family:var(--font-heading);font-size:var(--text-xl);margin:var(--space-7) 0 var(--space-3)}\
             .landing p{margin:0 0 var(--space-3)}\
             .landing .counts{font-family:var(--font-mono);font-size:var(--text-sm);color:var(--text-secondary)}\
             .landing ul{list-style:none;margin:0;padding:0}\
             .landing li{border-bottom:1px solid var(--border-subtle, rgba(128,128,128,.3));padding:var(--space-3) 0}\
             .landing .work-title{font-size:var(--text-lg);color:var(--text-link, inherit)}\
             .landing .meta{font-family:var(--font-mono);font-size:var(--text-sm);color:var(--text-secondary)}\
             .landing .empty-note{color:var(--text-secondary)}\
             .landing footer{margin-top:var(--space-8);border-top:1px solid var(--border-subtle, rgba(128,128,128,.3));\
             padding-top:var(--space-4);font-size:var(--text-sm);color:var(--text-secondary)}",
        )
        .close("style");
    page.close("head");

    page.open("body", &[("class", "landing")]);
    page.open("main", &[]);

    // ── the declaration ───────────────────────────────────────────────────
    page.open("h1", &[]).text(name).close("h1");
    if !tagline.trim().is_empty() {
        page.open("p", &[("class", "tagline")]).text(tagline).close("p");
    }
    if let Some(total) = total {
        let mut line = format!("{total} works catalogued");
        if let Some(sources) = sources {
            if sources > 0 {
                line.push_str(&format!(
                    " from {sources} source{}",
                    if sources == 1 { "" } else { "s" }
                ));
            }
        }
        page.open("p", &[("class", "counts")]).text(&line).close("p");
    }
    if !theme_tags.is_empty() {
        page.open("p", &[("class", "counts")])
            .text("This instance's published taste: ")
            .text(&theme_tags.join(" · "))
            .close("p");
    }

    // ── start reading (absent when there is nothing to start) ─────────────
    if !starters.is_empty() {
        page.open("section", &[]);
        page.open("h2", &[]).text("Start reading").close("h2");
        page.open("ul", &[]);
        for starter in starters {
            page.open("li", &[]);
            match &starter.href {
                Some(href) => {
                    page.open("a", &[("class", "work-title"), ("href", href)])
                        .text(&starter.title)
                        .close("a");
                }
                // No readable unit here: the title is text, not a link into an application that cannot render.
                None => {
                    page.open("span", &[("class", "work-title")])
                        .text(&starter.title)
                        .close("span");
                }
            }
            let mut meta = Vec::new();
            if !starter.author.trim().is_empty() {
                meta.push(format!("by {}", starter.author));
            }
            if starter.words > 0 {
                meta.push(format!("{} words", starter.words));
            }
            if !starter.completion.is_empty() {
                meta.push(starter.completion.replace('_', " "));
            }
            if !starter.rating.is_empty() {
                meta.push(starter.rating.replace('_', " "));
            }
            page.open("div", &[("class", "meta")])
                .text(&meta.join(" · "))
                .close("div");
            page.close("li");
        }
        page.close("ul");
        page.close("section");
    }

    // ── what this place is for ───────────────────────────────────────────
    page.open("section", &[]);
    page.open("h2", &[]).text("What you can do here").close("h2");
    page.open("p", &[])
        .text("Reading is free and needs no account. ")
        .open("a", &[("href", "/browse")])
        .text("Browse the catalogue")
        .close("a")
        .text(" by fandom, tag or length.");
    page.close("p");
    page.open("p", &[])
        .text("Writing: publish serially, keep every revision, and choose what kind of feedback reaches you. ")
        .open("a", &[("href", "/write")])
        .text("Open the editor")
        .close("a")
        .text(".");
    page.close("p");
    page.open("p", &[])
        .text("Community: discuss what you read, recommend what deserves it, and govern this instance with the people in it. ")
        .open("a", &[("href", "/forum")])
        .text("Browse the forum")
        .close("a")
        .text(".");
    page.close("p");
    page.close("section");

    page.open("footer", &[]);
    page.open("a", &[("href", "/auth/sign-in")])
        .text("Sign in")
        .close("a");
    page.text(" · ");
    page.open("a", &[("href", "/about")]).text("About").close("a");
    page.close("footer");

    page.close("main");
    page.close("body");
    page.close("html");
    page.finish()
}

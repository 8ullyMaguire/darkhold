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

use std::collections::HashMap;

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
/// Candidates inspected to find six a stranger can actually open. Fresh imports are
/// mostly `accounts_only`, so the pool has to be larger than the answer.
const STARTER_POOL: i64 = 60;
/// Gated works are still named on the page (a reader sees what exists), but as text only.
const GATED_TEXT: usize = 2;

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
    // Start with works a stranger might open; only if that pool is short do we fall back to the
    // freshness queue, so a gated work is still named without ever taking a link slot.
    let Ok(mut ids) =
        darkhold_db::content::list_open_shelf_candidates(state.db(), STARTER_POOL).await
    else {
        return Vec::new();
    };
    if ids.len() < (STARTERS + GATED_TEXT as i64) as usize {
        if let Ok(fresh) = darkhold_db::content::list_public_works_by_freshness(
            state.db(),
            darkhold_db::content::FreshnessSort::New,
            STARTER_POOL,
        )
        .await
        {
            for id in fresh {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }

    let mut out = Vec::with_capacity(STARTERS as usize + GATED_TEXT);
    let mut linked = 0usize;
    let mut gated = 0usize;
    // A freshness feed can be one prolific importer repeated six times. Show a range of hands:
    // two works per author at most, and stop scanning once the shelf is full rather than asking
    // the reader policy about every candidate in the pool.
    let mut author_counts: HashMap<String, usize> = HashMap::new();
    let mut scanned_after_full = 0usize;
    for id in ids {
        if linked >= STARTERS as usize
            && (gated >= GATED_TEXT || scanned_after_full >= GATED_TEXT * 4)
        {
            break;
        }
        if linked >= STARTERS as usize {
            scanned_after_full += 1;
        }
        let Ok(Some(work)) = darkhold_db::content::find_work(state.db(), id).await else {
            continue;
        };
        if work.lifecycle != "published" {
            continue;
        }
        let units = darkhold_db::content::units_for_work(state.db(), id)
            .await
            .unwrap_or_default();
        let first_readable = units.iter().find(|unit| unit.current_revision_id.is_some());
        // A unit with a revision is not a unit *this visitor* may read. A work whose body audience is narrower than the
        // instance's (27 464 rows on the live estate are `accounts_only`) is answered 404 by the reader for a stranger, so
        // linking it from the front page publishes a dead link — which is what a browser crawl of the live site found on
        // 2026-11-13. The decision is asked of the reader's own policy rather than re-derived here, so the two cannot drift.
        let readable_by_stranger = matches!(
            super::works::reading_decision(state, None, &work, &[]).await,
            super::works::Reading::Public | super::works::Reading::Contributor
        );
        // Precedence matters: only a work this visitor can read consumes one of the six
        // linked slots. Gated works remain visible as text, but never take a link slot.
        let unit_href = readable_by_stranger
            .then(|| first_readable.map(|unit| format!("/works/{}/units/{}", work.id, unit.id)))
            .flatten();
        if unit_href.is_some() {
            if linked >= STARTERS as usize {
                continue;
            }
            linked += 1;
        } else {
            if gated >= GATED_TEXT {
                continue;
            }
            gated += 1;
        }

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

        if !author.is_empty() {
            let count = author_counts.entry(author.clone()).or_insert(0);
            if *count >= 2 {
                continue;
            }
            *count += 1;
        }

        out.push(Starter {
            title: work.title,
            // The reader's URL is a document; the work page is still the application. Link the document.
            // The reader's URL takes the unit's id, not its order key: the route is
            // `/works/{work_id}/units/{unit_id}` and `parse_unit_id` refuses anything that is not a unit id.
            href: unit_href,
            author,
            words,
            completion: work.completion,
            rating: work.rating,
        });
    }
    // Linked works first: a stranger sees what they can open before what is gated.
    out.sort_by_key(|starter| if starter.href.is_some() { 0 } else { 1 });
    out
}

/// The instance's published taste, as its heaviest tags. Empty unless the theme is published: the operator's taste is
/// theirs to publish or withhold, and an unpublished theme must not leak through a summary line (§12.1, §8.4).
async fn published_theme_tags(state: &AppState) -> Vec<String> {
    let instance_id = state.config().site.base_url.clone();
    let Ok(Some(theme)) =
        darkhold_db::instance_theme::get_local_theme(state.db(), &instance_id).await
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
    weighted.into_iter().take(6).map(|(tag, _)| tag).collect()
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
    page.void("meta", &[("name", "description"), ("content", tagline)]);
    page.open("title", &[]).text(name).close("title");
    page.void(
        "link",
        &[
            ("rel", "icon"),
            ("href", "/icon.svg"),
            ("type", "image/svg+xml"),
        ],
    );
    if let Some(href) = stylesheet {
        page.void("link", &[("rel", "stylesheet"), ("href", href)]);
    }
    // Every SPA route gets this from the shell; `/` is server-rendered, so it did not, and the browser fell back to
    // requesting /favicon.ico -- which this build answers 404, so the console carried an error on the front page of the
    // site (measured 2026-11-14 by the mobile pass, which is the only pass that visits `/` first).
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
             padding-top:var(--space-4);font-size:var(--text-sm);color:var(--text-secondary)}\
             .landing .shelf-search{display:flex;gap:var(--space-2);margin:0 0 var(--space-6)}\
             .landing .shelf-search input{flex:1;min-width:0;padding:var(--space-2) var(--space-3);border:1px solid var(--border-subtle, rgba(128,128,128,.35));background:var(--surface, transparent);color:inherit;font:inherit}\
             .landing .shelf-search button{padding:var(--space-2) var(--space-4);font:inherit;cursor:pointer}\
             .landing .shelf-list{list-style:none;margin:0;padding:0}\
             .landing .shelf-list li{display:grid;grid-template-columns:2.5rem 1fr;column-gap:var(--space-3);align-items:baseline;border-bottom:1px solid var(--border-subtle, rgba(128,128,128,.3));padding:var(--space-3) 0}\
             .landing .shelf-list .rank{font-family:var(--font-mono);font-size:var(--text-sm);color:var(--text-secondary)}\
             .landing .shelf-list .work-title{font-size:var(--text-lg);text-decoration:none}\
             .landing .shelf-list .meta{grid-column:2;display:flex;flex-wrap:wrap;gap:0 var(--space-3)}\
             .landing .shelf-list .meta span{white-space:nowrap}\
             .landing .shelf-head{display:flex;align-items:baseline;justify-content:space-between;gap:var(--space-3)}\
             .landing .shelf-all{font-size:var(--text-sm);color:var(--text-link,inherit);min-height:44px;display:inline-flex;align-items:center}"
        )
        .close("style");
    page.close("head");

    page.open(
        "body",
        &[("class", "landing"), ("data-darkhold-has-search", "true")],
    );
    page.open("main", &[]);

    // ── the declaration ───────────────────────────────────────────────────
    page.open("h1", &[]).text(name).close("h1");
    if !tagline.trim().is_empty() {
        page.open("p", &[("class", "tagline")])
            .text(tagline)
            .close("p");
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
        page.open("p", &[("class", "counts")])
            .text(&line)
            .close("p");
    }
    if !theme_tags.is_empty() {
        page.open("p", &[("class", "counts")])
            .text("This instance's published taste: ")
            .text(&theme_tags.join(" · "))
            .close("p");
    }

    // The front door is a search, not a paragraph about features. The document posts to the
    // server-rendered search page, so this works with no script at all.
    page.open(
        "form",
        &[
            ("class", "shelf-search"),
            ("action", "/search"),
            ("method", "get"),
        ],
    );
    page.void(
        "input",
        &[
            ("type", "search"),
            ("name", "q"),
            ("placeholder", "Title, author or fandom"),
            ("aria-label", "Search the archive"),
        ],
    );
    page.open("button", &[("type", "submit")])
        .text("Search")
        .close("button");
    page.close("form");

    // ── open now: one ranked list, no cards ───────────────────────────────
    if !starters.is_empty() {
        page.open("section", &[("class", "shelf")]);
        page.open("div", &[("class", "shelf-head")]);
        page.open("h2", &[]).text("Open now").close("h2");
        page.open("a", &[("class", "shelf-all"), ("href", "/search")])
            .text("Browse all")
            .close("a");
        page.close("div");
        page.open("ol", &[("class", "shelf-list")]);
        for (i, starter) in starters.iter().enumerate() {
            page.open("li", &[]);
            page.open("span", &[("class", "rank")])
                .text(&format!("{:02}", i + 1))
                .close("span");
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
            page.open("div", &[("class", "meta")]);
            for part in &meta {
                page.open("span", &[]).text(part).close("span");
            }
            page.close("div");
            page.close("li");
        }
        page.close("ol");
        page.close("section");
    }

    // ── what this place is for ───────────────────────────────────────────
    page.open("section", &[]);
    page.open("h2", &[])
        .text("What you can do here")
        .close("h2");
    page.open("p", &[])
        .text("Reading is free and needs no account. ")
        // THE FRONT DOOR ONLY OFFERS DOORS THAT OPEN. These four links pointed at `/browse`, `/forum`, `/auth/sign-in` and
        // `/about`, and **none of them is a route**: the client answers each with its not-found view inside a 200, so a
        // reader who followed the landing page's own promises landed on "No such page" four times (measured by polaris-qa
        // 2026-11-15; the row was invisible until the crawler learned the phrase). The targets below are the routes the SPA
        // and the server actually serve -- `/search` is the catalogue by fandom, tag and rating, `/community` is the forum
        // hub, `/sign-in` is the SPA's own sign-in page, and `/docs` is help.
        .open("a", &[("href", "/search")])
        .text("Browse the catalogue")
        .close("a")
        .text(" by fandom, tag or rating.");
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
        .open("a", &[("href", "/community")])
        .text("Browse the forum")
        .close("a")
        .text(".");
    page.close("p");
    page.close("section");

    page.open("footer", &[]);
    page.open("a", &[("href", "/sign-in")])
        .text("Sign in")
        .close("a");
    page.text(" · ");
    page.open("a", &[("href", "/docs")]).text("Help").close("a");
    page.close("footer");

    page.close("main");
    page.close("body");
    page.close("html");
    page.finish()
}

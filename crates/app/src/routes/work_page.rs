//! The work page, rendered as a document (ADR 0006, M71-20).
//!
//! A work page is the most-linked page on an archive. It is what a reader sends to a friend, what a link preview
//! unfurls, what a crawler indexes, and what a person lands on from a search result. Until now all four of those got the
//! SPA shell and nothing else.
//!
//! What it renders: the title, the credited people and their roles, the work's own status (rating, completion, length,
//! language, discussion), the summary, its tags as links, its warnings, a table of contents of the readable units and the
//! primary action — *start reading* — pointing at the first readable unit's reader page. Every one of those is a link a
//! reader can follow with JavaScript off.
//!
//! What it refuses to render: a work the reader may not see. `reading_decision` is the same function the API and the
//! reader use, so the page cannot drift from them. A denial is a **404 page**, not a 403 and not a sign-in wall: §8.9
//! says a work's existence is not disclosed, and a page that says "sign in to see this" has disclosed it.
//!
//! What it does not render: the metrics aggregate when the owner has not published it (§9.5), the tags when there are
//! none, the warnings when there are none, the contents when no unit is readable. Absent, not empty (§8.11).

use axum::extract::{OriginalUri, Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::assets;
use crate::auth::MaybeSession;
use crate::html::Html;
use crate::routes::works::{actor_for, parse_work_id, reading_decision_publishing_metadata, Reading};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/works/{id}", get(work_page))
}

async fn work_page(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Path(raw): Path<String>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    // A signed-in reader keeps the application (ADR 0006): this page is the half that has to work without it, and a
    // member's own tools -- kudos, bookmarks, edit -- live in the SPA. The document is the fallback, not the
    // replacement, so a build without assets still answers with content rather than nothing.
    if session.is_some() {
        let shell = assets::serve(State(state.clone()), uri).await;
        if shell.status() == StatusCode::OK {
            return shell;
        }
    }

    let Ok(work_id) = parse_work_id(&raw) else {
        return not_found(&state, "That work does not exist.").await;
    };
    let Ok(Some(work)) = darkhold_db::content::find_work(state.db(), work_id).await else {
        return not_found(&state, "That work does not exist.").await;
    };
    let contributors = darkhold_db::collaboration::contributors_for_work(state.db(), work_id)
        .await
        .unwrap_or_default();
    let actor = actor_for(session.as_ref());

    // The *record* variant: this page hands out metadata, not text, and it is the same decision the public API's
    // work endpoint makes — so the document and the JSON cannot disagree about who may see a work exists.
    match reading_decision_publishing_metadata(&state, actor.as_ref(), &work, &contributors).await {
        Reading::Contributor | Reading::Public => document(&state, &work).await,
        // The record is published and the text is not. The page describes the work and says plainly that its text is
        // held — that is what `works.gated_metadata_public` means, and it is why this arm exists at all.
        Reading::MetadataOnly => withheld(&state, &work).await,
        // Denied covers "does not exist", "not yours" and "sign in": all three answer with the same page, because a
        // distinguishable answer would disclose the work (§8.9).
        Reading::Denied(_) => not_found(&state, "That work does not exist.").await,
    }
}

/// A published work the actor may read.
async fn document(state: &AppState, work: &darkhold_db::content::Work) -> Response {
    let db = state.db();
    let units = darkhold_db::content::units_for_work(db, work.id)
        .await
        .unwrap_or_default();
    let readable: Vec<_> = units
        .iter()
        .filter(|unit| unit.current_revision_id.is_some())
        .collect();
    let words: i64 = readable.iter().map(|unit| unit.word_count).sum();
    let authors = darkhold_db::collaboration::public_contributors(db, work.id)
        .await
        .unwrap_or_default();
    // `tag_names_for_work`, not `tags_for_work`: the latter answers node ids, and a page that printed identifiers
    // where a reader expects words would be worse than one with no tags at all.
    let tags = darkhold_db::taxonomy::tag_names_for_work(db, &work.id.to_string())
        .await
        .unwrap_or_default();
    let warnings = darkhold_db::taxonomy::list_work_warnings(db, &work.id.to_string())
        .await
        .unwrap_or_default();
    let metrics = if work.show_public_ratings {
        darkhold_db::work_metrics::get_metrics(db, &work.id).await.ok()
    } else {
        None
    };

    let stylesheet = assets::app_stylesheet(state).await;
    let site = state.config().site.name.clone();

    let mut page = shell(
        &site,
        &format!("{} — {site}", work.title),
        stylesheet.as_deref(),
    );

    page.open("article", &[("class", "work")]);
    page.open("h1", &[]).text(&work.title).close("h1");

    // ── credits: everybody who wrote it, with the role they wrote it in ──
    if !authors.is_empty() {
        page.open("p", &[("class", "byline")]).text("by ");
        for (index, (handle, display_name, role)) in authors.iter().enumerate() {
            if index > 0 {
                page.text(", ");
            }
            let name = if display_name.trim().is_empty() {
                handle.as_str()
            } else {
                display_name.as_str()
            };
            page.open("a", &[("href", &format!("/people/{handle}"))])
                .text(name)
                .close("a");
            if role != "owner" && role != "author" {
                page.text(&format!(" ({})", role.replace('_', " ")));
            }
        }
        page.close("p");
    }

    // ── status line: only the facts that are set ──
    let mut status = Vec::new();
    if !work.rating.is_empty() {
        status.push(work.rating.replace('_', " "));
    }
    if !work.completion.is_empty() {
        status.push(work.completion.replace('_', " "));
    }
    if words > 0 {
        status.push(format!("{words} words"));
    }
    if readable.len() > 1 {
        status.push(format!("{} chapters", readable.len()));
    }
    if !work.language.is_empty() {
        status.push(work.language.clone());
    }
    if !status.is_empty() {
        page.open("p", &[("class", "status")])
            .text(&status.join(" · "))
            .close("p");
    }

    // ── the primary action, before the summary: a reader who already knows what this is should not scroll ──
    if let Some(first) = readable.first() {
        page.open(
            "p",
            &[("class", "start")],
        )
        .open(
            "a",
            &[
                ("class", "start-link"),
                ("href", &format!("/works/{}/units/{}", work.id, first.id)),
            ],
        )
        .text("Start reading")
        .close("a")
        .close("p");
    }

    if !work.summary.trim().is_empty() {
        page.open("section", &[("class", "summary")]);
        for paragraph in work.summary.split("\n\n") {
            let paragraph = paragraph.trim();
            if !paragraph.is_empty() {
                page.open("p", &[]).text(paragraph).close("p");
            }
        }
        page.close("section");
    }

    if !warnings.is_empty() {
        page.open("section", &[("class", "warnings")]);
        page.open("h2", &[]).text("This work contains").close("h2");
        page.open("ul", &[]);
        for warning in &warnings {
            page.open("li", &[])
                .text(&warning.canonical)
                .text(&format!(" — {}", warning.depiction.replace('_', " ")))
                .close("li");
        }
        page.close("ul");
        page.close("section");
    }

    if !tags.is_empty() {
        page.open("section", &[("class", "tags")]);
        page.open("h2", &[]).text("Tags").close("h2");
        page.open("ul", &[("class", "tag-list")]);
        for tag in &tags {
            page.open(
                "li",
                &[],
            )
            .open("a", &[("href", &tag_href(tag))])
            .text(tag)
            .close("a")
            .close("li");
        }
        page.close("ul");
        page.close("section");
    }

    if !readable.is_empty() {
        page.open("section", &[("class", "contents")]);
        page.open("h2", &[]).text("Contents").close("h2");
        page.open("ol", &[]);
        for unit in &readable {
            let title = unit_title(unit);
            page.open("li", &[])
                .open(
                    "a",
                    &[("href", &format!("/works/{}/units/{}", work.id, unit.id))],
                )
                .text(&title)
                .close("a");
            if unit.word_count > 0 {
                page.text(&format!(" — {} words", unit.word_count));
            }
            page.close("li");
        }
        page.close("ol");
        page.close("section");
    }

    // ── the aggregate, only where the owner published it, and with its counts rather than a score (§9.5) ──
    if let Some(metrics) = metrics {
        let mut parts = Vec::new();
        if metrics.kudos > 0 {
            parts.push(format!("{} kudos", metrics.kudos));
        }
        if metrics.bookmarks > 0 {
            parts.push(format!("{} bookmarks", metrics.bookmarks));
        }
        if metrics.complete_reads > 0 {
            parts.push(format!("{} complete reads", metrics.complete_reads));
        }
        if metrics.reviews > 0 {
            parts.push(format!("{} reviews", metrics.reviews));
        }
        if !parts.is_empty() {
            page.open("p", &[("class", "metrics")])
                .text(&parts.join(" · "))
                .close("p");
        }
    }

    if !work.discussion_mode.is_empty() && work.discussion_mode != "disabled" {
        page.open("p", &[("class", "discussion")])
            .text(&format!(
                "Discussion: {}",
                work.discussion_mode.replace('_', " ")
            ))
            .close("p");
    }

    page.close("article");
    page.close("body");
    page.close("html");
    page_response(page)
}

/// A work whose record is published and whose text is held by the audience gate.
async fn withheld(state: &AppState, work: &darkhold_db::content::Work) -> Response {
    let site = state.config().site.name.clone();
    let stylesheet = assets::app_stylesheet(state).await;
    let mut page = shell(
        &site,
        &format!("{} — {site}", work.title),
        stylesheet.as_deref(),
    );
    page.open("article", &[("class", "work")]);
    page.open("h1", &[]).text(&work.title).close("h1");
    page.open("p", &[("class", "held")])
        .text("This work's text is held. Its record is public; the chapters are released to readers who are signed in and hold the required trust level.")
        .close("p");
    if !work.summary.trim().is_empty() {
        page.open("section", &[("class", "summary")])
            .open("p", &[])
            .text(work.summary.trim())
            .close("p")
            .close("section");
    }
    page.open("p", &[])
        .open("a", &[("href", "/auth/sign-in")])
        .text("Sign in")
        .close("a")
        .close("p");
    page.close("article");
    page.close("body");
    page.close("html");
    page_response(page)
}

async fn not_found(state: &AppState, message: &str) -> Response {
    let site = state.config().site.name.clone();
    let stylesheet = assets::app_stylesheet(state).await;
    let mut page = shell(&site, &format!("Not found — {site}"), stylesheet.as_deref());
    page.open("main", &[]);
    page.open("h1", &[]).text("Not found").close("h1");
    page.open("p", &[]).text(message).close("p");
    page.open("p", &[])
        .open("a", &[("href", "/")])
        .text(&format!("Back to {site}"))
        .close("a")
        .close("p");
    page.close("main");
    page.close("body");
    page.close("html");
    let mut response = (StatusCode::NOT_FOUND, page.finish()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

/// The document as an HTTP response. Everything this module returns goes through here or through `not_found`, so the
/// content type is stated once.
fn page_response(page: Html) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        page.finish(),
    )
        .into_response()
}

/// The document shell: head, tokens, page-local rhythm. No script element — that is the guarantee.
fn shell(site: &str, title: &str, stylesheet: Option<&str>) -> Html {
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
    page.open("title", &[]).text(title).close("title");
    if let Some(href) = stylesheet {
        page.void("link", &[("rel", "stylesheet"), ("href", href)]);
    }
    page.open("style", &[])
        .raw(
            ".doc{max-width:46rem;margin:0 auto;padding:var(--space-6) var(--space-4) var(--space-8);\
             font-family:var(--font-interface);color:var(--text-primary);line-height:var(--leading-normal)}\
             .doc h1{font-family:var(--font-heading);font-size:var(--text-3xl);line-height:var(--leading-tight);margin:0 0 var(--space-2)}\
             .doc h2{font-family:var(--font-heading);font-size:var(--text-lg);margin:var(--space-6) 0 var(--space-2)}\
             .doc p{margin:0 0 var(--space-3)}\
             .doc .byline,.doc .status,.doc .metrics,.doc .discussion{font-family:var(--font-mono);font-size:var(--text-sm);color:var(--text-secondary)}\
             .doc .start-link{display:inline-block;padding:var(--space-2) var(--space-4);border:1px solid currentColor;\
             text-decoration:none;color:var(--text-link, inherit)}\
             .doc .summary p{font-family:var(--font-reading, var(--font-interface));font-size:var(--text-lg)}\
             .doc ul{list-style:none;margin:0;padding:0}\
             .doc .tag-list{display:flex;flex-wrap:wrap;gap:var(--space-2)}\
             .doc .tag-list li{border:1px solid var(--border-subtle, rgba(128,128,128,.35));padding:0 var(--space-2)}\
             .doc ol{margin:0;padding-left:var(--space-5)}\
             .doc ol li{margin:0 0 var(--space-1)}\
             .doc .site{margin:0 0 var(--space-5);font-family:var(--font-mono);font-size:var(--text-sm)}\
             .doc .held{font-size:var(--text-lg)}",
        )
        .close("style");
    page.close("head");
    page.open("body", &[("class", "doc")]);
    page.open("p", &[("class", "site")])
        .open("a", &[("href", "/")])
        .text(site)
        .close("a")
        .close("p");
    page
}

fn unit_title(unit: &darkhold_db::content::Unit) -> String {
    if unit.title.trim().is_empty() {
        format!("Chapter {}", unit.order_key)
    } else {
        unit.title.clone()
    }
}

/// A tag's browse URL. Tags are authored words and may hold spaces; `%20` is the only encoding the browse route needs
/// because it parses the path segment, not a query string.
fn tag_href(tag: &str) -> String {
    let mut out = String::from("/tags/");
    for byte in tag.as_bytes() {
        match byte {
            b' ' => out.push_str("%20"),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'~' | b'.' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

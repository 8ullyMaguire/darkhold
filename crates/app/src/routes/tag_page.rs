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

    // M71-04: one query for the page, and each entry says where its text lives — the same word the API returns.
    let ids: Vec<String> = rows.iter().map(|(id, _, _, _)| id.clone()).collect();
    let provenance = crate::routes::provenance::for_work_ids(&state, &ids).await;

    let mut page = docs::shell(&site, &format!("{shown} — {site}"), stylesheet.as_deref());
    page.open("main", &[]);
    page.open("h1", &[]).text(shown).close("h1");
    // M71-13: the count that matters is the count **of the archive**, not of this page. "12 works" tells a reader the tag is small;
    // "12 of 3,400 works in this archive are tagged slow burn (0.4%)" tells them how much of the archive they are looking at, which is
    // the fact that decides whether to keep browsing.
    let coverage = darkhold_db::taxonomy::dimension_coverage(state.db(), "tag", shown)
        .await
        .ok();
    let mut counts = coverage
        .as_ref()
        .map(|coverage| coverage.sentence.clone())
        .unwrap_or_else(|| {
            if rows.len() == 1 {
                "1 work".to_owned()
            } else {
                format!("{} works", rows.len())
            }
        });
    if more {
        counts.push_str(&format!(
            " Showing the first {PAGE}, most recently updated first."
        ));
    }
    page.open("p", &[("class", "counts")])
        .text(&counts)
        .close("p");

    page.open("ul", &[("class", "entries")]);
    for (id, title, handle, updated) in &rows {
        page.open("li", &[]);
        page.open("a", &[("href", &format!("/works/{id}"))])
            .text(title)
            .close("a");
        let word = provenance
            .get(id)
            .copied()
            .unwrap_or(crate::routes::provenance::Provenance::Known);
        let label = match word {
            crate::routes::provenance::Provenance::Hosted => "readable here",
            crate::routes::provenance::Provenance::HostedLocked => "held",
            crate::routes::provenance::Provenance::Referenced => "elsewhere",
            crate::routes::provenance::Provenance::Known => "no text yet",
        };
        page.open("div", &[("class", "meta")])
            .text(&format!(
                "by {handle} · {} · updated {}",
                label,
                &updated[..10.min(updated.len())]
            ))
            .close("div");
        page.close("li");
    }
    page.close("ul");

    // ── related tags, by NPMI (M69-19) ──────────────────────────────────
    //
    // Absent when there is nothing to relate: a heading with no tags under it is the empty-section failure the other
    // pages are tested against. The word "related" is earned by the statistic, which is why a tag that is on
    // everything does not appear here.
    let related = darkhold_db::taxonomy::related_tags_by_npmi(state.db(), &norm, 12)
        .await
        .unwrap_or_default();
    if !related.is_empty() {
        page.open("section", &[("class", "related")]);
        page.open("h2", &[]).text("Related tags").close("h2");
        page.open("ul", &[("class", "tag-list")]);
        for (name, _npmi, _count) in &related {
            page.open("li", &[])
                .open("a", &[("href", &format!("/tags/{}", encode_tag(name)))])
                .text(name)
                .close("a")
                .close("li");
        }
        page.close("ul");
        page.close("section");
    }

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

/// Percent-encode a tag for a path segment. Spaces are the only character that matters in practice, and `%20` is what
/// the browse route parses.
fn encode_tag(tag: &str) -> String {
    let mut out = String::new();
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

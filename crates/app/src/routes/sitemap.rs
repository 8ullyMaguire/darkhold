//! `/sitemap.xml` — the one list a crawler is invited to read (M71-16).
//!
//! Every other page in this crate answers a reader or an API client; this one answers a machine that will not see a
//! 404 as a denial, a login wall, or a mistake. That makes eligibility a *different* question here: a work that is
//! merely hard to open is not a sitemap entry, because the crawler indexes the URL, not the experience.
//!
//! # What is listed
//!
//! * `/` — the landing page;
//! * `/works/{id}` — published, public, not deleted (`content::sitemap_works`);
//! * `/tags/{tag}` — a tag carried by at least one such work (`content::sitemap_tags`).
//!
//! # What is not
//!
//! Unlisted works. "Unlisted" means reachable by link and not by enumeration, and a sitemap is enumeration wearing a
//! machine's clothes. Private and restricted works are absent for the stronger reason that they are absent from the
//! anonymous browse door: one predicate, not two that drift.
//!
//! # The cap
//!
//! 5,000 works and 2,000 tags, newest change first. Past the cap the file is a *prefix*, not a claim of completeness —
//! and this instance does not reach it. When it does, the answer is a sitemap index, which is a change worth making
//! with the numbers in hand rather than a guess baked in now.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::state::AppState;

const WORK_LIMIT: i64 = 5_000;
const TAG_LIMIT: i64 = 2_000;

pub fn router() -> Router<AppState> {
    Router::new().route("/sitemap.xml", get(sitemap))
}

/// The sitemap. Anonymous by construction: the body never varies by caller, which is why it takes no session
/// extractor at all (`Audience::Anonymous` in the route table).
pub async fn sitemap(State(state): State<AppState>) -> Response {
    let base = state.config().site.base_url.trim_end_matches('/');
    let db = state.db();

    let works = darkhold_db::content::sitemap_works(db, WORK_LIMIT)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "sitemap: works query failed; emitting the root only");
            Vec::new()
        });
    let tags = darkhold_db::content::sitemap_tags(db, TAG_LIMIT)
        .await
        .unwrap_or_default();

    let mut body = String::with_capacity(256 + works.len() * 120 + tags.len() * 80);
    body.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    body.push_str("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    body.push_str(&url_entry(base, "/", None));
    for (id, updated_at) in &works {
        // `updated_at` is stored as an ISO timestamp with a `Z`; the sitemap wants a date or a full timestamp, and
        // passing the stored value through is more honest than reformatting a date we did not parse.
        let lastmod = if updated_at.len() >= 10 {
            Some(&updated_at[..10])
        } else {
            None
        };
        body.push_str(&url_entry(base, &format!("/works/{id}"), lastmod));
    }
    for tag in &tags {
        body.push_str(&url_entry(base, &format!("/tags/{}", encode_tag(tag)), None));
    }
    body.push_str("</urlset>\n");

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        body,
    )
        .into_response()
}

fn url_entry(base: &str, path: &str, lastmod: Option<&str>) -> String {
    let mut entry = String::with_capacity(path.len() + 64);
    entry.push_str("  <url><loc>");
    entry.push_str(&escape(&format!("{base}{path}")));
    entry.push_str("</loc>");
    if let Some(date) = lastmod {
        entry.push_str("<lastmod>");
        entry.push_str(&escape(date));
        entry.push_str("</lastmod>");
    }
    entry.push_str("</url>\n");
    entry
}

/// Percent-encode a tag for a path segment, the same rule the tag page's links use.
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

/// XML text escaping. A URL from configuration could contain `&` or `<`; the ids cannot, but the base URL is operator
/// input and an unescaped ampersand makes the whole document unparseable rather than wrong in one place.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

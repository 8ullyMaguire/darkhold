//! The shared shell for pages the server renders as documents (ADR 0006).
//!
//! First caller was the landing page, then the work page, and the tag page is the third — so the shell lives here
//! rather than three times in three modules. What is shared is only what a document needs to *be* a document: a head
//! with a charset, a viewport, a title, the application's own stylesheet, one style block for the page's rhythm, and a
//! body class that keeps that style from reaching the application. What is not shared is any layout decision: the
//! heading, the sections and the words belong to the page.
//!
//! **Nothing here writes a `<script>` element.** That is the guarantee these pages are tested against, and putting the
//! shells in one place is what keeps it a guarantee rather than three habits.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::html::Html;

/// The document frame. Callers open their content, then close `body` and `html` themselves.
pub(crate) fn shell(site: &str, title: &str, stylesheet: Option<&str>) -> Html {
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
             .doc .byline,.doc .status,.doc .metrics,.doc .discussion,.doc .counts{font-family:var(--font-mono);font-size:var(--text-sm);color:var(--text-secondary)}\
             .doc .start-link{display:inline-block;padding:var(--space-2) var(--space-4);border:1px solid currentColor;\
             text-decoration:none;color:var(--text-link, inherit)}\
             .doc .summary p{font-family:var(--font-reading, var(--font-interface));font-size:var(--text-lg)}\
             .doc ul{list-style:none;margin:0;padding:0}\
             .doc .tag-list{display:flex;flex-wrap:wrap;gap:var(--space-2)}\
             .doc .tag-list li{border:1px solid var(--border-subtle, rgba(128,128,128,.35));padding:0 var(--space-2)}\
             .doc ol{margin:0;padding-left:var(--space-5)}\
             .doc ol li,.doc .entries li{margin:0 0 var(--space-2)}\
             .doc .site{margin:0 0 var(--space-5);font-family:var(--font-mono);font-size:var(--text-sm)}\
             .doc .held{font-size:var(--text-lg)}\
             .doc .entries .meta{font-family:var(--font-mono);font-size:var(--text-sm);color:var(--text-secondary)}",
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

/// A finished document, as a response.
pub(crate) fn page_response(page: Html) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        page.finish(),
    )
        .into_response()
}

/// The one page every refusal renders: a 404 with the same shape for "does not exist" and "may not see it".
///
/// §8.9 again — but stated once here, so a fourth document page cannot invent its own distinguishable answer.
pub(crate) fn not_found(site: &str, message: &str, stylesheet: Option<&str>) -> Response {
    let mut page = shell(site, &format!("Not found — {site}"), stylesheet);
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

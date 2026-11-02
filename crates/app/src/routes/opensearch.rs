//! The OpenSearch description document (M69-25, suki spec §7.4).
//!
//! A reader who searches one fandom here every day should be able to type that
//! fandom into the address bar and land on this instance's results. That is the
//! whole feature: one XML document and one `<link rel="search">` in the shell.
//!
//! It exists because it is the kind of row that never gets scheduled — never
//! urgent, never broken, and cheap enough that writing it takes less time than
//! arguing about whether it belongs on the roadmap.
//!
//! Three decisions worth stating, because each is a way this goes wrong:
//!
//! * **The template points at the HTML page, not the JSON endpoint.** OpenSearch
//!   is a browser feature: the browser navigates, so a JSON response would show
//!   a reader a wall of braces.
//! * **The instance's own name is the short name**, not a constant. A document
//!   that says "Darkhold" on every instance is a document that lies about the
//!   instance the reader is on.
//! * **The document is escaped, not interpolated.** An operator may name their
//!   site `Fic & Fandom`; unescaped, that document is not XML and the browser
//!   silently refuses to add the engine, which looks like the feature never
//!   existed.

use crate::auth::MaybeSession;
use crate::state::AppState;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

/// The path the shell links to. Kept as a constant because the frontend cannot
/// read Rust, and the two spellings have to agree.
pub const PATH: &str = "/opensearch.xml";

pub fn router() -> Router<AppState> {
    Router::new().route(PATH, get(document))
}

/// Escape the five characters XML reserves.
///
/// `&` first: escaping it after the others would re-escape the ampersands they
/// introduce, and an operator whose name contains `&` would get a document that
/// parses to the wrong text rather than one that fails.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The description document for one instance.
///
/// `base_url` is the operator's configured public origin, with no trailing
/// slash (the config loader trims it). The search page is `/search`, which is
/// the reader-facing HTML surface, not the `/api/v1/search` projection.
pub fn describe(name: &str, base_url: &str) -> String {
    let name = escape(name);
    let base = escape(base_url.trim_end_matches('/'));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
  <ShortName>{name}</ShortName>
  <Description>Search {name}</Description>
  <InputEncoding>UTF-8</InputEncoding>
  <Url type="text/html" method="get" template="{base}/search?q={{searchTerms}}"/>
</OpenSearchDescription>
"#
    )
}

async fn document(State(state): State<AppState>, MaybeSession(_): MaybeSession) -> Response {
    let config = state.config();
    let body = describe(&config.site.name, &config.site.base_url);
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/opensearchdescription+xml; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_is_the_html_search_page() {
        let document = describe("Darkhold", "https://darkhold.example");
        assert!(
            document.contains(r#"template="https://darkhold.example/search?q={searchTerms}""#),
            "{document}"
        );
        // The JSON projection would render braces at a reader who typed a fandom
        // into their address bar.
        assert!(!document.contains("/api/v1/search"), "{document}");
    }

    #[test]
    fn the_instance_names_itself() {
        let document = describe("Sakura Archive", "https://sakura.example");
        assert!(document.contains("<ShortName>Sakura Archive</ShortName>"));
        assert!(document.contains("<Description>Search Sakura Archive</Description>"));
    }

    #[test]
    fn a_name_with_xml_characters_still_parses() {
        let document = describe("Fic & Fandom <beta>", "https://example.test");
        assert!(
            document.contains("Fic &amp; Fandom &lt;beta&gt;"),
            "{document}"
        );
        // The only raw ampersands in a valid document are the ones introducing
        // entities.
        let raw = document
            .replace("&amp;", "")
            .replace("&lt;", "")
            .replace("&gt;", "");
        assert!(!raw.contains('&'), "{document}");
        assert!(!document.contains("<beta>"), "{document}");
    }

    #[test]
    fn a_trailing_slash_does_not_double() {
        let document = describe("Darkhold", "https://darkhold.example/");
        assert!(
            document.contains("https://darkhold.example/search"),
            "{document}"
        );
        assert!(!document.contains("example//search"), "{document}");
    }
}

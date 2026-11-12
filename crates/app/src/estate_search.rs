//! The estate index, as a client of `polaris-search`.
//!
//! darkhold has its own catalogue search (`routes/search.rs`): it knows works, pseuds, content filters and saved
//! defaults, and it is the right answer to "find a work in this catalogue". It cannot answer "does anything in
//! the estate match this", because it only has this catalogue — gravity's 40,000 posts live in a different
//! database, and a second connection to it from here is what ADR 0001 refused.
//!
//! So this module is a proxy, deliberately thin: the service's own JSON passed through, and the service kept on
//! loopback where it belongs. The browser never learns its address, which is why the palette can call this route
//! from any page without a CORS exception and without a second credential in the page.
//!
//! The transport used to live here — the encoding, the 2500 ms timeout and the two error paths, all measured in
//! this file before they were measured anywhere else. It is now `polaris-search-client`, so the other four
//! products inherit those measurements instead of repeating the mistakes that produced them. What stays here is
//! what is darkhold's to decide:
//!
//! - a palette that cannot reach the index shows nothing and says nothing. The reader types, sees no suggestions,
//!   and presses Enter; an error banner on every keystroke is noise, not information.
//! - a real search that cannot reach the index answers 503 and says so. The reader asked a question and deserves
//!   an answer that is not a lie about having none.
//!
//! Configuration is environment, like `taste.rs`, because the URL is deployment, not product: a site shipped to
//! another host points at a different service and must not need a code change.
//!
//!   DARKHOLD_SEARCH_URL   default http://127.0.0.1:8060

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use polaris_search_client::SearchClient;
use serde::Deserialize;
use serde_json::json;

/// A client labelled for the service's query log.
///
/// This used to be a single `get()` that labelled *everything* `palette` — so a reader pressing Enter on a real
/// search was logged as a keystroke, and the log an operator reads to see "what people asked" answered with
/// completions. Two routes, two labels.
fn client(instrument: &str) -> SearchClient {
    SearchClient::from_env_var("DARKHOLD_SEARCH_URL").with_instrument(instrument)
}

#[derive(Debug, Deserialize)]
pub struct SuggestParams {
    pub q: String,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Restrict to one site, for a caller that wants this catalogue only. Absent means the estate.
    #[serde(default)]
    pub site: Option<String>,
}

/// `GET /api/v1/estate/suggest?q=&limit=&site=` — completions from the estate index.
pub async fn suggestions(q: &str, limit: Option<u32>, site: Option<&str>) -> Response {
    let q = q.trim();
    if q.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"code": "invalid_request", "message": "q must not be empty"}})),
        )
            .into_response();
    }
    // Eight is the palette's own number: a list long enough to scroll is a search result wearing another name.
    let limit = limit.unwrap_or(8).clamp(1, 20);
    match client("palette").suggest(q, site, limit).await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "estate suggest unavailable");
            (
                StatusCode::OK,
                Json(json!({"data": [], "unavailable": true})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub site: Option<String>,
}

/// `GET /api/v1/estate/search?q=&limit=&site=` — the estate's ranked rows, passed through untouched.
pub async fn rows(q: &str, limit: Option<u32>, site: Option<&str>) -> Response {
    let q = q.trim();
    if q.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"code": "invalid_request", "message": "q must not be empty"}})),
        )
            .into_response();
    }
    let limit = limit.unwrap_or(10).clamp(1, 50);
    // Raw, not typed: the body is the service's, and a proxy that re-serialises a typed model drops `why`,
    // `fallback` and `cross_site` — the three fields that let a caller explain the answer.
    match client("search").query_raw(q, site, None, limit).await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "estate search unavailable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": {"code": "unavailable", "message": "the estate index is not answering"}})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        serde_json::from_slice(&bytes).expect("json")
    }

    #[tokio::test]
    async fn an_empty_query_is_refused_by_both_routes_without_touching_the_index() {
        // Both handlers refuse before a client is used, so this test passes with the index stopped — which is the
        // point: a 500 here would mean darkhold blamed the index for a caller's empty box.
        for response in [
            rows("   ", None, None).await,
            suggestions("", None, None).await,
        ] {
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(
                body_json(response).await["error"]["code"],
                "invalid_request"
            );
        }
    }
}

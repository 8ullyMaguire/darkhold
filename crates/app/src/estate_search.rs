//! The estate index, as a client of `polaris-search`.
//!
//! darkhold has its own catalogue search (`routes/search.rs`): it knows works, pseuds, content filters and saved
//! defaults, and it is the right answer to "find a work in this catalogue". It cannot answer "does anything in
//! the estate match this", because it only has this catalogue — gravity's 40,000 posts live in a different
//! database, and a second connection to it from here is what ADR 0001 refused.
//!
//! So this module is a proxy, deliberately thin: one GET, the service's own JSON passed through, and the service
//! kept on loopback where it belongs. The browser never learns its address, which is why the palette can call
//! this route from any page without a CORS exception and without a second credential in the page.
//!
//! Configuration is environment, like `taste.rs`, because the URL is deployment, not product: a site shipped to
//! another host points at a different service and must not need a code change.
//!
//!   DARKHOLD_SEARCH_URL   default http://127.0.0.1:8060

use std::time::Duration;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_URL: &str = "http://127.0.0.1:8060";
/// A palette that waits longer than this is a palette the reader has stopped using. Measured, not guessed: the
/// first deployed version used 1200 ms and the very first request after a restart timed out at 1200.6 ms — the
/// index's cold plan over 98k documents is the slow case, and a warm query answers in 87 ms. 2500 ms covers the
/// cold plan and is still shorter than a reader's patience for a completion list.
const TIMEOUT: Duration = Duration::from_millis(2500);

fn base_url() -> String {
    std::env::var("DARKHOLD_SEARCH_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
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

/// Percent-encode a query value. Percent-encoding is not optional here: a work title contains `&`, `'` and `#`
/// routinely, and a proxy that concatenates instead of encoding turns a search for "Ron & Hermione" into a
/// search for "Ron ".
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// One GET against the search service, with the timeout a palette can afford.
async fn get(path: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| format!("client: {e}"))?;
    let response = client
        .get(format!("{}{}", base_url(), path))
        // The service logs queries so an operator can see what people asked. This call is a palette
        // completion, not a reader's search, and says so — the same discipline that removed gate probes from
        // the weekly report.
        .header("x-search-instrument", "palette")
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| format!("body: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "service answered {status}: {}",
            body.chars().take(160).collect::<String>()
        ));
    }
    serde_json::from_str(&body).map_err(|e| format!("json: {e}"))
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
    let mut path = format!("/api/v1/search/suggest?q={}&limit={limit}", encode(q));
    if let Some(site) = site {
        path.push_str(&format!("&site={}", encode(site)));
    }
    match get(&path).await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        // A palette whose index is down shows nothing and says nothing: the reader types, sees no suggestions,
        // and presses Enter. An error banner for a completion list would be noise on every keystroke.
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

/// `GET /api/v1/estate/search?q=&limit=&site=` — the estate's ranked rows, passed through.
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
    let mut path = format!("/api/v1/search?q={}&limit={limit}", encode(q));
    if let Some(site) = site {
        path.push_str(&format!("&site={}", encode(site)));
    }
    match get(&path).await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        // Unlike the palette, a real search that cannot reach the index says so: the reader asked a question
        // and deserves an answer that is not a lie about having none.
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
    use super::encode;

    #[test]
    fn encode_keeps_titles_from_becoming_different_queries() {
        // "Ron & Hermione" unencoded asks the index for "Ron " — the proxy must not be the place a query
        // silently loses its second half.
        assert_eq!(encode("Ron & Hermione"), "Ron+%26+Hermione");
        assert_eq!(encode("it's"), "it%27s");
        assert_eq!(encode("100%"), "100%25");
        assert_eq!(encode("plain"), "plain");
    }
}

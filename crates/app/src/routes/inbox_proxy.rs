//! The reader's side of the estate inbox: a count and a stream, both for the signed-in person.
//!
//! The browser cannot call the inbox: it holds a session cookie, and the inbox wants an assertion signed with a
//! secret that must never reach a page. So these two routes stand in front of it — `RequireSession` names the
//! person, `inbox::subject_assertion` proves it to the service, and the response comes back unchanged.
//!
//! The stream is passed through as a stream, never collected: it does not end.

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};

use crate::auth::RequireSession;
use crate::http::ApiResult;
use crate::inbox;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/inbox/unread-count", get(unread_count))
        .route("/inbox/stream", get(stream))
        .route("/inbox/read", post(mark_all_read))
}

async fn unread_count(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    // A bell that cannot reach the inbox shows zero rather than an error: an error banner where a number
    // belongs is worse than a number that is briefly wrong.
    let subject = inbox::canonical(state.db(), &user.account_id.to_string()).await;
    match inbox::unread_count(&subject).await {
        Ok(count) => Ok(Json(count)),
        Err(error) => {
            tracing::warn!(%error, "the inbox did not answer the unread count");
            Ok(Json(
                serde_json::json!({ "total": 0, "by_site": {}, "unavailable": true }),
            ))
        }
    }
}

async fn stream(State(state): State<AppState>, RequireSession(user): RequireSession) -> Response {
    let subject = inbox::canonical(state.db(), &user.account_id.to_string()).await;
    match inbox::stream(&subject).await {
        Ok(upstream) if upstream.status().is_success() => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache, no-transform")
            // nginx buffers proxied responses by default, which turns a live stream into a burst every 4 kB.
            .header("x-accel-buffering", "no")
            .body(Body::from_stream(upstream.bytes_stream()))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Ok(upstream) => {
            let status = upstream.status();
            tracing::warn!(%status, "the inbox refused the stream");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "the inbox is not streaming",
            )
                .into_response()
        }
        Err(error) => {
            tracing::warn!(%error, "the inbox did not answer the stream");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "the inbox is not answering",
            )
                .into_response()
        }
    }
}

async fn mark_all_read(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    let subject = inbox::canonical(state.db(), &user.account_id.to_string()).await;
    match inbox::mark_all_read(&subject).await {
        Ok(marked) => Ok(Json(serde_json::json!({ "marked": marked }))),
        Err(error) => {
            tracing::warn!(%error, "the inbox did not accept a mark-all");
            Ok(Json(
                serde_json::json!({ "marked": 0, "unavailable": true }),
            ))
        }
    }
}

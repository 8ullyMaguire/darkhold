//! W6 §7.5 — the ATProto feed-generator endpoint.
//!
//! `GET /xrpc/app.bsky.feed.getFeedSkeleton?feed=at://…&limit=…&cursor=…`
//!
//! # Why this lives at the root and not under `/api/v1`
//!
//! `/xrpc/` is the ATProto RPC namespace: a consumer builds the URL from the DID document, and the path belongs to the
//! protocol, not to this site's API. It is mounted before the SPA fallback for a measured reason — `/xrpc/…` answered
//! `404 text/html` (`<title>Not found — Darkhold</title>`), and a feed generator that answers a skeleton request with an
//! HTML page is unparseable to every consumer while looking like an ordinary 404 on this side.
//!
//! # Slice A: the shape before the content
//!
//! Every exit is JSON, including the one for a feed that does not exist, because the error shape is what a consumer
//! depends on when something is wrong. The three feeds answer `UnknownFeed` here; slice B fills them in.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::auth::MaybeSession;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SkeletonQuery {
    /// `at://did/app.bsky.feed.generator/rkey` — the feed the consumer subscribed to.
    pub feed: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/xrpc/app.bsky.feed.getFeedSkeleton", get(get_feed_skeleton))
}

/// The skeleton, or a JSON reason why not.
///
/// `feed` absent is `UnknownFeed` rather than a bare bad request: the lexicon marks the parameter required, but a
/// consumer that omits it is asking for a feed by no name, and `UnknownFeed` says that in one word.
/// `MaybeSession` rather than nothing: the route is public, and taking the extractor is what makes that a claim the
/// route table can check (`every_route_has_correct_audience`). The viewer is unused until a slice needs to filter a
/// skeleton by blocks or mutes — at which point the door already carries the answer.
async fn get_feed_skeleton(
    State(_state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Query(_q): Query<SkeletonQuery>,
) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "UnknownFeed", "message": "this generator serves no such feed" })),
    )
        .into_response()
}

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
//! # Three feeds, and no invented ranking
//!
//! `darkhold-recent`, `darkhold-updated`, `darkhold-tagged`. Each is an ordering the site already produces; there is no
//! `top`, because measured, the product's only ranked listing is `library_items ORDER BY likes DESC` — per reader — and
//! `work_metric_aggregates` is counted but never ordered by. A `top` feed would invent that rule for Bluesky.
//!
//! # Every exit is JSON
//!
//! A protocol consumer depends on the error shape when something is wrong: `{"error":"UnknownFeed"}` is actionable,
//! `<html>` is not, and the SPA fallback must never be able to answer this path.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use darkhold_db::atproto::Feed;

use crate::auth::MaybeSession;
use crate::state::AppState;

/// The record key of each feed. A consumer asks for `at://<our did>/app.bsky.feed.generator/<rkey>`, so the rkey is the
/// whole identity of a feed on this side.
const FEED_RECENT: &str = "darkhold-recent";
const FEED_UPDATED: &str = "darkhold-updated";
const FEED_TAGGED: &str = "darkhold-tagged";

#[derive(Debug, Deserialize)]
pub struct SkeletonQuery {
    /// `at://did/app.bsky.feed.generator/rkey` — the feed the consumer subscribed to.
    pub feed: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/xrpc/app.bsky.feed.getFeedSkeleton",
        get(get_feed_skeleton),
    )
}

/// The feed an `at://` URI names, or `None` when it names one this generator does not serve.
///
/// The collection is checked as well as the rkey: a URI in another collection is not a feed whatever its last segment
/// says, and answering it would claim to serve records this endpoint has never heard of.
fn feed_for_uri(uri: &str) -> Option<Feed> {
    if !uri.contains("/app.bsky.feed.generator/") {
        return None;
    }
    match uri.rsplit('/').next().unwrap_or_default() {
        FEED_RECENT => Some(Feed::Recent),
        FEED_UPDATED => Some(Feed::Updated),
        FEED_TAGGED => Some(Feed::Tagged),
        _ => None,
    }
}

/// A cursor is the ordering value of the last item shown, and the column stores it as `YYYY-MM-DDTHH:MM:SS`.
///
/// Validated rather than passed through: the column is TEXT, so a cursor of `zzz` is a perfectly good string that compares
/// below every real timestamp — the consumer would then page from the beginning forever, and the loop would read as a
/// slow feed rather than a refusal.
fn valid_cursor(cursor: &str) -> bool {
    let b = cursor.as_bytes();
    b.len() == 19
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b.iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16) || c.is_ascii_digit())
}

fn unknown_feed(why: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "UnknownFeed", "message": why })),
    )
        .into_response()
}

/// The skeleton, or a JSON reason why not.
///
/// `feed` absent is `UnknownFeed` rather than a bare bad request: the lexicon marks the parameter required, but a
/// consumer that omits it is asking for a feed by no name, and `UnknownFeed` says that in one word.
/// `MaybeSession` rather than nothing: the route is public, and taking the extractor is what makes that a claim the
/// route table can check (`every_route_has_correct_audience`). The viewer is unused until a slice needs to filter a
/// skeleton by blocks or mutes — at which point the door already carries the answer.
async fn get_feed_skeleton(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Query(q): Query<SkeletonQuery>,
) -> Response {
    let Some(uri) = q.feed.as_deref() else {
        return unknown_feed("no feed was named");
    };
    let Some(feed) = feed_for_uri(uri) else {
        return unknown_feed("this generator serves no such feed");
    };
    // The lexicon's own bounds: a page larger than 100 is refused by consumers anyway, and one smaller than 1 asks for
    // nothing.
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let cursor = match q.cursor.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(c) if valid_cursor(c) => Some(c),
        Some(c) => {
            return unknown_feed(&format!(
                "cursor {c:?} is not a timestamp this feed can resume from"
            ))
        }
    };

    match darkhold_db::atproto::feed_page(state.db(), feed, limit, cursor).await {
        Ok(items) => {
            // Absent when the page is empty: a consumer that receives a cursor for an empty page asks again forever.
            let next = items.last().map(|i| i.cursor.clone());
            let feed: Vec<_> = items.iter().map(|i| json!({ "post": i.at_uri })).collect();
            Json(json!({ "cursor": next, "feed": feed })).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "the feed skeleton could not be built");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "InternalServerError", "message": "the skeleton could not be built" })),
            )
                .into_response()
        }
    }
}

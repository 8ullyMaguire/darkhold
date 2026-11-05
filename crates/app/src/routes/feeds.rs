//! M63-14 — a saved query as a feed.
//!
//! ```text
//! GET /saved-views/:id/feed?format=atom|rss|json
//! ```
//!
//! # Why this exists
//!
//! A power user who lives in a feed reader should not have to open the site to see whether anything new
//! matched their saved query. The view is already stored, already scoped and already runnable; a feed is the
//! same query with a different renderer. `M63-06` built the views, `M63-14` is the second client.
//!
//! # The three decisions worth naming
//!
//! * **A 304 is the feature.** Every render carries a strong `ETag` computed from the bytes, and
//!   `If-None-Match` answers `304` with no body. A reader polling forty views every twenty minutes should cost
//!   forty tiny responses, not forty documents — and a feed that re-stamps "now" on every poll would defeat
//!   this by construction, which is why an item with no date prints no date (`darkhold_domain::feed`).
//! * **Both view kinds render.** A library view runs its stored `LibraryQuery`; a catalogue view runs its
//!   canonical query string through the same `search_raw` the catalogue route uses. One execution path per
//!   kind, not a second query engine.
//! * **A feed reader cannot set an `Authorization` header**, so `?token=` is accepted here and **only**
//!   here, rewritten into the header the scoped extractor already understands. That keeps one auth path:
//!   nothing in this module parses a token itself.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{middleware, Router};
use serde::Deserialize;

use darkhold_db::library::{self, SavedViewKind};
use darkhold_domain::feed::{self, Feed, FeedItem, Format};

use crate::auth::RequireActorScoped;
use crate::http::{ApiError, ApiResult};
use darkhold_domain::error::AppError;

/// A 422 with a sentence, in the shape the rest of the API answers with.
fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}
use crate::routes::catalogue::{self, CatalogueParams};
use crate::state::AppState;
use darkhold_domain::api_scopes::Scope;

/// How many items a feed carries by default. A reader that wants more asks with `?limit=`.
const DEFAULT_LIMIT: i64 = 50;
/// The most a feed will carry, whatever it asks for.
const MAX_LIMIT: i64 = 200;

/// The feed routes.
///
/// Mounted beside the saved-view routes but behind [`token_from_query`], because this is the one surface a
/// client without headers polls.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/saved-views/{id}/feed", get(saved_view_feed))
        .layer(middleware::from_fn(token_from_query))
}

#[derive(Debug, Deserialize)]
struct FeedParams {
    /// `atom` (default), `rss` or `json`.
    format: Option<String>,
    /// How many items. Clamped, never refused: a feed reader asking for 500 gets [`MAX_LIMIT`].
    limit: Option<i64>,
    /// A personal access token, for clients that cannot send a header.
    ///
    /// Read by [`token_from_query`], which runs before the handler — so this field is deliberately
    /// unused here: the handler sees an ordinary `Authorization` header and one code path checks tokens.
    #[allow(dead_code)]
    token: Option<String>,
}

/// Rewrite `?token=` into `Authorization: Bearer`.
///
/// A feed reader has no field for an auth header, so the token travels in the URL. Doing it here rather than
/// in the handler means the handler sees exactly the request a browser sends, and `RequireActorScoped`
/// stays the only thing that knows what a token is.
async fn token_from_query(mut request: axum::extract::Request, next: middleware::Next) -> Response {
    if request.headers().get(header::AUTHORIZATION).is_none() {
        let token = request
            .uri()
            .query()
            .and_then(|query| {
                url::form_urlencoded::parse(query.as_bytes())
                    .find(|(key, _)| key == "token")
                    .map(|(_, value)| value.into_owned())
            })
            .filter(|token| !token.trim().is_empty());
        if let Some(token) = token {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", token.trim())) {
                request.headers_mut().insert(header::AUTHORIZATION, value);
            }
        }
    }
    next.run(request).await
}

async fn saved_view_feed(
    State(state): State<AppState>,
    RequireActorScoped { actor }: RequireActorScoped,
    Path(id): Path<String>,
    Query(params): Query<FeedParams>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let actor = actor.require(Scope::LibraryRead)?;
    let account = actor.account_id.to_string();

    let format = match params
        .format
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
    {
        None => Format::Atom,
        Some(raw) => Format::parse(raw)
            .ok_or_else(|| invalid(format!("format: expected atom, rss or json; got {raw}")))?,
    };
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let view = library::find_saved_view(state.db(), &account, &id)
        .await?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "saved view",
            })
        })?;

    let items = match view.kind {
        SavedViewKind::Library => library_items(&state, &account, &view, limit).await?,
        SavedViewKind::Catalogue => catalogue_items(&state, &view, limit).await?,
    };

    let document = Feed {
        title: view.name.clone(),
        self_url: format!(
            "/api/v1/saved-views/{id}/feed?format={}",
            format.extension()
        ),
        link: "/library".to_owned(),
        items,
    };
    let body = feed::render(&document, format);
    let tag = feed::etag(&body);

    // The whole point: unchanged, the answer is three headers and no bytes.
    if let Some(sent) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
    {
        if feed::matches(sent, &tag) {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            set_common_headers(response.headers_mut(), &tag, format);
            return Ok(response);
        }
    }

    let mut response = body.into_response();
    set_common_headers(response.headers_mut(), &tag, format);
    Ok(response)
}

fn set_common_headers(headers: &mut HeaderMap, tag: &str, format: Format) {
    if let Ok(value) = HeaderValue::from_str(format.content_type()) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = HeaderValue::from_str(tag) {
        headers.insert(header::ETAG, value);
    }
    // `private`: a feed of a reader's library is theirs. `must-revalidate`: a stale feed is worse than a
    // slow one, and the tag makes revalidating free.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=0, must-revalidate"),
    );
}

/// A library view's items, through the same store function the page uses.
async fn library_items(
    state: &AppState,
    account: &str,
    view: &library::SavedView,
    limit: i64,
) -> ApiResult<Vec<FeedItem>> {
    let query = view.query.as_ref().ok_or_else(|| {
        invalid(format!(
                "saved view {} holds a query this build cannot read; open it in the interface to repair it",
            view.id
        ))
    })?;
    let page = library::query_library(state.db(), account, query, limit, 0).await?;
    Ok(page
        .items
        .into_iter()
        .map(|item| FeedItem {
            id: item.id,
            title: item.title,
            link: item.source_url,
            // The source's own clock, never ours: `last_synced_at` says when we looked, and a feed that
            // dated items by when we polled would move every item on every poll.
            updated: item.source_updated_at,
            summary: item.summary,
            author: item.author_text,
        })
        .collect())
}

/// A catalogue view's items, through the same page the catalogue route serves.
async fn catalogue_items(
    state: &AppState,
    view: &library::SavedView,
    limit: i64,
) -> ApiResult<Vec<FeedItem>> {
    // The feed carries the view's *filter*, not its page size: the stored `limit` is what the view
    // answered when it was saved, and the client asking for a feed says how many entries it wants. So the
    // stored `limit` is dropped and the feed's is appended — twice would make the query unparseable, which
    // is how this was found.
    let mut rebuilt: Vec<(String, String)> =
        url::form_urlencoded::parse(view.params.as_deref().unwrap_or("").trim().as_bytes())
            .filter(|(key, _)| key != "limit")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
    rebuilt.push(("limit".to_owned(), limit.min(100).to_string()));
    let stored = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(&rebuilt)
        .finish();
    let uri = format!("/catalogue?{stored}")
        .parse::<axum::http::Uri>()
        .map_err(|error| {
            invalid(format!(
                "saved view {} holds an unreadable catalogue query: {error}",
                view.id
            ))
        })?;
    // axum's own `Query` decodes the stored text, so the feed and the route parse a saved query the
    // same way. `try_from_uri` is the constructor it uses itself.
    let params: CatalogueParams = axum::extract::Query::try_from_uri(&uri)
        .map(|query| query.0)
        .map_err(|error| {
            invalid(format!(
                "saved view {} holds an unreadable catalogue query: {error}",
                view.id
            ))
        })?;
    let page = catalogue::search_raw(state.clone(), params).await?;
    Ok(page
        .items
        .into_iter()
        .map(|row| FeedItem {
            id: row.id,
            title: row.title,
            link: row.source_url,
            // The catalogue row carries no source clock; the feed says so rather than inventing one.
            updated: None,
            summary: row.summary,
            author: row.author_text,
        })
        .collect())
}

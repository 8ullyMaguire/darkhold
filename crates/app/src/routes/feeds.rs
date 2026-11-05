//! Feeds: a saved query as a document a reader can poll, and the public catalogue as one.
//!
//! ```text
//! GET /saved-views/:id/feed?format=atom|rss|json     (owner only, `?token=` accepted here)
//! GET /catalogue/feed?format=atom|rss|json&<filters> (public)
//! ```
//!
//! # Why this exists
//!
//! A power user who lives in a feed reader should not have to open the site to see whether anything new
//! matched their saved query. The view is already stored, already scoped and already runnable; a feed is the
//! same query with a different renderer. `M63-06` built the views, `M63-14` is the second client. The
//! catalogue feed is the same idea for the one surface a reader *without* an account can poll.
//!
//! # The decisions worth naming
//!
//! * **A 304 is the feature.** Every render carries a strong `ETag` computed from the bytes, and
//!   `If-None-Match` answers `304` with no body. A reader polling forty views every twenty minutes should cost
//!   forty tiny responses, not forty documents — and a feed that re-stamps "now" on every poll would defeat
//!   this by construction, which is why an item with no date prints no date (`commons_feed`).
//! * **One renderer estate-wide.** `commons_feed` does the XML — escaping, markdown stripping, the three
//!   shapes. Until 2026-11-05 darkhold had three renderers for this document and used two of them, one of
//!   which (`darkhold_domain::feeds`) was called by a test and by nothing else. What stays here is the HTTP
//!   half — format, content type, `ETag`, `If-None-Match` (`crate::feed_http`) — because four products have
//!   four cache policies and a shared renderer must not decide one of them.
//! * **Both view kinds render.** A library view runs its stored `LibraryQuery`; a catalogue view runs its
//!   canonical query string through the same `search_raw` the catalogue route uses. One execution path per
//!   kind, not a second query engine — and the catalogue feed goes through that same call too.
//! * **A feed reader cannot set an `Authorization` header**, so `?token=` is accepted here and **only**
//!   here, rewritten into the header the scoped extractor already understands. Nothing in this module parses
//!   a token itself.

use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{middleware, Router};
use chrono::{DateTime, Utc};
use commons_feed::{FeedEntry, FeedMeta};
use serde::Deserialize;

use darkhold_db::library::{self, SavedViewKind};

use crate::auth::{MaybeSession, RequireActorScoped};
use crate::feed_http::{self, Format};
use crate::http::{ApiError, ApiResult};
use crate::routes::catalogue::{self, CatalogueParams};
use crate::state::AppState;
use darkhold_domain::api_scopes::Scope;
use darkhold_domain::error::AppError;

/// A 422 with a sentence, in the shape the rest of the API answers with.
fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

/// How many items a feed carries by default. A reader that wants more asks with `?limit=`.
const DEFAULT_LIMIT: i64 = 50;
/// The most a feed will carry, whatever it asks for.
const MAX_LIMIT: i64 = 200;

/// The feed routes.
///
/// The saved-view route sits behind [`token_from_query`], because it is the one surface a client without
/// headers polls. The catalogue feed is public and needs no rewriting at all.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/saved-views/{id}/feed", get(saved_view_feed))
        .layer(middleware::from_fn(token_from_query))
        .route("/catalogue/feed", get(catalogue_feed))
}

#[derive(Debug, Deserialize)]
struct FeedParams {
    /// `atom` (default), `rss` or `json`.
    format: Option<String>,
    /// How many items. Clamped, never refused: a feed reader asking for 500 gets [`MAX_LIMIT`].
    limit: Option<i64>,
    /// A personal access token, for clients that cannot send a header.
    ///
    /// Read by [`token_from_query`], which runs before the handler — so this field is deliberately unused
    /// here: the handler sees an ordinary `Authorization` header and one code path checks tokens.
    #[allow(dead_code)]
    token: Option<String>,
}

/// Rewrite `?token=` into `Authorization: Bearer …`.
///
/// A feed reader has no field for an auth header, so the token travels in the URL. Doing it here rather than
/// in the handler means the handler sees exactly the request a browser sends, and [`RequireActorScoped`]
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

/// `?format=`, or atom.
fn format_from(raw: Option<&str>) -> ApiResult<Format> {
    match raw.map(str::trim).filter(|f| !f.is_empty()) {
        None => Ok(Format::Atom),
        Some(raw) => Format::parse(raw)
            .ok_or_else(|| invalid(format!("format: expected atom, rss or json; got {raw}"))),
    }
}

/// A stored body as a feed summary: markdown stripped, empty treated as absent.
///
/// An empty `<summary></summary>` makes some readers show a blank card; absent makes them show the title
/// alone, which is the truth.
fn summary_of(body: &str) -> Option<String> {
    let text = commons_feed::strip_markdown(body).trim().to_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// A name as an author: absent when the source had none, never the empty string.
fn author_of(name: &str) -> Option<String> {
    let text = name.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

/// The source's own clock, parsed. `None` when the source said nothing — never "now", which would move every
/// item on every poll and make the 304 impossible.
fn published_of(stamp: Option<&str>) -> Option<DateTime<Utc>> {
    stamp
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .and_then(|text| {
            DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|parsed| parsed.with_timezone(&Utc))
        })
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

    let format = format_from(params.format.as_deref())?;
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let view = library::find_saved_view(state.db(), &account, &id)
        .await?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "saved view",
            })
        })?;

    let entries = match view.kind {
        SavedViewKind::Library => library_items(&state, &account, &view, limit).await?,
        SavedViewKind::Catalogue => catalogue_items(&state, &view, limit).await?,
    };

    let base = state.config().site.base_url.clone();
    let meta = FeedMeta {
        title: view.name.clone(),
        link: format!("{base}/library"),
        description: Some(format!("{} — a saved query on darkhold", view.name)),
        language: Some("en".to_owned()),
        self_url: Some(format!(
            "{base}/api/v1/saved-views/{id}/feed?format={}",
            format.extension()
        )),
    };
    let body = feed_http::render(&meta, &entries, format);
    respond(&body, format, &headers, true)
}

/// The public catalogue as a feed.
///
/// Same query as `GET /api/v1/catalogue`: `format`/`limit`/`token` are lifted out and everything else goes to
/// [`CatalogueParams`] exactly as the catalogue route would parse it. A reader subscribed to
/// `?fandom=…&sort=new` gets the same order the page shows.
async fn catalogue_feed(
    State(state): State<AppState>,
    // A public route takes `MaybeSession` even when it does not personalise yet: the audience invariant is
    // what keeps a future per-reader filter from having to change the signature under a passing test.
    MaybeSession(_session): MaybeSession,
    Query(params): Query<FeedParams>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let format = format_from(params.format.as_deref())?;
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    // The catalogue's own parameters, minus the three the feed owns, plus the one the feed decides. Rebuilt
    // rather than mutated so the catalogue route's parser stays the only thing that defines a filter.
    let mut rest: Vec<(String, String)> =
        url::form_urlencoded::parse(raw.unwrap_or_default().as_bytes())
            .filter(|(key, _)| key != "format" && key != "limit" && key != "token")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
    rest.push(("limit".to_owned(), limit.to_string()));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(&rest)
        .finish();
    let uri = format!("/catalogue?{query}")
        .parse::<axum::http::Uri>()
        .map_err(|error| invalid(format!("unreadable catalogue query: {error}")))?;
    let catalogue_params: CatalogueParams = axum::extract::Query::try_from_uri(&uri)
        .map(|query| query.0)
        .map_err(|error| invalid(format!("unreadable catalogue query: {error}")))?;

    let page = catalogue::search_raw(state.clone(), catalogue_params).await?;
    let base = state.config().site.base_url.clone();
    let entries: Vec<FeedEntry> = page
        .items
        .into_iter()
        .map(|row| FeedEntry {
            id: row.id,
            title: row.title,
            link: row.source_url,
            summary: summary_of(&row.summary),
            author: author_of(&row.author_text),
            // The catalogue row carries no source clock; the feed says so rather than inventing one.
            published: None,
        })
        .collect();

    let mut meta = FeedMeta {
        title: "darkhold — catalogue".to_owned(),
        link: format!("{base}/catalogue"),
        description: Some("The public catalogue, as a feed.".to_owned()),
        language: Some("en".to_owned()),
        self_url: Some(format!(
            "{base}/api/v1/catalogue/feed?format={}",
            format.extension()
        )),
    };
    // A filtered feed says so in its title, so a reader subscribed to three of them can tell them apart.
    if let Some(note) = filters_note(&rest) {
        meta.title = format!("darkhold — catalogue ({note})");
    }

    let body = feed_http::render(&meta, &entries, format);
    respond(&body, format, &headers, false)
}

/// A short human note for a filtered catalogue feed, from the parameters the reader sent.
fn filters_note(params: &[(String, String)]) -> Option<String> {
    let parts: Vec<String> = params
        .iter()
        .filter_map(|(key, value)| match key.as_str() {
            "fandom" | "tag" | "author" | "q" | "sort" => {
                if value.trim().is_empty() {
                    None
                } else {
                    Some(format!("{key}: {value}"))
                }
            }
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Render the response, or the 304 that makes a feed cheap to poll.
fn respond(body: &str, format: Format, headers: &HeaderMap, private: bool) -> ApiResult<Response> {
    let tag = feed_http::etag(body);
    if let Some(sent) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
    {
        if feed_http::matches(sent, &tag) {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            set_common_headers(response.headers_mut(), &tag, format, private);
            return Ok(response);
        }
    }
    let mut response = body.to_owned().into_response();
    set_common_headers(response.headers_mut(), &tag, format, private);
    Ok(response)
}

fn set_common_headers(headers: &mut HeaderMap, tag: &str, format: Format, private: bool) {
    if let Some(value) = feed_http::header_value(format.content_type()) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Some(value) = feed_http::header_value(tag) {
        headers.insert(header::ETAG, value);
    }
    // `private` for a feed of a reader's library — it is theirs. `public` for the catalogue, which is the
    // same document for everybody. `must-revalidate` either way: a stale feed is worse than a slow one, and
    // the tag makes revalidating free.
    headers.insert(
        header::CACHE_CONTROL,
        if private {
            HeaderValue::from_static("private, max-age=0, must-revalidate")
        } else {
            HeaderValue::from_static("public, max-age=0, must-revalidate")
        },
    );
}

/// A library view's items, through the same store function the page uses.
async fn library_items(
    state: &AppState,
    account: &str,
    view: &library::SavedView,
    limit: i64,
) -> ApiResult<Vec<FeedEntry>> {
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
        .map(|item| FeedEntry {
            id: item.id,
            title: item.title,
            link: item.source_url,
            summary: summary_of(&item.summary),
            author: author_of(&item.author_text),
            // The source's own clock, never ours: `last_synced_at` says when we looked, and a feed that
            // dated items by when we polled would move every item on every poll.
            published: published_of(item.source_updated_at.as_deref()),
        })
        .collect())
}

/// A catalogue view's items, through the same page the catalogue route serves.
async fn catalogue_items(
    state: &AppState,
    view: &library::SavedView,
    limit: i64,
) -> ApiResult<Vec<FeedEntry>> {
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
        .map(|row| FeedEntry {
            id: row.id,
            title: row.title,
            link: row.source_url,
            summary: summary_of(&row.summary),
            author: author_of(&row.author_text),
            // The catalogue row carries no source clock; the feed says so rather than inventing one.
            published: None,
        })
        .collect())
}

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
use axum::routing::{get, put};
use axum::{middleware, Router};
use chrono::{DateTime, Utc};
use commons_feed::{FeedEntry, FeedMeta};
use serde::Deserialize;

use darkhold_db::library::{self, SavedViewKind};

use crate::auth::{MaybeSession, RequireActorScoped, RequireSession};
use crate::feed_http::{self, Format};
use crate::http::{ApiError, ApiResult};
use crate::routes::catalogue::{self, CatalogueParams};
use crate::routes::discovery::require_operator;
use crate::state::AppState;
use axum::Json;
use darkhold_domain::api_scopes::Scope;
use darkhold_domain::error::AppError;
use serde_json::json;

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
        // A short handle as a feed: `feed_handles` names a subject, and a reader who wants "the naruto feed" should not have to
        // reconstruct a query string. `/feeds/{handle}/atom` is the same feed with the format fixed, because a feed reader's
        // "add by URL" dialog is where the path gets typed and some of them append the format.
        .route("/feeds/{handle}", get(handle_feed))
        // Handles are curation, not content: the operator decides what `/feeds/{name}` means, because a handle is a public
        // name and whoever picks it picks what the instance says at that address.
        .route("/admin/feeds/{handle}", put(name_handle))
        .route("/feeds/{handle}/atom", get(handle_atom_feed))
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
/// The catalogue query a handle names, or an answer saying the handle's kind is one this build cannot serve.
///
/// Only two kinds map onto the catalogue's own filters: `tag` is a search term and `user` an author. `work` and `series` were in
/// the table's vocabulary from migration 0020 and have no filter behind them — a feed of one work and its siblings is a
/// different query, and inventing a mapping here would be a feed that silently returns the wrong works. Those kinds answer
/// `422` with the kind named.
fn handle_query(kind: &str, subject: &str) -> ApiResult<(&'static str, String)> {
    match kind {
        "tag" => Ok(("q", subject.to_owned())),
        "user" => Ok(("author", subject.to_owned())),
        other => Err(invalid(format!(
            "this handle names a {other}, and only tag and user handles are served as feeds"
        ))),
    }
}

async fn handle_feed(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(handle): Path<String>,
    Query(params): Query<FeedParams>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    handle_feed_inner(
        state,
        handle,
        params.format.as_deref(),
        params.limit,
        &headers,
    )
    .await
}

/// The same feed with Atom fixed, for readers that append the format to the path.
async fn handle_atom_feed(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(handle): Path<String>,
    Query(params): Query<FeedParams>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let _ = params.format;
    handle_feed_inner(state, handle, Some("atom"), params.limit, &headers).await
}

async fn handle_feed_inner(
    state: AppState,
    handle: String,
    format_raw: Option<&str>,
    limit_raw: Option<i64>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let format = format_from(format_raw)?;
    let limit = limit_raw.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let Some((kind, subject)) = darkhold_db::external::feed_handle(state.db(), &handle)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
    else {
        return Err(ApiError(AppError::NotFound {
            resource: "feed handle",
        }));
    };
    let (field, value) = handle_query(&kind, &subject)?;
    let rest = vec![
        (field.to_owned(), value),
        ("limit".to_owned(), limit.to_string()),
    ];
    let base = state.config().site.base_url.clone();
    render_catalogue(
        state,
        rest,
        format,
        format!("handle {handle}"),
        format!("{base}/api/v1/feeds/{handle}?format={}", format.extension()),
        headers,
    )
    .await
}

/// Name a feed handle: `{"kind":"tag"|"user","subject":"..."}`.
///
/// The upsert is not a plain insert: naming a handle that already exists **repoints** it, because a handle is a public address
/// and an address that grows duplicates is a set of feeds nobody can link to. Re-pointing is the operator's call and is
/// answered with the handle, so the change is visible in the response rather than only in the table.
async fn name_handle(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
    Path(handle): Path<String>,
    Json(body): Json<NameHandleBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_operator(&state, &session)?;
    let handle = handle.trim();
    if handle.is_empty() || handle.len() > 64 {
        return Err(invalid("a handle is one to sixty-four characters"));
    }
    if !handle
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        // It goes in a URL path: a handle with a slash or a space in it is a feed nobody can type.
        return Err(invalid(
            "a handle is letters, digits, hyphens, underscores and dots",
        ));
    }
    if !matches!(body.kind.as_str(), "tag" | "user") {
        return Err(invalid(format!(
            "a {} handle names nothing this build can serve; tag and user are the kinds it can",
            body.kind
        )));
    }
    let subject = body.subject.trim();
    if subject.is_empty() {
        return Err(invalid("a handle needs a subject"));
    }
    let id = darkhold_db::external::upsert_feed_handle(state.db(), &body.kind, subject, handle)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({
        "id": id,
        "handle": handle,
        "kind": body.kind,
        "subject": subject,
        "url": format!("{}/api/v1/feeds/{handle}", state.config().site.base_url),
    })))
}

#[derive(Debug, Deserialize)]
struct NameHandleBody {
    kind: String,
    subject: String,
}

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

    let base = state.config().site.base_url.clone();
    let self_url = format!("{base}/api/v1/catalogue/feed?format={}", format.extension());
    render_catalogue(
        state,
        rest.clone(),
        format,
        note_of(&rest),
        self_url,
        &headers,
    )
    .await
}

/// A filtered feed says so in its title, so a reader subscribed to three of them can tell them apart.
fn note_of(rest: &[(String, String)]) -> String {
    filters_note(rest).unwrap_or_else(|| "catalogue".to_owned())
}

/// Render the catalogue query `rest` describes as a feed.
///
/// Extracted so the handle feeds and the catalogue feed are one renderer and one query path: a second copy is how two machine
/// surfaces come to disagree about what is eligible (M71-16), and the eligibility rules live in `catalogue::search_raw`.
async fn render_catalogue(
    state: AppState,
    rest: Vec<(String, String)>,
    format: Format,
    note: String,
    self_url: String,
    headers: &HeaderMap,
) -> ApiResult<Response> {
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
            // `commons-feed` grew a `categories` slot for a reader's filter vocabulary (suki puts a
            // rating there). These rows carry no labels yet, so the slot stays empty rather than being
            // filled with something invented — the renderers then emit nothing at all for it.
            categories: Vec::new(),
        })
        .collect();

    let meta = FeedMeta {
        title: format!("darkhold — {note}"),
        link: format!("{base}/catalogue"),
        description: Some("The public catalogue, as a feed.".to_owned()),
        language: Some("en".to_owned()),
        self_url: Some(self_url),
    };

    let body = feed_http::render(&meta, &entries, format);
    respond(&body, format, headers, false)
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
    // Every feed document, private or public, passes through here — including the 304 path below, because a
    // conditional request that costs no bytes is still a poll the product answered.
    crate::metrics::feed_rendered();
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
/// `pub(crate)` because the alert sweep runs a view through the same two helpers: a saved view that renders a feed
/// and a saved view that alerts must be the same query, or a reader is told about works the feed never showed.
pub(crate) async fn library_items(
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
            // `commons-feed` grew a `categories` slot for a reader's filter vocabulary (suki puts a
            // rating there). These rows carry no labels yet, so the slot stays empty rather than being
            // filled with something invented — the renderers then emit nothing at all for it.
            categories: Vec::new(),
        })
        .collect())
}

/// A catalogue view's items, through the same page the catalogue route serves.
pub(crate) async fn catalogue_items(
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
            // `commons-feed` grew a `categories` slot for a reader's filter vocabulary (suki puts a
            // rating there). These rows carry no labels yet, so the slot stays empty rather than being
            // filled with something invented — the renderers then emit nothing at all for it.
            categories: Vec::new(),
        })
        .collect())
}

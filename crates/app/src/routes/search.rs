//! Search routes: site search, in-work search, taxonomy autocomplete.
//!
//! Spec §15.4, §15.9.

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use darkhold_db::search::{
    search_in_work, search_works_ast_scoped, InWorkMatch, SearchError, SearchFilters,
};
use darkhold_db::search_history as db_history;
use darkhold_db::settings as db_settings;
use darkhold_domain::ids::WorkId;
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search", get(search))
        // §9.6 / M47-08b. Declared before `/search/in-work/{id}` for readability; axum matches
        // `/search/history` exactly, so the `{id}` route below cannot swallow it.
        .route("/search/history", get(list_history).delete(clear_history))
        .route("/search/in-work/{id}", get(in_work))
        .route("/users/search", get(users_search))
}

/// Search pseudonyms with the shared query language.
///
/// The same parser, the same operators and the same 422 as `/search` and
/// `/forum-search`; only the surface differs. A field from another one is an
/// error naming where it does belong, because "no results" is
/// indistinguishable from "nobody matches" and a reader has no other way to
/// learn they used the wrong box.
///
/// An empty query returns an empty page rather than every pseudonym on the
/// instance, which on a large one is a phone book and a denial of service at
/// the same time.
///
/// The `MaybeSession` extractor is declared but unused, and that is
/// deliberate. This is an anonymous surface, so nothing about the viewer
/// changes the result -- but a handler with no session extractor has no way
/// to *become* viewer-aware later without someone noticing the query needs
/// revisiting, and the route inventory asserts that a public route declares
/// its audience. The extractor is how this route states "anonymous, on
/// purpose".
async fn users_search(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Query(params): Query<UserSearchQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let results =
        darkhold_db::search::search_users_ast(state.db(), &params.q, params.limit.clamp(1, 100))
            .await
            .map_err(search_failure)?;

    let total = results.len();
    Ok(Json(serde_json::json!({
        "total": total,
        "items": results,
    })))
}

#[derive(Debug, Deserialize)]
pub struct UserSearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default = "default_limit")]
    limit: i64,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default = "default_limit")]
    limit: i64,
    /// Min word count filter (M47-05: applies user default if set).
    #[serde(default)]
    min_words: Option<i64>,
    /// Max word count filter (M47-05).
    #[serde(default)]
    max_words: Option<i64>,
    /// Sort order (M47-05: applies user default if set).
    #[serde(default)]
    sort: Option<String>,
    /// Rating ceiling (M47-05).
    #[serde(default)]
    max_rating: Option<String>,
}

fn default_limit() -> i64 {
    20
}

/// Resolve search defaults from the user's settings (M47-05, spec §46.4).
///
/// Explicit query parameters override stored defaults. Defaults come from the
/// user's search_settings (per-pseud, falling back to account).
async fn resolve_search_defaults(
    state: &AppState,
    session: &crate::auth::SessionUser,
    params: &SearchQuery,
) -> (Option<i64>, Option<i64>, Option<String>, Option<String>) {
    let pseud_id = session
        .pseud_id
        .as_ref()
        .map(|p| p.as_uuid())
        .unwrap_or_else(|| session.account_id.as_uuid());

    let stored = db_settings::read_search_settings(state.db(), pseud_id)
        .await
        .unwrap_or_default();
    let mut defaults_map = std::collections::HashMap::new();
    for (k, v) in &stored {
        defaults_map.insert(k.clone(), v.clone());
    }

    let min_words = params
        .min_words
        .or_else(|| defaults_map.get("min_words").and_then(|v| v.as_i64()));
    let max_words = params
        .max_words
        .or_else(|| defaults_map.get("max_words").and_then(|v| v.as_i64()));
    let sort = params.sort.clone().or_else(|| {
        defaults_map
            .get("sort")
            .and_then(|v| v.as_str().map(String::from))
    });
    let max_rating = params.max_rating.clone().or_else(|| {
        defaults_map
            .get("max_rating")
            .and_then(|v| v.as_str().map(String::from))
    });

    (min_words, max_words, sort, max_rating)
}

/// How many distinct searches a pseud keeps (spec §9.6), and how many the UI asks for.
const HISTORY_CAP: i64 = 20;
const HISTORY_DEFAULT_LIMIT: i64 = 10;

/// The pseud a search belongs to.
///
/// `SessionUser.pseud_id` is optional — a signed-in account that has not picked a pseud yet
/// searches as the account, which is what every other per-pseud surface in this file does. One
/// helper because three call sites must agree: if the writer keyed on the pseud and the reader on
/// the account, a reader would have a history that never appears.
fn viewer_pseud(session: &crate::auth::SessionUser) -> uuid::Uuid {
    session
        .pseud_id
        .as_ref()
        .map(|p| p.as_uuid())
        .unwrap_or_else(|| session.account_id.as_uuid())
}

/// Whether this pseud wants searches recorded (`search.record_history`, default on).
///
/// Absent means on: the registry's default is `true` and a reader who has never opened settings
/// has not opted out. An unreadable settings row also means on, because failing closed would
/// silently disable a feature for everyone the moment the settings query broke, and the reader
/// would have no way to tell that from having turned it off.
async fn recording_enabled(state: &AppState, pseud_id: uuid::Uuid) -> bool {
    let stored = db_settings::read_search_settings(state.db(), pseud_id)
        .await
        .unwrap_or_default();
    stored
        .iter()
        .find(|(key, _)| key == "search.record_history")
        .and_then(|(_, value)| value.as_bool())
        .unwrap_or(true)
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_history_limit")]
    limit: i64,
}

fn default_history_limit() -> i64 {
    HISTORY_DEFAULT_LIMIT
}

/// The reader's own last searches (spec §9.6, item 31 of the reader audit).
///
/// `RequireSession`, not `MaybeSession`: an anonymous caller has no pseud to key on, and answering
/// an empty list would make "not signed in" indistinguishable from "no history yet" — the same
/// confusion this file's query parser refuses to create for a typo'd field.
///
/// With `search.record_history` off the answer is an empty list *and* `recording: false`, so the
/// reader is not shown a history that will never refill.
async fn list_history(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
    Query(params): Query<HistoryQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let pseud_id = viewer_pseud(&session);
    let recording = recording_enabled(&state, pseud_id).await;

    // The list is what is stored, whether or not recording continues. `search.record_history`
    // governs *keeping new* searches; it does not make the instance forget what it already holds.
    //
    // The first version returned an empty list once the switch was off, and that was the wrong
    // lie: the rows were still on disk, so "recording off" showed an empty history while the
    // database kept it — the one combination a privacy switch must not produce. Showing what is
    // stored and reporting `recording: false` makes the state visible, and the reader who wants
    // it gone has the Clear control two buttons away.
    let items = db_history::recent(state.db(), pseud_id, params.limit.clamp(1, HISTORY_CAP))
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(
        serde_json::json!({ "items": items, "recording": recording }),
    ))
}

/// Forget every recorded search (spec §9.6: cleared history is a cleared review).
///
/// No confirmation step and no undo: the reader can recreate any entry by running the search
/// again, which is exactly why a confirm dialog here would be friction without protection.
async fn clear_history(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    let pseud_id = viewer_pseud(&session);
    let cleared = db_history::clear(state.db(), pseud_id)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({ "cleared": cleared })))
}

async fn search(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Query(params): Query<SearchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let viewer_id = session.as_ref().map(|s| s.account_id.to_string());
    // M47-04: load content filters for the viewer's pseud and exclude matching works.
    let filters = if let Some(ref session) = session {
        let pseud_id = session
            .pseud_id
            .as_ref()
            .map(|p| p.as_uuid())
            .unwrap_or_else(|| session.account_id.as_uuid());
        db_settings::list_content_filters(state.db(), pseud_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.filter_type, r.value))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    // M47-05: resolve search defaults (explicit params override stored).
    //
    // M47-06b: an anonymous caller's explicit parameters used to be thrown away here — the whole
    // tuple was `(None, None, None, None)` unless there was a session, so `?max_words=` on a
    // signed-out search was resolved to nothing and then echoed back as `null`. Explicit
    // parameters are the caller's own request and apply with or without a session; only the
    // *stored* defaults need somebody to store them.
    let (min_words, max_words, sort, max_rating) = match session.as_ref() {
        Some(s) => resolve_search_defaults(&state, s, &params).await,
        None => (
            params.min_words,
            params.max_words,
            params.sort.clone(),
            params.max_rating.clone(),
        ),
    };

    // …and they reach the statement. Before M47-06b the four values above existed only to be
    // returned in the response's `filters` object, which made the endpoint report a ceiling it
    // had never applied: the reader sets "maximum rating: general", reads `"max_rating":
    // "general"` in the reply, and gets explicit works back.
    let scope = SearchFilters {
        min_words,
        max_words,
        max_rating: max_rating.clone(),
        sort: sort.clone(),
    };

    let results = search_works_ast_scoped(
        state.db(),
        &params.q,
        viewer_id.as_deref(),
        params.limit,
        &filters,
        &scope,
    )
    .await
    .map_err(search_failure)?;

    // §9.6 / M47-08b: record the search — AFTER it succeeded, so a 422 from the query language
    // leaves no entry (a query the parser rejected is not one the reader would want offered
    // back), and with the count the reader is about to see rather than a later recount.
    //
    // The write may not fail the search. A history is a convenience on top of a working result
    // page; turning a successful search into a 500 because a bookkeeping insert hit a lock would
    // be a worse bug than a missing history row, so the error is logged and dropped.
    if let Some(ref session) = session {
        let pseud_id = viewer_pseud(session);
        if db_history::worth_recording(&params.q) && recording_enabled(&state, pseud_id).await {
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            if let Err(error) = db_history::record(
                state.db(),
                pseud_id,
                &params.q,
                results.len() as i64,
                &now,
                HISTORY_CAP,
            )
            .await
            {
                tracing::warn!(
                    error = %error,
                    "search history write failed; the search itself succeeded"
                );
            }
        }
    }

    Ok(Json(serde_json::json!({
        "items": results,
        "filters": {
            "min_words": min_words,
            "max_words": max_words,
            "sort": sort,
            "max_rating": max_rating,
        }
    })))
}

async fn in_work(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(id): Path<String>,
    Query(params): Query<SearchQuery>,
) -> ApiResult<Json<Vec<InWorkMatch>>> {
    let work_id: WorkId = id
        .parse()
        .map_err(|_| ApiError(darkhold_domain::AppError::NotFound { resource: "work" }))?;
    let results = search_in_work(state.db(), &work_id, &params.q)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;
    Ok(Json(results))
}

/// Turn a search failure into the right response.
///
/// A query the reader got wrong -- a typo'd operator, a field that belongs to
/// the forum search typed into the works search -- is `422` carrying the reason.
///
/// Public because the forum search needs the same mapping and the whole point
/// of the shared query language is that a mistake reads the same on every
/// surface. Two copies would drift, and the drift would show up as one surface
/// answering 422 and the other 500 for the identical typo.
/// It used to be an `anyhow!` string, so every one of them was a `500`: the
/// reader learned nothing and the operator saw a server fault that was neither.
///
/// Everything else stays `500`, because it is one.
pub fn search_failure(error: anyhow::Error) -> ApiError {
    match error.downcast_ref::<SearchError>() {
        Some(search) => ApiError(darkhold_domain::AppError::Validation {
            message: search.problem.message().to_owned(),
            field_errors: Default::default(),
        }),
        None => ApiError(darkhold_domain::AppError::Internal(error)),
    }
}

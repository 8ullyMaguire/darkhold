//! M69-22 — the morning queue and the daily surprise.
//!
//! `GET /api/v1/discovery/morning` — a small, diverse set of works drawn from a **saved filter**, plus one work that is the
//! reader's for the day.
//!
//! # The filter is the surface
//!
//! The queue is not a page: it is a saved filter's result, taken through `[discovery] morning_queue_view` (a slug this instance
//! owns), so an operator retunes it without a deploy and the response says **which filter** answered — `filter: {slug, name,
//! params}`. That is the requirement's second half: a reader can see the filter behind what they were shown, and a reader who
//! disagrees with it can point at the thing they disagree with rather than at the archive.
//!
//! # Off by default, and 404 rather than empty
//!
//! The switch off answers `404 MODULE_-shaped` `NOT_FOUND` for the route, and the switch on with **no filter named** answers
//! `404` too: a queue with no filter behind it cannot be shown to a reader, and an empty 200 would say "we looked and found
//! nothing" when the truth is "nobody configured this".
//!
//! # The day is the server's, and it is the only clock here
//!
//! The surprise is a function of (day, reader). The day is taken once, at the top of the handler, from the same `now_rfc3339`
//! the rest of the instance uses; using `Utc::now()` further down would let two reads of one request land on different days at
//! midnight and is exactly the sort of thing that makes a "daily" pick change twice.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::MaybeSession;
use crate::http::{ApiError, ApiResult};
use crate::routes::feeds;
use crate::state::AppState;
use darkhold_db::library;
use darkhold_domain::error::AppError;
use darkhold_domain::morning::{self, Item, DEFAULT_PER_FANDOM, DEFAULT_QUEUE_SIZE};

/// The morning surface.
pub fn router() -> Router<AppState> {
    Router::new().route("/discovery/morning", get(morning))
}

/// Optional knobs a caller may pass; both are bounded by the instance's own settings.
#[derive(Debug, Deserialize)]
pub struct MorningParams {
    /// How many works to offer. Clamped to the instance's `morning_queue_size` and to 1..=10.
    #[serde(default)]
    limit: Option<i64>,
}

pub async fn morning(
    State(state): State<AppState>,
    // A public door takes `MaybeSession` even though nothing here personalises by account yet: the surprise is seeded by the
    // reader when there is one, and by the anonymous bucket when there is not, and the audience invariant is what keeps that
    // from being a signature change later.
    MaybeSession(session): MaybeSession,
    Query(params): Query<MorningParams>,
) -> ApiResult<Json<Value>> {
    if !state.config().discovery.morning_queue {
        return Err(ApiError(AppError::NotFound {
            resource: "morning queue",
        }));
    }
    let Some(slug) = state.config().discovery.morning_queue_view.clone() else {
        // Configured on, but no filter named. Said plainly: an empty queue would look like an archive with nothing in it.
        return Err(ApiError(AppError::NotFound {
            resource: "morning queue (no saved filter is named in [discovery] morning_queue_view)",
        }));
    };

    // A slug is unique **per account**, so the queue's filter is looked up on the operator's account: the configuration says
    // which slug, and "the operator's own" is the only owner a config string can name without a second identifier. An
    // instance with no operator recorded cannot resolve one, and says so rather than silently drawing from nothing.
    let Some(operator) = state.config().administration.operator_account_id else {
        return Err(ApiError(AppError::Validation {
            message: format!(
                "the morning queue names the filter '{slug}', but this instance records no operator account to look it up on"
            ),
            field_errors: std::collections::BTreeMap::new(),
        }));
    };
    let operator = operator.to_string();
    let Some(view_id) = library::saved_view_id_for_slug(state.db(), &operator, &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?
    else {
        return Err(ApiError(AppError::NotFound {
            resource: "morning queue (the named filter does not exist on the operator's account)",
        }));
    };
    let Some(view) = library::find_saved_view(state.db(), &operator, &view_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?
    else {
        return Err(ApiError(AppError::NotFound {
            resource: "morning queue (the named filter vanished between the two reads)",
        }));
    };
    let account_id = operator;
    if !view.is_public {
        // A queue drawn from a private filter would leak the filter's contents to every reader. The refusal names the reason so
        // an operator can fix it in one step rather than by bisecting the config.
        return Err(ApiError(AppError::Validation {
            message: format!(
                "the morning queue's filter '{slug}' is private; a queue shown to every reader must be drawn from a public saved filter"
            ),
            field_errors: std::collections::BTreeMap::new(),
        }));
    }

    let configured = state
        .config()
        .discovery
        .morning_queue_size
        .unwrap_or(DEFAULT_QUEUE_SIZE as i64);
    let size = params
        .limit
        .unwrap_or(configured)
        .clamp(1, configured.clamp(1, 10)) as usize;

    // Ask the filter for more than the queue holds: the diversity rule needs a pool to choose from, and a pool of exactly
    // `size` would leave nothing to interleave.
    let pool_limit = (size as i64 * 4).clamp(8, 60);
    let entries = match view.kind {
        library::SavedViewKind::Catalogue => {
            feeds::catalogue_items(&state, &view, pool_limit).await?
        }
        library::SavedViewKind::Library => {
            // A library-scoped view is somebody's shelf, not a catalogue query. Saying so is better than silently serving
            // their library.
            return Err(ApiError(AppError::Validation {
                message: format!(
                    "the morning queue's filter '{slug}' is a library view; the queue runs a public catalogue filter"
                ),
                field_errors: std::collections::BTreeMap::new(),
            }));
        }
    };

    let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
    let fandoms = darkhold_db::catalogue::fandom_for_items(state.db(), &ids)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let candidates: Vec<Item> = entries
        .iter()
        .map(|entry| Item {
            work_id: entry.id.clone(),
            title: entry.title.clone(),
            fandom: fandoms.get(&entry.id).cloned().unwrap_or_default(),
        })
        .collect();

    let queue = morning::diversify(&candidates, size, DEFAULT_PER_FANDOM);
    let day = darkhold_db::identity::now_rfc3339();
    let day = &day[..10.min(day.len())];
    // The surprise is seeded by the reader when there is one. `MaybeSession` is used rather than `Option<SessionUser>` so the
    // seed is a stable string whether or not a cookie came with the request.
    let reader = session
        .map(|user| user.account_id.to_string())
        .unwrap_or_else(|| "anonymous".to_owned());
    let surprise_wanted = state.config().discovery.daily_surprise.unwrap_or(true);
    let surprise = surprise_wanted
        .then(|| morning::surprise(&candidates, day, &reader))
        .flatten();

    let base = state.config().site.base_url.trim_end_matches('/');
    let link = |item: &Item| {
        let fandom = fandoms.get(&item.work_id).cloned().unwrap_or_default();
        json!({
            "work_id": item.work_id,
            "title": item.title,
            "fandom": fandom,
            "url": format!("{base}/works/{}", item.work_id),
        })
    };

    Ok(Json(json!({
        "day": day,
        // The filter behind what the reader was shown, in the reader's own terms, with the parameters it was run with: this is
        // the half of the requirement that says a reader can see why these works.
        "filter": {
            "slug": slug,
            "name": view.name,
            "params": view.params,
            "url": format!("{base}/s/{slug}"),
            "owner": account_id,
        },
        "queue": queue.iter().map(link).collect::<Vec<Value>>(),
        "surprise": surprise.as_ref().map(link),
        "note": "a small diverse set from one saved filter, plus one work that is yours for the day",
    })))
}

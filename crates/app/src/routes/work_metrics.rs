//! Repairing a work card's counters — the operator half of `work_metrics` (M70-13).
//!
//! `work_metrics::recompute_and_store` documented itself as "used by the seed script and as a repair tool" and had no caller in
//! the binary. The counters on a work card are maintained **incrementally** — one bump per event, in the same code path that
//! writes the event — and the module's header gives the reason (never a trigger, so the dialect-parity test stays green).
//! Incremental maintenance has exactly one failure mode a materialized aggregate cannot escape: **drift**. A request that dies
//! between the event write and the counter bump, an import that writes events directly, a producer that was wrong until it was
//! fixed — each leaves the stored number permanently wrong, on a work page readers are looking at, and the only repair was
//! `psql`.
//!
//! One door: `POST /admin/works/{id}/metrics/recompute`. It answers with the row it stored, not a description of what it did,
//! so the operator sees what readers will see. The route is operator-only through `require_admin_of(None)` — the same decision
//! ADR 0003 puts in one place for every admin surface — and a caller with no standing gets `404`, because a `403` would tell
//! them this instance holds a repair lock they might want to pick.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/works/{id}/metrics/recompute", post(recompute))
}

async fn recompute(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let metrics = darkhold_db::work_metrics::recompute_and_store(state.db(), &id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(json!({
        "work_id": metrics.work_id,
        "views": metrics.views,
        "complete_reads": metrics.complete_reads,
        "reactions": metrics.reactions,
        "kudos": metrics.kudos,
        "bookmarks": metrics.bookmarks,
        "collection_adds": metrics.collection_adds,
        "reviews": metrics.reviews,
    })))
}

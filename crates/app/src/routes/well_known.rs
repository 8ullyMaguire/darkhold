//! M69-01 — `/.well-known/taste-fingerprint.json`: what this instance is into, in one readable shape.
//!
//! Peering begins by comparing two of these (M69-02). The path is `/.well-known/` because it is a statement about
//! the instance rather than an API call a client makes, and it sits outside `/api/v1` for the same reason the
//! OpenSearch description does: another instance fetches it with no session, no client code and no knowledge of
//! this product's route vocabulary.
//!
//! # It answers with the same vector the operators already see
//!
//! The numbers come from `GET /federation/theme` — the precedence `season > operator shelf > computed` — so the
//! published fingerprint cannot disagree with what the operator console shows. A second derivation of "the
//! instance's taste" would be a second answer to the same question.
//!
//! # It refuses when the instance has not said this is public
//!
//! `instance_themes.public` is the operator's statement that their taste is for sharing. Publishing anyway would
//! make that flag decorative, so a non-public (or absent) theme is a `404` with the reason, not a document with
//! empty tags — a sibling that receives `{"tags": []}` would read it as "no taste" rather than "not offered".

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;

use darkhold_domain::taste_fingerprint::{build_with_rules, Fingerprint};

use crate::auth::MaybeSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// The well-known routes.
pub fn router() -> Router<AppState> {
    Router::new().route("/.well-known/taste-fingerprint.json", get(fingerprint))
}

/// This instance's own fingerprint, or `None` when it has not published one.
///
/// Shared deliberately: M69-02 ranks candidates against *this* document, and a second assembly of it here and
/// there is how the published shape and the ranked shape drift apart.
pub async fn current(state: &AppState) -> ApiResult<Option<Fingerprint>> {
    let instance_id = state.config().site.base_url.clone();

    let scheduled = darkhold_db::theme_schedule::active_at(
        state.db(),
        &instance_id,
        &darkhold_db::identity::now_rfc3339(),
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error)))?;
    let operator = darkhold_db::operator_theme::get(state.db(), &instance_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let theme = darkhold_db::instance_theme::get_local_theme(state.db(), &instance_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;

    // The same precedence the operator console reads: a running event, then the shelf, then what readers did.
    let (vector, public) = match (&scheduled, &operator, &theme) {
        (Some(schedule), _, _) => (Some(schedule.theme_vector.clone()), true),
        (None, Some(shelf), _) => (Some(shelf.theme_vector.clone()), true),
        (None, None, Some(computed)) => (Some(computed.theme_vector.clone()), computed.public),
        (None, None, None) => (None, false),
    };
    if !public {
        return Ok(None);
    }
    let Some(vector) = vector else {
        return Ok(None);
    };

    let weights = weights_from(&vector);
    let generated_at = theme
        .as_ref()
        .map(|row| row.computed_at.clone())
        .or_else(|| operator.as_ref().map(|row| row.updated_at.clone()))
        .unwrap_or_else(darkhold_db::identity::now_rfc3339);
    // The declared hardcoded layer travels with the document: a peer decides whether to suggest this instance on the
    // same two facts this instance decides on (M69-02), and it cannot compare a layer it was never sent.
    Ok(Some(build_with_rules(
        &instance_id,
        &generated_at,
        &weights,
        state.config().discovery.enforced_rules.iter().cloned(),
    )))
}

async fn fingerprint(
    State(state): State<AppState>,
    // Public routes take `MaybeSession` by invariant (route_inventory enforces it): a surface that cannot see a
    // session today is a surface that cannot personalise later without changing its signature under a green test.
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<Fingerprint>> {
    current(&state)
        .await?
        .ok_or(ApiError(AppError::NotFound {
            resource: "public taste fingerprint",
        }))
        .map(Json)
}

/// A stored theme vector is `{tag: number}`; the fingerprint builder takes exactly that, so anything that is not a
/// number is dropped rather than coerced (a string tag weight is a bug upstream, and `0.0` would hide it).
fn weights_from(vector: &Value) -> std::collections::BTreeMap<String, f64> {
    vector
        .as_object()
        .map(|object| {
            object
                .iter()
                .filter_map(|(tag, value)| value.as_f64().map(|weight| (tag.clone(), weight)))
                .collect()
        })
        .unwrap_or_default()
}

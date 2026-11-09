//! The exclusion registry, which a gate read and nobody could write (M70-13).
//!
//! `permission::is_excluded` is read in exactly one place — Guard 2 of `fork_work`, "this work is excluded from forking" — and
//! `permission::insert_exclusion_entry` had **no caller anywhere**, no route and no test fixture. The registry was therefore empty
//! on every instance, Guard 2 could never fire, and §33's exclusion was a comment with a SQL query behind it: a gate that reads as
//! protection in the code and the spec while being incapable of refusing anything.
//!
//! Two doors, both operator-only through `require_admin_of(None)`:
//!
//! * `POST /admin/exclusions` — `{target_type, target_id, reason}`.
//! * `GET /admin/exclusions?target_type=&target_id=` — what is armed for one target.
//!
//! Three decisions, each of which the registry would be lying about if it went the other way:
//!
//! * **A reason is required.** The table has a `reason` column; without this rule it would hold empty strings forever, and an
//!   exclusion nobody can explain is a decision nobody can review.
//! * **A second registration of the same target is refused**, with a field saying so. A registry exists to answer *when* and *why*
//!   something was excluded; two rows for one target make that answer ambiguous. It is `422` and not `409` because the only `409`
//!   this error vocabulary carries is `REVISION_CONFLICT`, and telling a client a version raced when the request never mentioned
//!   one is a worse error than a generic status.
//! * **`creator` is stored and named as unenforced.** The vocabulary has both targets; `ExclusionTarget::Creator` has no consumer
//!   today, and refusing to store what the schema allows would make the door lie. The response carries `enforced_by`, the list of
//!   gates that actually consult that target — empty for `creator` — so an operator is never told an exclusion is in force when
//!   nothing reads it.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_domain::permission::{ExclusionEntry, ExclusionTarget};
use darkhold_domain::AppError;

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/exclusions", post(register).get(read))
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    target_type: String,
    target_id: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ReadRequest {
    target_type: String,
    target_id: String,
}

fn field_error(field: &str, message: impl Into<String>) -> ApiError {
    let message = message.into();
    ApiError(AppError::Validation {
        message: message.clone(),
        field_errors: std::iter::once((field.to_owned(), message)).collect(),
    })
}

/// Which gates consult a target kind, named in the answer so an operator knows what they just switched on.
///
/// `work` is read by `fork_work`'s Guard 2. `creator` is read by nothing — the enum has the variant, the schema has the column, and
/// no code path asks. Publishing the empty list is the difference between "this is in force" and "this is recorded for a gate that
/// does not exist yet".
fn enforced_by(target: ExclusionTarget) -> Vec<&'static str> {
    match target {
        ExclusionTarget::Work => vec!["fork_work"],
        ExclusionTarget::Creator => Vec::new(),
    }
}

fn parse_target(raw: &str) -> Result<ExclusionTarget, ApiError> {
    match raw {
        "work" => Ok(ExclusionTarget::Work),
        "creator" => Ok(ExclusionTarget::Creator),
        other => Err(field_error(
            "target_type",
            format!("`{other}` is not a target this registry knows (`work`, `creator`)"),
        )),
    }
}

async fn register(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<RegisterRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let target = parse_target(&body.target_type)?;
    // The reason is checked before the duplicate: a malformed request is malformed whatever the registry already holds.
    if body.reason.trim().is_empty() {
        return Err(field_error(
            "reason",
            "an exclusion needs a reason an operator can review",
        ));
    }
    let db = state.db();
    if darkhold_db::permission::is_excluded(db, target, &body.target_id)
        .await
        .map_err(ApiError::from)?
    {
        return Err(field_error(
            "target_id",
            "this target is already excluded — the registry keeps the first decision",
        ));
    }
    let entry = ExclusionEntry {
        id: uuid::Uuid::new_v4().to_string(),
        target_type: target,
        target_id: body.target_id.clone(),
        reason: body.reason.trim().to_owned(),
        created_by: user.account_id.to_string(),
        created_at: darkhold_db::identity::now_rfc3339(),
    };
    darkhold_db::permission::insert_exclusion_entry(db, &entry)
        .await
        .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": entry.id,
            "target_type": entry.target_type.as_str(),
            "target_id": entry.target_id,
            "reason": entry.reason,
            "created_by": entry.created_by,
            "created_at": entry.created_at,
            "enforced_by": enforced_by(entry.target_type),
        })),
    ))
}

async fn read(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Query(query): Query<ReadRequest>,
) -> ApiResult<Json<Value>> {
    crate::routes::admin_scope::require_admin_of(&state, &user, None).await?;
    let target = parse_target(&query.target_type)?;
    let entry = darkhold_db::permission::exclusion_for(state.db(), target, &query.target_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(match entry {
        Some(entry) => json!({
            "excluded": true,
            "id": entry.id,
            "target_type": entry.target_type.as_str(),
            "target_id": entry.target_id,
            "reason": entry.reason,
            "created_by": entry.created_by,
            "created_at": entry.created_at,
            "enforced_by": enforced_by(entry.target_type),
        }),
        // `excluded: false` rather than a `404`: the operator asked a question about their own registry, and "no entry" is the
        // answer to it, not a missing page.
        None => json!({ "excluded": false, "enforced_by": enforced_by(target) }),
    }))
}

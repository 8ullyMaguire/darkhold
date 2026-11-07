//! One-click admin actions with a linked inverse (W7 task 3).
//!
//! `POST /admin/actions/{action}` performs a registered verb and returns `{audit_id, undo, undoable, before}`.
//! `POST /admin/actions/undo` performs the inverse of a recorded action and links the two audit rows through
//! `document->>'undo_of'` — the original row is never rewritten, because "was this undone?" must stay answerable from an
//! append-only log.
//!
//! The registry is deliberately tiny. It ships the **mechanism**, proven on the one verb pair the product actually has
//! (`ban` → `unban`, i.e. `governance::execute_sanction` / `lift_sanction`). Four of the six verbs the wave document lists —
//! hide, unhide, quarantine resolve, quarantine release — have no endpoint to wrap, and `pick` is irreversible by design
//! (`tasting.rs`: a pick makes the card "permanently unpickable"). Registering those as stubs would recreate exactly the inert
//! surfaces this wave spent a day removing: doors that answer 404 to everyone while the capability document advertises them.
//!
//! The verb is invoked through the **db** layer, not the HTTP handler: `execute_sanction`/`lift_sanction` in
//! `routes/governance.rs` are private and perform their own `require_scoped_bans`, so calling them from here would either
//! bypass that check or require widening its visibility. One authorisation, one verb, one audit row.

use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::ApiError;
use crate::routes::admin_scope::require_admin_of;
use crate::state::AppState;
use darkhold_domain::AppError;

/// One reversible verb. `inverse: None` means the verb has no inverse and is never offered as a one click.
pub struct ActionSpec {
    pub name: &'static str,
    pub inverse: Option<&'static str>,
    /// The site authority this verb requires. Constant, never from the request body: a moderator of one site must not be able
    /// to name another site.
    pub site: Option<&'static str>,
}

/// The **closed** set of actions. A name outside this list is a 404, never a dispatch read out of a document — an audit row
/// must not become an executable program.
pub const ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        name: "ban",
        inverse: Some("unban"),
        site: Some("darkhold"),
    },
    ActionSpec {
        name: "unban",
        inverse: Some("ban"),
        site: Some("darkhold"),
    },
];

fn spec(name: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|a| a.name == name)
}

#[derive(Deserialize)]
pub struct ActionBody {
    /// What the action is aimed at: a sanction id, for both `ban` and `unban`.
    pub target: String,
}

#[derive(Deserialize)]
pub struct UndoBody {
    pub audit_id: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/actions/{action}", post(perform))
        .route("/admin/actions/undo", post(undo))
}

/// `POST /api/v1/admin/actions/{action}`
async fn perform(
    State(state): State<AppState>,
    Path(action): Path<String>,
    RequireSession(user): RequireSession,
    Json(body): Json<ActionBody>,
) -> Result<Json<Value>, ApiError> {
    let Some(spec) = spec(&action) else {
        return Err(ApiError(AppError::NotFound { resource: "action" }));
    };
    require_admin_of(&state, &user, spec.site).await?;

    let before = darkhold_db::governance::sanction_state(state.db(), &body.target)
        .await
        .map_err(|e| ApiError(AppError::field("sanction", e.to_string())))?;

    let actor = user.account_id.to_string();
    match spec.name {
        "ban" => {
            let applied =
                darkhold_db::governance::execute_sanction(state.db(), &body.target, &actor)
                    .await
                    .map_err(|e| ApiError(AppError::field("sanction", e.to_string())))?;
            if !applied {
                return Err(ApiError(AppError::field(
                    "sanction",
                    format!(
                        "{} is not pending (state: {}); a second execution would record an action that changed nothing",
                        body.target,
                        before.as_deref().unwrap_or("unknown")
                    ),
                )));
            }
        }
        "unban" => {
            darkhold_db::governance::lift_sanction(state.db(), &body.target, &actor)
                .await
                .map_err(|e| ApiError(AppError::field("sanction", e.to_string())))?;
        }
        // Unreachable while the registry and this match agree. Refusing is the safe direction if they ever drift.
        _ => return Err(ApiError(AppError::NotFound { resource: "action" })),
    }

    let document = json!({
        "target": body.target,
        "before": before,
        "undo": spec.inverse,
    })
    .to_string();
    let audit_id = darkhold_db::governance::audit_append_id(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &actor,
            actor_pseud: user.pseud_id.map(|p| p.to_string()).as_deref(),
            reason: None,
            action: &format!("admin.action.{}", spec.name),
            subject_type: "sanction",
            subject_id: &body.target,
            document: &document,
        },
    )
    .await
    .map_err(|e| ApiError(AppError::field("audit", e.to_string())))?;

    Ok(Json(json!({
        "audit_id": audit_id,
        "undo": spec.inverse,
        "undoable": spec.inverse.is_some(),
        "before": before,
    })))
}

/// `POST /api/v1/admin/actions/undo`
async fn undo(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<UndoBody>,
) -> Result<Json<Value>, ApiError> {
    let row = darkhold_db::governance::audit_find(state.db(), &body.audit_id)
        .await
        .map_err(|e| ApiError(AppError::field("audit_id", e.to_string())))?;
    let Some((action, subject_type, subject_id, document)) = row else {
        return Err(ApiError(AppError::NotFound { resource: "action" }));
    };

    // Undo is itself an admin action, authorised against the same site the original required.
    let original = action
        .strip_prefix("admin.action.")
        .unwrap_or(action.as_str());
    let Some(spec) = spec(original) else {
        return Err(ApiError(AppError::field(
            "audit_id",
            "that row is not a registered action and cannot be undone",
        )));
    };
    require_admin_of(&state, &user, spec.site).await?;

    if darkhold_db::governance::audit_undone_by(state.db(), &body.audit_id)
        .await
        .map_err(|e| ApiError(AppError::field("audit_id", e.to_string())))?
        .is_some()
    {
        // Not a repeat and not a success: undone twice restores a state nobody asked for. The estate's only 409 variant is
        // `RevisionConflict { expected, actual }`, which exists for row versions this action does not have — inventing two
        // numbers to reach a status code would put a lie in the audit trail. So this answers as a field error that names the
        // undo that already exists, which is the useful thing to tell an operator.
        return Err(ApiError(AppError::field(
            "audit_id",
            "that action was already undone",
        )));
    }

    let Some(inverse) = spec.inverse else {
        return Err(ApiError(AppError::field(
            "audit_id",
            "that action has no inverse; it requires a typed confirmation instead",
        )));
    };

    let actor = user.account_id.to_string();
    match inverse {
        "unban" => {
            darkhold_db::governance::lift_sanction(state.db(), &subject_id, &actor)
                .await
                .map_err(|e| ApiError(AppError::field("sanction", e.to_string())))?;
        }
        "ban" => {
            let applied =
                darkhold_db::governance::execute_sanction(state.db(), &subject_id, &actor)
                    .await
                    .map_err(|e| ApiError(AppError::field("sanction", e.to_string())))?;
            if !applied {
                return Err(ApiError(AppError::field(
                    "sanction",
                    "the sanction is no longer pending; banning it again would change nothing",
                )));
            }
        }
        _ => return Err(ApiError(AppError::NotFound { resource: "action" })),
    }

    let undo_document = json!({ "undo_of": body.audit_id, "original": document }).to_string();
    let undo_id = darkhold_db::governance::audit_append_id(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &actor,
            actor_pseud: user.pseud_id.map(|p| p.to_string()).as_deref(),
            reason: None,
            action: &format!("admin.action.{inverse}"),
            subject_type: &subject_type,
            subject_id: &subject_id,
            document: &undo_document,
        },
    )
    .await
    .map_err(|e| ApiError(AppError::field("audit", e.to_string())))?;

    Ok(Json(
        json!({ "audit_id": undo_id, "undo_of": body.audit_id }),
    ))
}

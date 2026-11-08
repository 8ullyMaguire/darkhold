//! The exchange's operator side (§11.17, §15.17, §19.14) — the switch, the instance's name, and the two queues.
//!
//! `crates/db/src/exchange.rs` implemented the whole settings surface and nothing could reach it: `set_enabled` and
//! `set_instance_id` had no caller, so **the only way to turn this instance's exchange on was SQL**, and the identifier a
//! signal carries was whatever a migration left there. `list_latent_demand` had no caller, so the instance accumulated
//! latent demand it could not show anybody; `count_provenance_signals` had no caller, so the function whose header exists
//! to keep "provenance" from being confused with "headcount" was never read by a person.
//!
//! Three doors, and the rules worth stating:
//!
//! * **The switch is one door and it answers what it did.** `PUT /admin/exchange/settings` takes any subset of
//!   `enabled` and `instance_id` and returns the whole settings row afterwards — read back, not echoed. The row is created
//!   by the first write with §11.17's 1000/hour default, so an operator who flips the switch on does not also have to know
//!   the second knob.
//! * **The instance id is an identifier, not free text.** It travels as `source_instance` on every signal this instance
//!   sends and is the dedup key for latent demand, so it is validated once here: non-empty, at most 64 characters, and
//!   restricted to `[A-Za-z0-9._:-]` (a hostname, a DID, or a short slug all fit; a space or a quote does not). Setting it
//!   is a change to what this instance calls itself, so it takes a reason and lands in the modlog.
//! * **Latent demand is demand, never a headcount** (§16.16.1). `signal_count` is how many times this instance has
//!   signalled, which is why the response names it that and carries no reader count — there is deliberately no
//!   `count_holders` anywhere in this module, and a field that looked like one would be the defect the spec warns about.
//! * **`count_provenance_signals` is an operator diagnostic.** It counts the distinct signals retained for a work — not
//!   how many people hold it. It is `Operator` rather than public for the same reason: the number is fine, but a public
//!   field invites somebody to read it as a holder count on a page where nobody can correct them.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_db::exchange;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{RequireSession, SessionUser};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/exchange/settings", get(get_settings_route))
        .route("/admin/exchange/settings", post(put_settings_route))
        .route("/admin/exchange/latent-demand", get(latent_demand_route))
        .route(
            "/admin/exchange/works/{id}/provenance",
            get(work_provenance_route),
        )
}

fn operator_only(state: &AppState, user: &SessionUser) -> ApiResult<()> {
    crate::routes::admin_scope::require_estate_hidden(state, user)
}

fn internal(error: anyhow::Error) -> ApiError {
    ApiError(darkhold_domain::AppError::Internal(error))
}

/// The settings row, or the defaults the first write would create.
///
/// `configured: false` is a real answer and not an error: no row means no operator has ever touched the exchange, which is
/// a different state from "an operator turned it off" — the second one is a decision, and this door should not report the
/// two the same way.
fn settings_view(settings: Option<&exchange::ExchangeSettings>) -> Value {
    let enabled = settings.is_some_and(|s| s.enabled);
    let rate_limit = settings.map_or(1000, |s| s.rate_limit_per_hour);
    let instance_id = settings.and_then(|s| s.instance_id.clone());
    json!({
        "configured": settings.is_some(),
        "enabled": enabled,
        "rate_limit_per_hour": rate_limit,
        "instance_id": instance_id,
        // §11.17: a disabled exchange answers 404, not 403, so the operator should see what the door *does* when it is
        // off rather than having to read the handler.
        "when_disabled": "the exchange routes answer 404, so a prober learns nothing about whether this instance runs one",
    })
}

async fn get_settings_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let settings = exchange::get_settings(state.db())
        .await
        .map_err(|error| internal(error.into()))?;
    Ok(Json(settings_view(settings.as_ref())))
}

#[derive(Debug, Deserialize)]
pub struct SettingsBody {
    /// Turn the exchange on or off.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// What this instance calls itself in a signal it sends.
    #[serde(default)]
    pub instance_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Both knobs, one door, and the answer is the row as it now stands.
///
/// A body with neither field is refused rather than treated as a no-op: an operator who sent nothing meant to change
/// something, and answering `200` to a request that changed nothing hides the bug in their client.
async fn put_settings_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<SettingsBody>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    if body.enabled.is_none() && body.instance_id.is_none() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "enabled",
            "send enabled, instance_id, or both",
        )));
    }

    if let Some(instance_id) = body.instance_id.as_deref() {
        let candidate = instance_id.trim();
        if candidate.is_empty() {
            return Err(ApiError(darkhold_domain::AppError::field(
                "instance_id",
                "must not be empty",
            )));
        }
        if candidate.chars().count() > 64 {
            return Err(ApiError(darkhold_domain::AppError::field(
                "instance_id",
                "at most 64 characters",
            )));
        }
        if !candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
        {
            return Err(ApiError(darkhold_domain::AppError::field(
                "instance_id",
                "letters, digits, dot, underscore, colon and hyphen only — this travels on the wire and is a dedup key",
            )));
        }
        exchange::set_instance_id(state.db(), candidate)
            .await
            .map_err(|error| internal(error.into()))?;
    }
    if let Some(enabled) = body.enabled {
        exchange::set_enabled(state.db(), enabled)
            .await
            .map_err(|error| internal(error.into()))?;
    }

    // A settings change is an operator decision other operators need to be able to explain, and the instance id is what
    // this instance calls itself to strangers — the one change most worth a modlog row.
    let reason = body.reason.as_deref().map(str::trim).unwrap_or_default();
    if reason.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "reason",
            "an exchange settings change says why",
        )));
    }
    let document = json!({
        "enabled": body.enabled,
        "instance_id": body.instance_id.as_deref().map(str::trim),
    })
    .to_string();
    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &user.account_id.to_string(),
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            reason: Some(reason),
            action: "exchange.settings",
            subject_type: "instance",
            subject_id: "exchange",
            document: &document,
        },
    )
    .await
    .map_err(|error| internal(error.into()))?;

    let settings = exchange::get_settings(state.db())
        .await
        .map_err(|error| internal(error.into()))?;
    Ok(Json(settings_view(settings.as_ref())))
}

async fn latent_demand_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let items = exchange::list_latent_demand(state.db(), 100)
        .await
        .map_err(|error| internal(error.into()))?;
    let views: Vec<Value> = items
        .iter()
        .map(|item| {
            json!({
                "work_id": item.work_id,
                "source_instance": item.source_instance,
                // Named for what it is. §16.16.1: re-signalling a work adds no weight, and this counter exists so an
                // operator can see a demand item is still live — not so anybody can read it as a headcount.
                "signal_count": item.signal_count,
                "reinforced_at": item.reinforced_at,
            })
        })
        .collect();
    Ok(Json(json!({"count": views.len(), "items": views})))
}

async fn work_provenance_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let signals = exchange::count_provenance_signals(state.db(), &work_id)
        .await
        .map_err(|error| internal(error.into()))?;
    Ok(Json(json!({
        "work_id": work_id,
        // The field name is the whole point of the store function's name: distinct signals retained, never people.
        "provenance_signals": signals,
        "note": "distinct signals retained for this work, not a count of readers or holders (§11.17)",
    })))
}

//! Account identity, as an operator acts on it (§14.2 folds, §15.8 guardians, §12.6 facts) — the module's doors.
//!
//! Five store functions in `crates/db/src/identity.rs` had no caller, and one of them was a documented promise the
//! instance could not keep: `grant_guardian_authorization` says in its own header that before it, "a minor could never
//! participate, whatever the operator set up" — `guardian_authorizations` existed and nothing wrote to it.
//!
//! Four doors and one honest removal:
//!
//! * **`POST /admin/accounts/{id}/age-state`** — `set_age_state`. The one place a claim about a person changes, and it
//!   takes a reason: the modlog's contract says every entry carries one, and an age decision recorded without a reason is
//!   the one change nobody can review later.
//! * **`POST /admin/accounts/{id}/guardian-authorization`** — `grant_guardian_authorization`, the implemented half of the
//!   guardian workflow. It records the authorisation *and* moves the state; a granted row that left the account in
//!   `authorization_required` would be a row nobody reads.
//! * **`POST /admin/accounts/{id}/fold`** — `fold_account`, the tombstone. A fold names where the contents went, and the
//!   door refuses a target that does not exist or is the account itself: a tombstone pointing at nothing is worse than no
//!   tombstone, because the next reader of the row has no way to tell it was a mistake.
//! * **`GET /admin/accounts/{id}/facts`** — `account_facts`, the authoritative read of trust, resonance, guest status and
//!   supporter date. `404` when the account does not exist, which is a different answer from an account with no facts and
//!   the whole reason the store returns `Option`.
//!
//! `count_excluding_system_accounts` gets **no door**. Its own header says both of its callers are tests asserting what
//! the application created; it is a test helper that lives in the store, and turning it into an operator metric would be
//! inventing a surface to lower a number. It moved to `crates/test-support` in the same change.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_db::identity::{self, age_state_string, parse_age_state, AccountStatus};
use darkhold_domain::policy::AgeState;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/accounts/{id}/facts", get(account_facts_route))
        .route("/admin/accounts/{id}/age-state", post(set_age_state_route))
        .route(
            "/admin/accounts/{id}/guardian-authorization",
            post(grant_guardian_authorization_route),
        )
        .route("/admin/accounts/{id}/fold", post(fold_account_route))
        .route("/admin/accounts/{id}/trust", post(set_trust_route))
        .route(
            "/admin/accounts/{id}/spam-demotion",
            post(apply_spam_demotion_route),
        )
        .route(
            "/admin/accounts/{id}/operator-role",
            post(grant_operator_role_route),
        )
}

/// The operator predicate, in one place (ADR 0003).
fn operator_only(state: &AppState, user: &crate::auth::SessionUser) -> ApiResult<()> {
    crate::routes::admin_scope::require_estate_hidden(state, user)
}

fn internal(error: anyhow::Error) -> ApiError {
    ApiError(darkhold_domain::AppError::Internal(error))
}

/// Every operator action here writes a modlog row, and every one of them has a reason.
///
/// The reason is not optional and not defaulted: an age decision or a fold with no reason is the kind of change that is
/// unexplainable six months later, in the one log that cannot be edited.
async fn note(
    state: &AppState,
    user: &crate::auth::SessionUser,
    reason: &str,
    action: &str,
    subject: &str,
    document: &str,
) -> ApiResult<()> {
    use darkhold_db::governance::{audit_append, AuditEntry};
    audit_append(
        state.db(),
        &AuditEntry {
            actor: &user.account_id.to_string(),
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            reason: Some(reason),
            action,
            subject_type: "account",
            subject_id: subject,
            document,
        },
    )
    .await
    .map_err(|error| internal(error.into()))
}

async fn require_account(state: &AppState, raw: &str) -> ApiResult<darkhold_domain::AccountId> {
    let id: darkhold_domain::AccountId = raw
        .parse()
        .map_err(|_| ApiError(darkhold_domain::AppError::NotFound { resource: "account" }))?;
    match darkhold_db::identity::find_account(state.db(), id).await {
        Ok(Some(_)) => Ok(id),
        Ok(None) => Err(ApiError(darkhold_domain::AppError::NotFound {
            resource: "account",
        })),
        Err(error) => Err(internal(error.into())),
    }
}

fn reason_of(reason: Option<&str>) -> ApiResult<String> {
    let reason = reason.map(str::trim).unwrap_or_default();
    if reason.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "reason",
            "an operator action on an account says why",
        )));
    }
    Ok(reason.to_string())
}

async fn account_facts_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let account = require_account(&state, &id).await?;
    let facts = identity::account_facts(state.db(), account)
        .await
        .map_err(|error| internal(error.into()))?
        // `require_account` has just proven the row exists, so `None` here is a race with a deletion rather than a
        // missing account — and a 404 is still the honest answer to "tell me about this account".
        .ok_or_else(|| {
            ApiError(darkhold_domain::AppError::NotFound {
                resource: "account",
            })
        })?;

    Ok(Json(json!({
        "account_id": account.to_string(),
        "is_guest": facts.is_guest,
        "trust_level": facts.trust_level,
        "trust_name": state.config().trust.ladder().name_of(facts.trust_level),
        "resonance": facts.resonance,
        "supporter_until": facts.supporter_until,
    })))
}

#[derive(Debug, Deserialize)]
pub struct AgeStateBody {
    /// One of `unknown`, `declared_minor`, `declared_adult`, `authorization_required`,
    /// `authorized_under_policy`, `restricted`.
    pub state: String,
    /// Optional in the *type* so that a missing one is this handler's 422 naming `reason`, rather than the JSON
    /// extractor's generic "Unprocessable Entity." — the field matters and the error should say which.
    #[serde(default)]
    pub reason: Option<String>,
}

async fn set_age_state_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<AgeStateBody>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let account = require_account(&state, &id).await?;
    let reason = reason_of(body.reason.as_deref())?;

    // The vocabulary is the store's, not the request's: `parse_age_state` answers `Unknown` for anything it does not
    // know, so an unknown word is refused here rather than silently recorded as "we have not been told anything".
    let raw = body.state.trim();
    if ![
        "unknown",
        "declared_minor",
        "declared_adult",
        "authorization_required",
        "authorized_under_policy",
        "restricted",
    ]
    .contains(&raw)
    {
        return Err(ApiError(darkhold_domain::AppError::field(
            "state",
            "unknown, declared_minor, declared_adult, authorization_required, authorized_under_policy or restricted",
        )));
    }
    let next: AgeState = parse_age_state(raw);
    let before = identity::find_account(state.db(), account)
        .await
        .map_err(|error| internal(error.into()))?
        .map(|row| age_state_string(row.age_state).to_string())
        .unwrap_or_default();

    identity::set_age_state(state.db(), account, next)
        .await
        .map_err(|error| internal(error.into()))?;
    note(
        &state,
        &user,
        &reason,
        "account.age_state",
        &account.to_string(),
        &format!("{{\"from\":\"{before}\",\"to\":\"{raw}\"}}"),
    )
    .await?;

    Ok(Json(json!({
        "account_id": account.to_string(),
        "from": before,
        "state": raw,
        "may_participate": next.may_participate(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct GuardianBody {
    /// Whatever the guardian's verification produced. Required: an authorisation with no reference is an assertion.
    #[serde(default)]
    pub verification_reference: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

async fn grant_guardian_authorization_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<GuardianBody>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let account = require_account(&state, &id).await?;
    let reason = reason_of(body.reason.as_deref())?;
    let reference = body
        .verification_reference
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if reference.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "verification_reference",
            "the reference the guardian's verification produced",
        )));
    }

    identity::grant_guardian_authorization(state.db(), account, reference)
        .await
        .map_err(|error| internal(error.into()))?;
    note(
        &state,
        &user,
        &reason,
        "account.guardian_authorization",
        &account.to_string(),
        &format!("{{\"verification_reference\":{reference:?}}}"),
    )
    .await?;

    // The grant's second half, read back rather than assumed: the store moves the state, and this is the caller's proof
    // that it did.
    let state_now = identity::find_account(state.db(), account)
        .await
        .map_err(|error| internal(error.into()))?
        .map(|row| age_state_string(row.age_state).to_string())
        .unwrap_or_default();

    Ok(Json(json!({
        "account_id": account.to_string(),
        "verification_reference": reference,
        "state": state_now,
        "may_participate": parse_age_state(&state_now).may_participate(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct FoldBody {
    /// The account whose contents were folded into.
    pub folded_into: String,
    #[serde(default)]
    pub reason: Option<String>,
}

async fn fold_account_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<FoldBody>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let account = require_account(&state, &id).await?;
    let reason = reason_of(body.reason.as_deref())?;

    // A tombstone is terminal. Folding it again would silently redirect the marker to a second account, and every row
    // re-parented by the first fold would then be attributed to somewhere it never went.
    let current = identity::find_account(state.db(), account)
        .await
        .map_err(|error| internal(error.into()))?
        .ok_or_else(|| {
            ApiError(darkhold_domain::AppError::NotFound {
                resource: "account",
            })
        })?;
    if current.status == AccountStatus::Folded {
        return Err(ApiError(darkhold_domain::AppError::field(
            "status",
            "this account is already folded; a tombstone points at one place",
        )));
    }

    let target: darkhold_domain::AccountId = body
        .folded_into
        .trim()
        .parse()
        .map_err(|_| {
            ApiError(darkhold_domain::AppError::field(
                "folded_into",
                "the account whose contents were folded into",
            ))
        })?;
    if target == account {
        return Err(ApiError(darkhold_domain::AppError::field(
            "folded_into",
            "an account cannot be folded into itself",
        )));
    }
    // The target must exist and must not itself be a tombstone: folding into a folded account leaves a chain that ends
    // nowhere, which is exactly the state this marker exists to prevent.
    let target_row = identity::find_account(state.db(), target)
        .await
        .map_err(|error| internal(error.into()))?
        .ok_or_else(|| {
            ApiError(darkhold_domain::AppError::field(
                "folded_into",
                "no such account",
            ))
        })?;
    if target_row.status != AccountStatus::Active {
        return Err(ApiError(darkhold_domain::AppError::field(
            "folded_into",
            &format!(
                "that account is {}, and a fold must point at a live account",
                target_row.status.as_str()
            ),
        )));
    }

    identity::fold_account(state.db(), account, &target.to_string())
        .await
        .map_err(|error| internal(error.into()))?;
    note(
        &state,
        &user,
        &reason,
        "account.folded",
        &account.to_string(),
        &format!("{{\"folded_into\":\"{target}\"}}"),
    )
    .await?;

    Ok(Json(json!({
        "account_id": account.to_string(),
        "status": "folded",
        "folded_into": target.to_string(),
    })))
}

// ---------------------------------------------------------------------------
// Trust, spam demotion, and standing (M70-13)
// ---------------------------------------------------------------------------
//
// Three store functions had no caller, and between them they are how standing is *given* on this instance:
//
// * `set_trust` — the ladder exists, `trust_for` reads it, the modlog records changes to it, and until now the only way to
//   move an account up or down was `UPDATE trust_levels` through psql. A moderation power with no door is a power that only
//   the person with database access has.
// * `apply_spam_demotion` — §6.7's rule for confirmed spam reports. The domain predicate (`spam_demotes`) was tested and the
//   applier was written, and nothing ran it: a spam wave left every account's level exactly where it was.
// * `grant_operator_role` — the table exists and the estate reads `role:operator` claims; granting one was SQL.
//
// Every one of them is an operator action on somebody else's account, so every one carries a reason and writes a modlog row.

#[derive(Debug, Deserialize)]
pub struct SetTrustBody {
    /// The level to set. Must be one the configured ladder names: a level the ladder does not have is a typo, not a
    /// promotion, and storing it would make `name_of` answer `null` for an account whose level *is* set.
    pub level: i64,
    #[serde(default)]
    pub reason: Option<String>,
}

async fn set_trust_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<SetTrustBody>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let account = require_account(&state, &id).await?;
    let reason = reason_of(body.reason.as_deref())?;

    let ladder = state.config().trust.ladder();
    let Some(name) = ladder.name_of(body.level) else {
        return Err(ApiError(darkhold_domain::AppError::field(
            "level",
            "a level the configured trust ladder names",
        )));
    };
    let before = darkhold_db::governance::trust_for(state.db(), &account.to_string())
        .await
        .map_err(|error| internal(error.into()))?;

    // The basis column is JSONB on PostgreSQL and TEXT on SQLite, so the reason travels as a document rather than as a bare
    // string: `set_trust(…, "spam")` is valid on SQLite and `invalid input syntax for type json` on PostgreSQL, which is the
    // kind of difference a door should absorb once rather than each caller rediscovering.
    let basis = json!({ "reason": reason, "operator": user.account_id.to_string() }).to_string();
    darkhold_db::governance::set_trust(state.db(), &account.to_string(), body.level, &basis)
        .await
        .map_err(|error| internal(error.into()))?;
    note(
        &state,
        &user,
        &reason,
        "trust-set",
        &account.to_string(),
        &json!({ "from": before, "to": body.level, "name": name }).to_string(),
    )
    .await?;

    Ok(Json(json!({
        "account_id": account.to_string(),
        "level": body.level,
        "level_name": name,
        "previous_level": before,
    })))
}

#[derive(Debug, Deserialize)]
pub struct SpamDemotionBody {
    #[serde(default)]
    pub reason: Option<String>,
}

/// Run §6.7's confirmed-spam demotion against one account.
///
/// The store answers `Ok(None)` for two different situations — not enough confirmed reports, and already at the floor — and a
/// door that reported both as "no change" would make the one an operator can act on (collect more evidence) indistinguishable
/// from the one that is finished. So the door asks the two questions itself and names which one it answered.
async fn apply_spam_demotion_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<SpamDemotionBody>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let account = require_account(&state, &id).await?;
    let reason = reason_of(body.reason.as_deref())?;
    let account_text = account.to_string();

    let settings = state.config().trust.trust_settings();
    let confirmed = darkhold_db::governance::confirmed_spam_reports(
        state.db(),
        &account_text,
        darkhold_domain::trust::SPAM_DEMOTION_WINDOW_DAYS,
    )
    .await
    .map_err(|error| internal(error.into()))?;
    let before = darkhold_db::governance::trust_for(state.db(), &account_text)
        .await
        .map_err(|error| internal(error.into()))?;

    let applied = darkhold_db::governance::apply_spam_demotion(state.db(), &account_text, &settings)
        .await
        .map_err(|error| internal(error.into()))?;

    let (outcome, after) = match applied {
        Some(level) => ("demoted", level),
        None if !darkhold_domain::trust::spam_demotes(confirmed) => {
            ("not_enough_confirmed_reports", before)
        }
        None => ("at_floor", before),
    };
    if applied.is_some() {
        note(
            &state,
            &user,
            &reason,
            "trust-spam-demotion",
            &account_text,
            &json!({ "from": before, "to": after, "confirmed_reports": confirmed }).to_string(),
        )
        .await?;
    }

    Ok(Json(json!({
        "account_id": account_text,
        "outcome": outcome,
        "confirmed_reports": confirmed,
        "level_before": before,
        "level_after": after,
    })))
}

#[derive(Debug, Deserialize)]
pub struct OperatorRoleBody {
    /// `role:operator`, or `role:moderator:<site>` / `role:curator:<site>`.
    pub role: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Grant this instance's claim for an estate role.
///
/// The vocabulary is checked here because the table has none, and a role string that no reader of `operator_role` recognises
/// is worse than a refusal: it looks like standing and grants nothing. The site in `role:moderator:<site>` is part of the
/// claim, not a request parameter — ADR 0003's rule for the doors applies to the grant that opens them.
async fn grant_operator_role_route(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<OperatorRoleBody>,
) -> ApiResult<Json<Value>> {
    operator_only(&state, &user)?;
    let account = require_account(&state, &id).await?;
    let reason = reason_of(body.reason.as_deref())?;

    let role = body.role.trim();
    let valid = match role.strip_prefix("role:") {
        Some("operator") => true,
        Some(rest) => {
            let mut parts = rest.splitn(2, ':');
            matches!(parts.next(), Some("moderator" | "curator"))
                && parts.next().is_some_and(|site| !site.trim().is_empty())
        }
        None => false,
    };
    if !valid {
        return Err(ApiError(darkhold_domain::AppError::field(
            "role",
            "role:operator, or role:moderator:<site> / role:curator:<site>",
        )));
    }

    darkhold_db::governance::grant_operator_role(state.db(), &account.to_string(), role)
        .await
        .map_err(|error| internal(error.into()))?;
    note(
        &state,
        &user,
        &reason,
        "operator-role-granted",
        &account.to_string(),
        &json!({ "role": role }).to_string(),
    )
    .await?;

    Ok(Json(json!({
        "account_id": account.to_string(),
        "role": role,
    })))
}


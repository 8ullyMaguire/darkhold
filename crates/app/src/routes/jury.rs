//! GS-5 — the jury's doors: draw a panel, read it, and let a seated juror speak.
//!
//! The rule is `darkhold_domain::jury` (pure, no database, no clock). The store is `darkhold_db::jury` (the pool, the panel, the verdicts). This
//! module is only the doorway, and it makes four decisions the other two deliberately leave open:
//!
//! * **Drawing a jury is a service, not a member's gesture.** `POST …/jury` takes `Privilege::JuryService` (level 5 by default), because the person
//!   who decides *that a report needs a jury* is the one triaging, and a reporter who could draw their own panel could draw it five times until it
//!   looked right. Voting takes no privilege at all beyond being seated: a juror was chosen by the draw, and asking them to qualify a second time
//!   would empty the pool the rule just filled.
//! * **A draw is one per report.** Asking again returns the existing panel rather than redrawing (409 would also be defensible; returning the panel is
//!   better because the caller's actual question is "what is the panel"). Redrawing is an appeal, and it is `appeal: true` explicitly.
//! * **The exclusions are the reporter and the reported, plus the earlier panel on an appeal.** They are read here and passed into the store, so the
//!   rule stays a rule and the appeal semantics are visible at the call site.
//! * **A stranger cannot tell a panel exists.** A non-member asking for the panel gets `404`, and a non-member voting gets `404` — the same answer as
//!   a report id that does not exist. A juror's identity is a moderation fact about a person, and a 403 would confirm it.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_domain::error::AppError;
use darkhold_domain::jury::JuryRules;
use darkhold_domain::trust::Privilege;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{privilege_check, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/reports/{task}/jury", post(draw_panel))
        .route("/reports/{task}/jury", get(read_panel))
        .route("/reports/{task}/jury/verdict", post(cast_verdict))
        .route("/me/jury", get(my_assignments))
}

/// The rules this instance draws with, read from `[governance]`.
fn rules_of(state: &AppState) -> JuryRules {
    let governance = &state.config().governance;
    JuryRules {
        // A negative or zero size is a config error the draw refuses (`EmptyPanel`); clamping here would silently decide a jury of one.
        size: governance.jury_size.max(0) as usize,
        min_level: governance.jury_min_level,
        elastic_min: governance.jury_elastic_min.max(0) as usize,
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct DrawBody {
    /// The seed, if the caller wants a reproducible draw. Absent means "pick one and store it", which is the normal case: the seed is the audit
    /// handle, not a knob a juror chooses.
    #[serde(default)]
    pub seed: Option<String>,
    /// Draw a fresh panel for a decision being appealed. Passes the current panel as the exclusion set, so the same people cannot decide the appeal.
    #[serde(default)]
    pub appeal: bool,
}

/// `POST /reports/{task}/jury` — draw a panel for a report's review task.
pub async fn draw_panel(
    State(state): State<AppState>,
    session: RequireSession,
    Path(task): Path<String>,
    body: Option<Json<DrawBody>>,
) -> ApiResult<Json<Value>> {
    let caller = session.0.account_id.to_string();
    let (level, required) = privilege_check(&state, &caller, Privilege::JuryService)
        .await
        .map_err(|error| ApiError(AppError::internal("reading the caller's standing", error)))?;
    if level < required {
        return Err(ApiError(AppError::TrustLevelInsufficient { level, required }));
    }

    let body = body.map(|Json(body)| body).unwrap_or_default();

    // The report behind the task, so the exclusions are the people actually involved rather than whoever the caller names.
    let Some((reporter, reported)) = darkhold_db::jury::task_parties(state.db(), &task)
        .await
        .map_err(|error| ApiError(AppError::internal("reading the report behind this task", error)))?
    else {
        return Err(ApiError(AppError::NotFound { resource: "task" }));
    };

    let existing = darkhold_db::jury::panel_for_task(state.db(), &task)
        .await
        .map_err(|error| ApiError(AppError::internal("reading the panel", error)))?;

    if let Some(panel) = &existing {
        if !body.appeal {
            // Answering with the panel, not 409: the caller's question is "what is the panel", and a second draw would make the seed stored on the
            // first one a lie.
            return Ok(Json(panel_view(panel, &caller, level)));
        }
    }

    // The caller is excluded too, and §14 does not say so — it names the reporter, the reported and the earlier panel. The reason to add the caller is
    // that the caller is the person triaging this report: a triager who can seat themselves on the panel that decides it has both halves of the
    // decision, and any other juror can be outvoted by the person who chose the case. It costs one seat from a pool that had to have five anyway.
    let mut exclusions: Vec<String> = vec![reporter, reported, caller.clone()];
    let appeal_of = if body.appeal {
        let Some(panel) = &existing else {
            return Err(ApiError(AppError::Validation {
                message: "there is no panel to appeal: draw one first".to_owned(),
                field_errors: Default::default(),
            }));
        };
        // `governance.appeal_fresh_jury` is the switch that makes this exclusion exist. Off, an appeal may reuse the panel — an instance that wants
        // continuity rather than a fresh draw says so in its config, and the panel it avoided is still recorded.
        if state.config().governance.appeal_fresh_jury {
            for member in &panel.members {
                exclusions.push(member.account.clone());
            }
        }
        Some(panel.id.clone())
    } else {
        None
    };

    let seed = body.seed.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let panel_id = uuid::Uuid::new_v4().to_string();

    let drawn = darkhold_db::jury::draw_and_save(
        state.db(),
        &panel_id,
        &task,
        rules_of(&state),
        &exclusions,
        &seed,
        appeal_of.as_deref(),
    )
    .await
    .map_err(|error| ApiError(AppError::internal("drawing the panel", error)))?;

    match drawn {
        darkhold_db::jury::Drawn::TooSmall {
            available,
            required,
        } => Err(ApiError(AppError::Validation {
            message: format!(
                "a jury needs at least {required} eligible accounts who are not the reporter or the reported, and this instance has {available}: \
                 escalate to an operator"
            ),
            field_errors: Default::default(),
        })),
        darkhold_db::jury::Drawn::Panel { .. } => {
            let panel = darkhold_db::jury::panel_for_task(state.db(), &task)
                .await
                .map_err(|error| ApiError(AppError::internal("reading the panel back", error)))?
                .ok_or(ApiError(AppError::internal(
                    "the panel written a moment ago cannot be read",
                    std::io::Error::other("missing panel"),
                )))?;
            Ok(Json(panel_view(&panel, &caller, level)))
        }
    }
}

/// `GET /reports/{task}/jury` — the panel, for a seated juror or an operator.
pub async fn read_panel(
    State(state): State<AppState>,
    session: RequireSession,
    Path(task): Path<String>,
) -> ApiResult<Json<Value>> {
    let caller = session.0.account_id.to_string();
    let Some(panel) = darkhold_db::jury::panel_for_task(state.db(), &task)
        .await
        .map_err(|error| ApiError(AppError::internal("reading the panel", error)))?
    else {
        return Err(ApiError(AppError::NotFound { resource: "task" }));
    };

    let seated = panel.members.iter().any(|member| member.account == caller);
    let level = darkhold_db::governance::trust_for(state.db(), &caller)
        .await
        .map_err(|error| ApiError(AppError::internal("reading the caller's standing", error)))?;
    if !seated && level < 5 {
        // Not a member and not a service: 404, because "you are not on this jury" is a fact about a moderation decision.
        return Err(ApiError(AppError::NotFound { resource: "task" }));
    }
    Ok(Json(panel_view(&panel, &caller, level)))
}

#[derive(Debug, Deserialize)]
pub struct VerdictBody {
    pub verdict: String,
}

/// `POST /reports/{task}/jury/verdict` — a seated juror answers.
pub async fn cast_verdict(
    State(state): State<AppState>,
    session: RequireSession,
    Path(task): Path<String>,
    Json(body): Json<VerdictBody>,
) -> ApiResult<Json<Value>> {
    let caller = session.0.account_id.to_string();
    let Some(panel) = darkhold_db::jury::panel_for_task(state.db(), &task)
        .await
        .map_err(|error| ApiError(AppError::internal("reading the panel", error)))?
    else {
        return Err(ApiError(AppError::NotFound { resource: "task" }));
    };

    let seated = panel.members.iter().any(|member| member.account == caller);
    if !seated {
        return Err(ApiError(AppError::NotFound { resource: "task" }));
    }
    if !matches!(body.verdict.as_str(), "uphold" | "overturn") {
        return Err(ApiError(AppError::Validation {
            message: "a verdict is `uphold` or `overturn`".to_owned(),
            field_errors: Default::default(),
        }));
    }

    let recorded = darkhold_db::jury::record_verdict(state.db(), &panel.id, &caller, &body.verdict)
        .await
        .map_err(|error| ApiError(AppError::internal("recording the verdict", error)))?;
    if !recorded {
        // `422` with a field error rather than a 409: `AppError` has no conflict-with-a-message variant, and "you already voted" is best taught by
        // naming the field. What matters is that it is refused: a juror who may revise after seeing the tally is a juror whose vote is a function of
        // the tally.
        return Err(ApiError(AppError::Validation {
            message: "you have already given a verdict on this panel; a jury is not a poll".to_owned(),
            field_errors: std::collections::BTreeMap::from([(
                "verdict".to_owned(),
                "already recorded".to_owned(),
            )]),
        }));
    }

    let panel = darkhold_db::jury::panel_for_task(state.db(), &task)
        .await
        .map_err(|error| ApiError(AppError::internal("reading the panel back", error)))?
        .ok_or(ApiError(AppError::NotFound { resource: "task" }))?;

    // The last verdict is not just a row: it is the decision. Applying it here means the queue moves when the panel is done, instead of waiting for
    // somebody to notice that every juror has spoken. `undecided` (a tie, which only an even panel configured against the rules can produce) leaves
    // the report open for a human.
    let mut outcome = Value::Null;
    if panel.closed() {
        let decided = darkhold_db::jury::apply_panel_outcome(state.db(), &panel)
            .await
            .map_err(|error| ApiError(AppError::internal("recording the panel's decision", error)))?;
        outcome = decided.map(Value::String).unwrap_or(Value::Null);
    }

    let mut view = panel_view(&panel, &caller, 0);
    view["decided"] = outcome;
    Ok(Json(view))
}

/// `GET /me/jury` — the panels this account sits on. Nothing else tells a juror that a decision is waiting on them.
pub async fn my_assignments(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    let caller = session.0.account_id.to_string();
    let assignments = darkhold_db::jury::panels_for_account(state.db(), &caller)
        .await
        .map_err(|error| ApiError(AppError::internal("reading your jury assignments", error)))?;

    let rows: Vec<Value> = assignments
        .into_iter()
        .map(|(panel_id, task_id, verdict, drawn_at)| {
            json!({
                "panel": panel_id,
                "task": task_id,
                "verdict": if verdict.is_empty() { Value::Null } else { Value::String(verdict) },
                "drawn_at": drawn_at,
            })
        })
        .collect();
    Ok(Json(json!({ "panels": rows })))
}

/// What a caller may see of a panel.
///
/// A service caller (level ≥ the draw's floor) sees who sits and how they voted, with the accounts named: it has to act on the result. A juror sees
/// the roster and the tally but **not** other jurors' individual votes while their own is open — a juror who knows the count before voting is a
/// juror whose vote is a function of the count.
fn panel_view(panel: &darkhold_db::jury::Panel, caller: &str, viewer_level: i64) -> Value {
    let (uphold, overturn) = panel.tally();
    let own_open = panel
        .members
        .iter()
        .find(|member| member.account == caller)
        .map(|member| member.verdict.is_empty())
        .unwrap_or(false);
    // **Votes open only when your own is in.** A service caller (level 5, `Privilege::JuryService`) has to act on the result and always sees them;
    // a juror sees them once they have voted. A juror who can read the split before deciding is a juror whose vote is a function of the split, and
    // every remaining voter inherits the same temptation — which is how a jury becomes a herd in seating order.
    let sees_votes = viewer_level >= 5 || !own_open;
    let members: Vec<Value> = panel
        .members
        .iter()
        .map(|member| {
            let own = member.account == caller;
            let may_see = sees_votes || (own && !member.verdict.is_empty());
            json!({
                "account": member.account,
                "you": own,
                "verdict": if may_see && !member.verdict.is_empty() {
                    Value::String(member.verdict.clone())
                } else {
                    Value::Null
                },
            })
        })
        .collect();

    json!({
        "panel": panel.id,
        "task": panel.task_id,
        "seed": panel.seed,
        "size": panel.size,
        "level_floor": panel.level_floor,
        "drawn_at": panel.drawn_at,
        "appeal_of": panel.appeal_of,
        "closed": panel.closed(),
        "tally": { "uphold": uphold, "overturn": overturn },
        "open": own_open,
        "members": members,
        "votes_shown": sees_votes,
    })
}

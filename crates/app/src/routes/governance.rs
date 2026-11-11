//! M14 — Governance routes: reports, moderation, sanctions, appeals, trust.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use darkhold_domain::AppError;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{MaybeSession, RequireSession};
use crate::format_rfc3339;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// The seven reason codes a report may be filed against.
///
/// Mirrored from `estate/contracts/reason-codes.json`, which is the published contract and the source the legal
/// page at `/legal/acceptable-use` is checked against. Free-text reasons were the state of things until 2026-11-07:
/// the moderation queue could be sorted by a string anybody could invent, which is the same as not being sorted at
/// all. Gate 57 compares this list with the contract, so the copy cannot drift quietly.
pub const REPORT_REASONS: [&str; 7] = [
    "spam",
    "harassment",
    "off-topic",
    "copyright",
    "illegal",
    "csam",
    "other",
];

/// Codes that skip the ordinary moderation ladder and go straight to the operator.
///
/// The policy says so, and now the write path says so too: a report filed as `csam` or `illegal` is not something a
/// volunteer reads first.
pub const OPERATOR_ONLY_REASONS: [&str; 2] = ["csam", "illegal"];

/// Rejects a reason that is not one of the seven, naming the alternatives in the field error.
///
/// The error is a `Validation` (code `VALIDATION_FAILED`, status 422) rather than a 400, because the request is
/// well-formed JSON and the *field* is wrong — a client can switch on it and highlight the field.
fn validate_reason(reason: &str) -> Result<(), AppError> {
    if REPORT_REASONS.contains(&reason) {
        return Ok(());
    }
    let mut field_errors = std::collections::BTreeMap::new();
    field_errors.insert(
        "reason".to_owned(),
        format!("must be one of: {}", REPORT_REASONS.join(", ")),
    );
    Err(AppError::Validation {
        message: format!("unknown report reason: {reason}"),
        field_errors,
    })
}

pub fn router() -> Router<AppState> {
    routes()
}

fn routes() -> Router<AppState> {
    Router::new()
        .route("/reports", post(submit_report))
        .route("/reports", get(list_reports))
        .route("/reports/{id}", get(get_report))
        .route("/moderation/queue", get(moderation_queue))
        .route("/moderation/reports/{id}/assign", post(assign_task))
        .route("/moderation/tasks/{id}/decide", post(decide_task))
        // M45-52 step 5: the transparency report in miniature. Counts per ground and outcome, nothing that can be
        // deanonymised — a cell below the threshold is suppressed, not rounded into a lie.
        .route("/statements/summary", get(statements_summary))
        .route("/sanctions", post(issue_sanction))
        // Two-phase for the IRREVERSIBLE case (capability ledger 3.4): filing creates a pending
        // case and changes nothing, executing applies it. Ported from gravity's
        // `/discipline/permanent-ban` + `/permanent-ban/execute` pair.
        .route("/sanctions/file", post(file_sanction))
        .route("/sanctions/{id}/execute", post(execute_sanction))
        .route("/sanctions/{id}/lift", post(lift_sanction))
        .route("/me/sanctions", get(my_sanctions))
        .route("/appeals", post(open_appeal))
        .route("/appeals/{id}/decide", post(decide_appeal))
        .route("/appeals", get(list_my_appeals))
        .route("/me/trust", get(my_trust))
        .route("/me/audit-log", get(my_audit_log))
        // The modlog: readable by any signed-in reader (spec §14.6).
        .route("/modlog", get(modlog))
        // An account's trust history: the events, not just the level.
        .route("/accounts/{id}/trust", get(account_trust_history))
}

// ---------------------------------------------------------------------------
// Reports (spec §19.3)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SubmitReportBody {
    subject_type: String,
    subject_id: String,
    reason: String,
    detail: Option<String>,
    /// `dsa` (the default) or `dmca`. Two regimes, two forms: the DSA's Article 16 notice and 17 U.S.C. 512(c)(3)'s
    /// statutory elements are not the same list, and folding them together makes one of the two non-compliant.
    #[serde(default)]
    kind: String,
    // The four statutory elements of a copyright notice. Optional at the type level so a missing one is refused **by
    // name** (`422` naming the field) rather than by serde's anonymous "missing field".
    #[serde(default)]
    copyrighted_work: String,
    #[serde(default)]
    infringing_material: String,
    #[serde(default)]
    good_faith_statement: String,
    #[serde(default)]
    signature: String,
}

async fn submit_report(
    State(state): State<AppState>,
    MaybeSession(user): MaybeSession,
    Json(body): Json<SubmitReportBody>,
) -> ApiResult<Json<Value>> {
    // The code is checked before anything is written: a report the operator cannot sort is a report nobody finds.
    validate_reason(&body.reason)?;

    let pages_operator = OPERATOR_ONLY_REASONS.contains(&body.reason.as_str());

    // Reports may be submitted by anyone, including unauthenticated visitors.
    // Anonymous reports are attributed to a sentinel account id "anonymous".
    let reporter = user
        .as_ref()
        .map(|u| u.account_id.to_string())
        .unwrap_or_else(|| "anonymous".to_owned());
    // Step 6: **the two regimes are two forms.**
    //
    // `dsa` is the default and is what every existing caller already sends. `dmca` carries the four elements a copyright
    // notice must contain under 17 U.S.C. 512(c)(3): identification of the copyrighted work, identification of the material
    // claimed to infringe, a good-faith statement, and the complainant's signature. Each one missing is a `422` naming that
    // field, because a notice that fails one element is not a weak notice, it is not a notice.
    let kind = if body.kind.trim().is_empty() { "dsa" } else { body.kind.trim() };
    match kind {
        "dsa" => {}
        "dmca" => {
            for (field, value) in [
                ("copyrighted_work", body.copyrighted_work.trim()),
                ("infringing_material", body.infringing_material.trim()),
                ("good_faith_statement", body.good_faith_statement.trim()),
                ("signature", body.signature.trim()),
            ] {
                if value.is_empty() {
                    return Err(ApiError(AppError::field(
                        field,
                        "a copyright notice must contain this element (17 U.S.C. 512(c)(3))",
                    )));
                }
            }
        }
        other => {
            return Err(ApiError(AppError::field(
                "kind",
                format!("unknown notice kind `{other}`: expected `dsa` or `dmca`"),
            )));
        }
    }

    let id = darkhold_db::governance::open_report(
        state.db(),
        &body.subject_type,
        &body.subject_id,
        &reporter,
        &body.reason,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    if kind == "dmca" {
        darkhold_db::governance::write_dmca_notice(
            state.db(),
            &id,
            body.copyrighted_work.trim(),
            body.infringing_material.trim(),
            body.good_faith_statement.trim(),
            body.signature.trim(),
        )
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    }

    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &reporter,
            // A report may be filed by an anonymous visitor, and then there is
            // no pseudonym behind it. An honest absence beats a made-up one.
            actor_pseud: user
                .as_ref()
                .and_then(|u| u.pseud_id.map(|id| id.to_string()))
                .as_deref(),
            reason: Some(&body.reason),
            action: "report.submitted",
            subject_type: &body.subject_type,
            subject_id: &body.subject_id,
            // `pages_operator` is written rather than implied: severity lives in the contract, but whether the
            // report was supposed to reach the operator is a fact about this submission and belongs in the record.
            document: &json!({
                "reason": body.reason,
                "detail": body.detail,
                "report_id": id,
                "pages_operator": pages_operator
            })
            .to_string(),
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

async fn list_reports(
    State(state): State<AppState>,
    RequireSession(_user): RequireSession,
) -> ApiResult<Json<Value>> {
    let items = darkhold_db::governance::list_open_reports(state.db(), 100)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({ "items": items })))
}

async fn get_report(
    State(state): State<AppState>,
    Path(id): Path<String>,
    RequireSession(_user): RequireSession,
) -> ApiResult<Json<Value>> {
    let items = darkhold_db::governance::list_open_reports(state.db(), 1000)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    let found = items.into_iter().find(|r| r["id"].as_str() == Some(&id));
    match found {
        Some(r) => Ok(Json(r)),
        None => Err(ApiError(AppError::field("report", "not found"))),
    }
}

// ---------------------------------------------------------------------------
// Moderation queue (spec §19.4)
// ---------------------------------------------------------------------------

async fn moderation_queue(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let reviewer = user.account_id.to_string();
    // Spec §14.4: the moderation queue is a **moderator's** privilege. The
    // route asks for the privilege and does not know what level that is.
    let (level, required) = crate::auth::privilege_check(
        &state,
        &reviewer,
        darkhold_domain::trust::Privilege::ModerationQueue,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if level < required {
        return Err(ApiError(AppError::TrustLevelInsufficient {
            level,
            required,
        }));
    }
    let items = darkhold_db::governance::list_open_reports(state.db(), 100)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
struct AssignTaskBody {}

async fn assign_task(
    State(state): State<AppState>,
    Path(report_id): Path<String>,
    RequireSession(user): RequireSession,
    _body: Json<AssignTaskBody>,
) -> ApiResult<Json<Value>> {
    let reviewer = user.account_id.to_string();
    // Assigning a report is **holding the moderation queue** — the same
    // privilege that reads it. The gate was missing here, which meant any
    // signed-in reader could put their name on a review task.
    let (level, required) = crate::auth::privilege_check(
        &state,
        &reviewer,
        darkhold_domain::trust::Privilege::ModerationQueue,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if level < required {
        return Err(ApiError(AppError::TrustLevelInsufficient {
            level,
            required,
        }));
    }
    let task_id = darkhold_db::governance::assign_task(state.db(), &report_id, &reviewer)
        .await
        .map_err(|e| {
            if e.to_string().contains("self-review") {
                AppError::field("reviewer", "self-review not allowed")
            } else {
                AppError::Internal(e.into())
            }
        })?;

    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &reviewer,
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            reason: None,
            action: "review.assigned",
            subject_type: "report",
            subject_id: &report_id,
            document: &json!({ "task_id": task_id }).to_string(),
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({ "task_id": task_id })))
}

#[derive(Debug, Deserialize)]
struct DecideTaskBody {
    outcome: String,
    /// The reviewer's paragraph. Optional at the *type* level so its absence is refused by name (`422` naming `facts`)
    /// rather than by serde's generic "missing field", which is the same refusal without the field.
    #[serde(default)]
    ground: String,
    #[serde(default)]
    facts: String,
    #[serde(default)]
    legal_ground: String,
}

async fn decide_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    RequireSession(user): RequireSession,
    Json(body): Json<DecideTaskBody>,
) -> ApiResult<Json<Value>> {
    let reviewer = user.account_id.to_string();
    // Spec §14.4: **resolving a report** is a senior reviewer's privilege.
    // The gate was missing here, which meant any signed-in reader could
    // uphold or dismiss a report.
    let (level, required) = crate::auth::privilege_check(
        &state,
        &reviewer,
        darkhold_domain::trust::Privilege::ResolveReports,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if level < required {
        return Err(ApiError(AppError::TrustLevelInsufficient {
            level,
            required,
        }));
    }
    // M45-52 step 2: the statement is written WITH the decision, not after it.
    //
    // The alternative — decide now, explain later — is how a queue ends up full of decisions nobody can defend: the
    // reviewer has moved on, the report is closed, and the author is left with a sanction and no reasons. So the reasons
    // are validated *before* the decision is recorded, and a missing one refuses the decision.
    let ground = darkhold_domain::governance::StatementGround::parse(&body.ground).ok_or_else(|| {
        ApiError(AppError::field(
            "ground",
            format!(
                "a decision must name its ground: one of {}",
                darkhold_domain::governance::StatementGround::ALL
                    .iter()
                    .map(|g| g.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ))
    })?;
    if body.facts.trim().is_empty() {
        return Err(ApiError(AppError::field(
            "facts",
            "a decision must state the facts it rests on, in one paragraph",
        )));
    }
    if ground.requires_legal_ground() && body.legal_ground.trim().is_empty() {
        return Err(ApiError(AppError::field(
            "legal_ground",
            format!(
                "the ground `{}` is a claim about law, so it must name the instrument",
                ground.as_str()
            ),
        )));
    }

    darkhold_db::governance::decide_task(state.db(), &task_id, &reviewer, &body.outcome)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    // The deadline is stored, not derived: a window a person is told about must not move when a constant does.
    let appeal_until = (time::OffsetDateTime::now_utc()
        + time::Duration::days(darkhold_domain::governance::APPEAL_WINDOW_DAYS))
    .format(&time::format_description::well_known::Rfc3339)
    .map_err(|e| ApiError(AppError::Internal(anyhow::anyhow!(e.to_string()))))?;

    let statement_id = darkhold_db::governance::write_statement(
        state.db(),
        &darkhold_db::governance::NewStatement {
            task_id: &task_id,
            outcome: &body.outcome,
            ground: ground.as_str(),
            facts: &body.facts,
            legal_ground: &body.legal_ground,
            decided_by: &reviewer,
            appeal_until: &appeal_until,
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    // M45-52 step 3: **both sides are told.**
    //
    // The notifier gets the outcome and the statement id; the person whose work or account was acted on gets the reasons and
    // the deadline they can appeal by. Until this existed the reporter got an answer and the author got a sanction with no
    // explanation, which is the asymmetry the row was filed for. The existing notification machinery carries both — no
    // second inbox — and an account that muted this event kind is honoured by `notify` itself.
    if let Some(statement) =
        darkhold_db::governance::statement_by_id(state.db(), &statement_id)
            .await
            .map_err(|e| ApiError(AppError::Internal(e.into())))?
    {
        if let Some(report_id) = statement.report_id.clone() {
            if let Some((subject_type, subject_id, reporter)) =
                darkhold_db::governance::report_subject(state.db(), &report_id)
                    .await
                    .map_err(|e| ApiError(AppError::Internal(e.into())))?
            {
                let _ = darkhold_db::notifications::notify(
                    state.db(),
                    &reporter,
                    "report_decided",
                    "Your report was decided",
                    &format!(
                        "Outcome: {}. Ground: {}. Statement of reasons: {statement_id}.",
                        body.outcome,
                        ground.as_str()
                    ),
                    None,
                )
                .await;

                let author = if subject_type == "work" {
                    darkhold_db::governance::work_owner_account(state.db(), &subject_id)
                        .await
                        .map_err(|e| ApiError(AppError::Internal(e.into())))?
                } else {
                    Some(subject_id.clone())
                };
                if let Some(author) = author {
                    let _ = darkhold_db::notifications::notify(
                        state.db(),
                        &author,
                        "moderation_decision",
                        "A moderation decision concerns you",
                        &format!(
                            "{}. Ground: {}. You may appeal until {appeal_until}.",
                            body.facts,
                            ground.as_str()
                        ),
                        if subject_type == "work" {
                            Some(subject_id.as_str())
                        } else {
                            None
                        },
                    )
                    .await;
                }
            }
        }
    }

    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &reviewer,
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            // The outcome **is** the reason: a review was decided, and what it
            // was decided as is why.
            reason: Some(&body.outcome),
            action: "review.decided",
            subject_type: "task",
            subject_id: &task_id,
            document: &json!({
                "outcome": body.outcome,
                "ground": ground.as_str(),
                "statement_id": statement_id,
            })
            .to_string(),
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({
        "ok": true,
        "statement_id": statement_id,
        "ground": ground.as_str(),
        "appeal_until": appeal_until,
    })))
}

/// `GET /statements/summary` — counts per ground and outcome (M45-52 step 5).
///
/// Public on purpose: Article 24's transparency obligation is owed to the public, not to the operator. What it is *not*
/// is a list — no statement ids, no accounts, no report ids. A cell smaller than `TRANSPARENCY_MIN_COUNT` is reported as
/// suppressed with the threshold named, because a count of one is a person.
async fn statements_summary(
    State(state): State<AppState>,
    // Public, and the extractor is not decoration: it is how this tree records that a door was *offered* to an anonymous
    // caller rather than only to a session (the route inventory checks every public door takes it).
    MaybeSession(_viewer): MaybeSession,
) -> ApiResult<Json<Value>> {
    let rows = darkhold_db::governance::statement_summary(state.db())
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    let mut by_ground: Vec<Value> = Vec::new();
    for ground in darkhold_domain::governance::StatementGround::ALL {
        let cells: Vec<&(String, String, i64)> = rows
            .iter()
            .filter(|(name, _, _)| name == ground.as_str())
            .collect();
        if cells.is_empty() {
            continue;
        }
        let outcomes: Vec<Value> = cells
            .iter()
            .map(|(_, outcome, count)| {
                if *count < darkhold_domain::governance::TRANSPARENCY_MIN_COUNT {
                    json!({
                        "outcome": outcome,
                        "count": Value::Null,
                        "suppressed": true,
                        "reason": format!(
                            "below the transparency threshold of {}",
                            darkhold_domain::governance::TRANSPARENCY_MIN_COUNT
                        ),
                    })
                } else {
                    json!({ "outcome": outcome, "count": count, "suppressed": false })
                }
            })
            .collect();
        let published: i64 = cells
            .iter()
            .filter(|(_, _, count)| *count >= darkhold_domain::governance::TRANSPARENCY_MIN_COUNT)
            .map(|(_, _, count)| count)
            .sum();
        by_ground.push(json!({
            "ground": ground.as_str(),
            "outcomes": outcomes,
            "published_total": published,
        }));
    }

    Ok(Json(json!({
        "min_count": darkhold_domain::governance::TRANSPARENCY_MIN_COUNT,
        "grounds": by_ground,
    })))
}

// ---------------------------------------------------------------------------
// Sanctions (spec §19.5)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct IssueSanctionBody {
    account: String,
    kind: String,
    reason_ref: String,
    duration_days: Option<i64>,
}

async fn issue_sanction(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<IssueSanctionBody>,
) -> ApiResult<Json<Value>> {
    let issuer = user.account_id.to_string();
    require_scoped_bans(&state, &issuer).await?;

    // THE IRREVERSIBLE CASE IS NOT ONE CALL (ledger 3.4).
    //
    // A sanction with no `duration_days` never expires, and `lift` is the only thing that can
    // undo it. Sending it through this one-shot route is exactly the accident the two-phase
    // exists to prevent, so it is refused with a message that names the route to use instead —
    // a refusal a caller cannot read is indistinguishable from a bug.
    if body.duration_days.is_none() {
        return Err(ApiError(AppError::field(
            "duration_days",
            "a sanction with no expiry is irreversible, so it takes two calls: POST /sanctions/file, then POST /sanctions/{id}/execute with the returned id",
        )));
    }

    let ends_at = body.duration_days.map(|d| {
        let end = time::OffsetDateTime::now_utc() + time::Duration::days(d);
        format_rfc3339(end)
    });

    let id = darkhold_db::governance::issue_sanction(
        state.db(),
        &body.account,
        &body.kind,
        &body.reason_ref,
        &issuer,
        ends_at.as_deref(),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &issuer,
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            // A sanction without a reason is a sanction nobody can contest.
            reason: Some(&body.reason_ref),
            action: "sanction.issued",
            subject_type: "account",
            subject_id: &body.account,
            document: &json!({ "sanction_id": id, "kind": body.kind, "days": body.duration_days })
                .to_string(),
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

/// The one door for "may this account issue a scoped ban?" (spec §14.4).
///
/// A helper rather than three copies: the two-phase routes and the one-shot route must not be
/// able to drift into different privilege requirements, and the drift would be invisible —
/// each handler reads correctly on its own.
async fn require_scoped_bans(state: &AppState, issuer: &str) -> ApiResult<()> {
    // Spec §14.4: a **scoped ban** is a senior reviewer's privilege.
    //
    // **The nominal level is 5 and the elastic allocation is what keeps a small site governable**
    // (spec §14.4): on an instance with no senior reviewer the requirement falls to the level that
    // actually has somebody in it, and it returns to 5 on the next evaluation once a senior
    // reviewer appears.
    let (level, required) =
        crate::auth::privilege_check(state, issuer, darkhold_domain::trust::Privilege::ScopedBans)
            .await
            .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if level < required {
        return Err(ApiError(AppError::TrustLevelInsufficient {
            level,
            required,
        }));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct FileSanctionBody {
    account: String,
    kind: String,
    reason_ref: String,
    /// Absent means permanent. That is allowed HERE and refused on the one-shot route: filing a
    /// permanent case is reversible (a pending case bites nobody), executing it is not.
    duration_days: Option<i64>,
}

/// `POST /sanctions/file` — phase one. Audited, pending, and inert.
async fn file_sanction(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<FileSanctionBody>,
) -> ApiResult<Json<Value>> {
    let filer = user.account_id.to_string();
    require_scoped_bans(&state, &filer).await?;

    let ends_at = body.duration_days.map(|d| {
        let end = time::OffsetDateTime::now_utc() + time::Duration::days(d);
        format_rfc3339(end)
    });

    let id = darkhold_db::governance::file_sanction(
        state.db(),
        &body.account,
        &body.kind,
        &body.reason_ref,
        &filer,
        ends_at.as_deref(),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &filer,
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            reason: Some(&body.reason_ref),
            action: "sanction.filed",
            subject_type: "account",
            subject_id: &body.account,
            document: &json!({
                "sanction_id": id,
                "kind": body.kind,
                "days": body.duration_days,
                "state": "pending",
            })
            .to_string(),
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({
        "id": id,
        "state": "pending",
        // Said out loud because a client that treats this as done will never call execute, and the
        // account would stay unsanctioned while the operator believes otherwise.
        "next": "POST /sanctions/{id}/execute",
    })))
}

/// `POST /sanctions/{id}/execute` — phase two. Applies a filed case, once.
async fn execute_sanction(
    State(state): State<AppState>,
    Path(sanction_id): Path<String>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let executor = user.account_id.to_string();
    require_scoped_bans(&state, &executor).await?;

    let applied = darkhold_db::governance::execute_sanction(state.db(), &sanction_id, &executor)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    if !applied {
        // Not pending: unknown, already executed, or lifted before it ran. A conflict rather than a
        // quiet success, because "executed" is a claim an operator will rely on.
        let state_now = darkhold_db::governance::sanction_state(state.db(), &sanction_id)
            .await
            .map_err(|e| ApiError(AppError::Internal(e.into())))?;
        return Err(ApiError(AppError::field(
            "sanction",
            match state_now {
                Some(s) => format!(
                    "sanction {sanction_id} is not pending (state: {s}); it was already executed or lifted, and executing twice would record an action that changed nothing"
                ),
                None => format!("sanction {sanction_id} does not exist"),
            },
        )));
    }

    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &executor,
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            reason: Some("executed a filed sanction"),
            action: "sanction.executed",
            subject_type: "sanction",
            subject_id: &sanction_id,
            document: &json!({ "state": "active" }).to_string(),
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": sanction_id, "state": "active" })))
}

async fn lift_sanction(
    State(state): State<AppState>,
    Path(sanction_id): Path<String>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let lifter = user.account_id.to_string();
    // Lifting a ban is the same authority as issuing one.
    let (level, required) = crate::auth::privilege_check(
        &state,
        &lifter,
        darkhold_domain::trust::Privilege::ScopedBans,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if level < required {
        return Err(ApiError(AppError::TrustLevelInsufficient {
            level,
            required,
        }));
    }
    darkhold_db::governance::lift_sanction(state.db(), &sanction_id, &lifter)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &lifter,
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            reason: None,
            action: "sanction.lifted",
            subject_type: "sanction",
            subject_id: &sanction_id,
            document: "{}",
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({ "ok": true })))
}

async fn my_sanctions(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let items = darkhold_db::governance::active_sanctions(state.db(), &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({ "items": items })))
}

// ---------------------------------------------------------------------------
// Appeals (spec §19.10)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OpenAppealBody {
    sanction_id: String,
    statement: String,
}

async fn open_appeal(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<OpenAppealBody>,
) -> ApiResult<Json<Value>> {
    let appellant = user.account_id.to_string();

    // M45-52 step 4: **the appeal window is a field, not a convention.**
    //
    // The deadline was stored when the decision was written, so it cannot move afterwards. A sanction whose reasons were
    // filed as `statement:<id>` inherits that deadline; one issued without reasons has no window to check, which is itself
    // the honest state (and the row that fixes it is step 3's notification, not a guessed default here).
    let (owner, reason_ref) = darkhold_db::governance::sanction_subject(state.db(), &body.sanction_id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?
        .ok_or_else(|| ApiError(AppError::NotFound { resource: "sanction" }))?;
    // A stranger learns nothing about a sanction that is not theirs — not even that it exists.
    if owner != appellant {
        return Err(ApiError(AppError::NotFound { resource: "sanction" }));
    }
    if let Some(statement_id) = reason_ref.strip_prefix("statement:") {
        if let Some(statement) =
            darkhold_db::governance::statement_by_id(state.db(), statement_id)
                .await
                .map_err(|e| ApiError(AppError::Internal(e.into())))?
        {
            let deadline = time::OffsetDateTime::parse(
                &statement.appeal_until,
                &time::format_description::well_known::Rfc3339,
            )
            .map_err(|e| ApiError(AppError::Internal(anyhow::anyhow!(e.to_string()))))?;
            if time::OffsetDateTime::now_utc() > deadline {
                return Err(ApiError(AppError::field(
                    "appeal_until",
                    format!(
                        "the appeal window for this decision closed at {}",
                        statement.appeal_until
                    ),
                )));
            }
        }
    }

    let id = darkhold_db::governance::open_appeal(
        state.db(),
        &body.sanction_id,
        &appellant,
        &body.statement,
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &appellant,
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            reason: Some(&body.statement),
            action: "appeal.opened",
            subject_type: "sanction",
            subject_id: &body.sanction_id,
            document: &json!({ "appeal_id": id }).to_string(),
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({ "id": id })))
}

#[derive(Debug, Deserialize)]
struct DecideAppealBody {
    decision: String,
}

async fn decide_appeal(
    State(state): State<AppState>,
    Path(appeal_id): Path<String>,
    RequireSession(user): RequireSession,
    Json(body): Json<DecideAppealBody>,
) -> ApiResult<Json<Value>> {
    let decider = user.account_id.to_string();

    // The decider must not be the issuer of the sanction (independence).
    // Independence checking against the audit log lands with the M14
    // quorum work; for now the decision is recorded as given.
    darkhold_db::governance::decide_appeal(state.db(), &appeal_id, &body.decision, &decider)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &decider,
            actor_pseud: user.pseud_id.map(|id| id.to_string()).as_deref(),
            reason: Some(&body.decision),
            action: "appeal.decided",
            subject_type: "appeal",
            subject_id: &appeal_id,
            document: &json!({ "decision": body.decision }).to_string(),
        },
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({ "ok": true })))
}

async fn list_my_appeals(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let appeals = darkhold_db::governance::list_appeals(state.db(), &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({ "items": appeals })))
}

// ---------------------------------------------------------------------------
// Trust (spec §19.1)
// ---------------------------------------------------------------------------

/// The modlog: every moderator, curator and operator action, newest first
/// (spec §14.6).
///
/// **Anonymous access is 401, not 403.** "403 answers 'yes, and you may not'"
/// — but the modlog is not a door that exists only for the permitted. It is a
/// public record that any reader of the instance may read, and being
/// anonymous is a fact about the *reader* rather than about the record. A 403
/// would tell an anonymous caller that the modlog exists and that they may
/// not see it, which is an answer to a question the specification says they
/// may ask.
///
/// **The acting account is never published.** The actor is the acting
/// pseudonym, and a subject that is an account is published as that account's
/// public face. The account id stays where it belongs: in the instance's own
/// audit.
async fn modlog(
    State(state): State<AppState>,
    RequireSession(_user): RequireSession,
    Query(q): Query<ModlogQuery>,
) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let after = q.cursor.as_deref().map(crate::cursor::decode).transpose()?;
    let entries = darkhold_db::governance::list_modlog(
        state.db(),
        limit,
        after.as_ref().map(|(at, id)| (at.as_str(), id.as_str())),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    let next_cursor = if i64::try_from(entries.len()).unwrap_or(i64::MAX) >= limit {
        entries
            .last()
            .map(|entry| crate::cursor::encode(&entry.created_at, &entry.id))
    } else {
        None
    };

    Ok(Json(json!({
        "items": entries
            .into_iter()
            .map(|entry| json!({
                "id": entry.id,
                "actor": {
                    "pseud_id": entry.actor_pseud,
                    "handle": entry.actor_handle,
                    // A panel decision is the instance acting, not a person.
                    "is_panel": entry.actor_is_panel,
                },
                "action": entry.action,
                "target": {
                    "type": entry.subject_type,
                    "id": entry.subject_id,
                    // True when the target was an account and was published as
                    // a pseudonym instead.
                    "is_pseudonym": entry.subject_is_pseudonym,
                },
                "reason": entry.reason,
                "created_at": entry.created_at,
            }))
            .collect::<Vec<_>>(),
        "next_cursor": next_cursor,
    })))
}

/// The query the modlog takes.
#[derive(Debug, Deserialize)]
pub struct ModlogQuery {
    /// How many entries to return.
    pub limit: Option<i64>,
    /// Which page: the cursor the previous page handed back.
    pub cursor: Option<String>,
}

/// An account's trust history: the events, not just the level (spec §6.7).
///
/// **The level is the headline and the history is the answer.** A reader who
/// wants to know why their level is what it is needs the events, and an
/// operator who wants to know why somebody's level fell needs the same events.
async fn account_trust_history(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Query(q): Query<ModlogQuery>,
) -> ApiResult<Json<Value>> {
    // A trust history is the modlog's record of one person: every level change, why, and who made it. It is the
    // account's own, or an operator's — anybody else with a session would be reading somebody's disciplinary record for
    // no reason, and a `404` (not `403`) keeps the question of whether the record exists out of the answer (ADR 0030).
    if user.account_id.to_string() != id
        && !crate::routes::admin_scope::holds_estate_claim(&state, &user)
    {
        return Err(ApiError(AppError::NotFound {
            resource: "account",
        }));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let events = darkhold_db::trust_events::list_for_account(state.db(), &id, limit)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    let level = darkhold_db::governance::trust_for(state.db(), &id)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    Ok(Json(json!({
        "account_id": id,
        "level": level,
        "level_name": state.config().trust.ladder().name_of(level),
        "events": events
            .into_iter()
            .map(|event| json!({
                "id": event.id,
                "from_level": event.from_level,
                "to_level": event.to_level,
                "cause": event.cause.as_str(),
                "evidence_ref": event.evidence_ref,
                "created_at": event.created_at,
            }))
            .collect::<Vec<_>>(),
    })))
}

async fn my_trust(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let level = darkhold_db::governance::trust_for(state.db(), &account)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    let description = match level {
        0 => "New account",
        1 => "Established participant",
        2 => "Regular participant",
        3 => "Reviewed trusted contributor",
        4 => "Trained steward",
        5 => "Senior independent reviewer",
        6 => "Explicitly appointed trustee",
        _ => "Unknown",
    };

    // What produced the level, in the one term a reader can act on. A trust page that shows a number and no evidence asks
    // the reader to trust the ladder; `confirmed_spam_reports` is the input a reader can avoid feeding, and it was a query
    // with a test and no caller (M70-13).
    let confirmed_spam = darkhold_db::governance::confirmed_spam_reports(state.db(), &account, 90)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    Ok(Json(json!({
        "level": level,
        "description": description,
        "evidence": { "confirmed_spam_reports_90d": confirmed_spam },
    })))
}

async fn my_audit_log(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let items =
        darkhold_db::governance::list_audit_log(state.db(), &user.account_id.to_string(), 50)
            .await
            .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    Ok(Json(json!({ "items": items })))
}

#[cfg(test)]
mod submit_report_tests {
    use super::*;

    #[test]
    fn every_contract_code_is_accepted() {
        for code in REPORT_REASONS {
            assert!(validate_reason(code).is_ok(), "{code} must be accepted");
        }
    }

    #[test]
    fn an_invented_reason_is_rejected_and_the_error_names_the_alternatives() {
        let err = validate_reason("vexatious").expect_err("an invented code must not be accepted");
        match err {
            AppError::Validation {
                message,
                field_errors,
            } => {
                assert!(
                    message.contains("vexatious"),
                    "message names the rejected code: {message}"
                );
                let hint = field_errors
                    .get("reason")
                    .expect("the reason field carries the alternatives");
                for code in REPORT_REASONS {
                    assert!(hint.contains(code), "the field error lists {code}: {hint}");
                }
            }
            other => panic!("expected a Validation error, got {other:?}"),
        }
    }

    #[test]
    fn rejection_is_case_sensitive_because_the_contract_is_lowercase() {
        // A client sending "Spam" is not sending the contract's code; accepting it would put a second spelling of the
        // same reason in the queue, which is the drift this check exists to stop.
        assert!(validate_reason("Spam").is_err());
        assert!(validate_reason("").is_err());
    }

    #[test]
    fn the_operator_only_set_is_a_subset_of_the_contract() {
        for code in OPERATOR_ONLY_REASONS {
            assert!(
                REPORT_REASONS.contains(&code),
                "{code} must also be a report reason"
            );
        }
    }
}

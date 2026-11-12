//! M45-55 — compliance tooling: the forms this instance publishes, the reasons templates its reviewers write from, and a
//! subject-access path for the operator that is **audited**.
//!
//! ```text
//! GET  /notices/forms                     — the notice forms, as data (public)
//! GET  /statements/templates              — one reasons template per ground (operator)
//! GET  /admin/subjects/{account}/data     — an operator's subject-access read, with its legal basis (operator, audited)
//! ```
//!
//! # Why the forms are served rather than drawn
//!
//! `POST /reports` already validates two regimes: the DSA's Article 16 notice and 17 U.S.C. 512(c)(3)'s four statutory
//! elements. A frontend that renders a form from its own copy of those fields is a second declaration of the same
//! contract, and the two drift the first time one side adds a field — with the failure landing on a complainant whose
//! notice is refused for a field the UI never asked for. So the shape is served, from the same module that enforces it.
//!
//! # Why the operator's read is a different route from `GET /me/data`
//!
//! `GET /me/data` is the reader asking about themselves: one route, session-scoped, no path parameter, no audit entry
//! needed because nothing is disclosed to anyone else. An operator asking *what do you hold on reader X* is a different
//! act with a different legal basis, and it is the act that must be **recorded**: who looked, at whose data, and under
//! which basis. A tool that can do that silently is the defect this route exists to avoid — so the basis is a required
//! parameter from a closed set, the refusal names it, and every successful read appends to the audit log.
//!
//! # Why the templates live beside the vocabulary
//!
//! A statement of reasons is only as good as the reviewer's paragraph, and the paragraph is written under time pressure.
//! One template per ground, each carrying the placeholders the route fills, means a new ground cannot ship without prose:
//! the test walks `StatementGround::ALL` and fails if any ground has no template, or a template has no `{facts}`.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::governance::StatementGround;
use darkhold_domain::trust::Privilege;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notices/forms", get(notice_forms))
        .route("/statements/templates", get(statement_templates))
        .route("/admin/subjects/{account}/data", get(subject_access))
}

/// The notice forms, as data.
///
/// Public on purpose: a complainant needs the form before they have an account, and a form behind a sign-in is a form
/// most complainants never fill in.
async fn notice_forms(
    // The route inventory requires every door to carry the state, and this one takes it without using it: the forms are the
    // same for every instance that runs this build. Underscored rather than threaded through, because an unused binding
    // that looks used is worse than an obviously unused one.
    State(_state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "forms": [
            {
                "kind": "dsa",
                "title": "Notice of illegal content",
                "legal_basis": "Regulation (EU) 2022/2065, Article 16",
                "fields": [
                    { "name": "subject_type", "required": true, "kind": "choice", "choices": ["work", "comment", "account", "collection"] },
                    { "name": "subject_id", "required": true, "kind": "text" },
                    { "name": "reason", "required": true, "kind": "choice", "choices": ["spam", "harassment", "copyright", "inappropriate_content", "positivity_violation", "safety_concern", "other"] },
                    { "name": "detail", "required": false, "kind": "paragraph",
                      "help": "What is wrong, and where. A notice that names nothing specific cannot be acted on." }
                ],
                "endpoint": "POST /api/v1/reports",
            },
            {
                "kind": "dmca",
                "title": "Copyright notice (17 U.S.C. 512(c)(3))",
                "legal_basis": "17 U.S.C. 512(c)(3)",
                "fields": [
                    { "name": "subject_type", "required": true, "kind": "choice", "choices": ["work", "comment"] },
                    { "name": "subject_id", "required": true, "kind": "text" },
                    { "name": "reason", "required": true, "kind": "choice", "choices": ["copyright"] },
                    { "name": "kind", "required": true, "kind": "hidden", "value": "dmca" },
                    { "name": "copyrighted_work", "required": true, "kind": "text",
                      "help": "Identify the work you own." },
                    { "name": "infringing_material", "required": true, "kind": "paragraph",
                      "help": "Identify the material claimed to infringe, specifically enough to find it." },
                    { "name": "good_faith_statement", "required": true, "kind": "paragraph",
                      "help": "A statement that you have a good faith belief the use is not authorised." },
                    { "name": "signature", "required": true, "kind": "text",
                      "help": "Your name or electronic signature." }
                ],
                "endpoint": "POST /api/v1/reports",
            }
        ],
    })))
}

/// One reasons template per ground.
///
/// Operator-only: the templates are the instance's voice in a sanction, and a reader does not need to know how the
/// reviewer's paragraph was drafted. Every ground in the vocabulary appears, which is the property the test enforces —
/// a ground with no template would be a decision written from nothing.
async fn statement_templates(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    let (level, required) =
        crate::auth::privilege_check(&state, &account, Privilege::ResolveReports)
            .await
            .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;
    if level < required {
        return Err(ApiError(
            darkhold_domain::AppError::TrustLevelInsufficient { level, required },
        ));
    }

    let templates: Vec<Value> = StatementGround::ALL
        .into_iter()
        .map(|ground| {
            json!({
                "ground": ground.as_str(),
                "requires_legal_ground": ground.requires_legal_ground(),
                "template": template_for(ground),
                "placeholders": placeholders_for(ground),
            })
        })
        .collect();

    Ok(Json(json!({ "templates": templates })))
}

/// The reviewer's paragraph, drafted per ground.
fn template_for(ground: StatementGround) -> &'static str {
    match ground {
        StatementGround::IllegalContent => {
            "The material at {location} falls within {legal_ground}. In particular: {facts}. \
             It was removed rather than restricted because the claim is one of unlawfulness, not of taste."
        }
        StatementGround::IncompatibleWithTerms => {
            "This instance's terms refuse {rule}. The material at {location} does that because {facts}. \
             The content is lawful elsewhere; this is a decision about this instance's rules."
        }
        StatementGround::Spam => {
            "The account's activity at {location} is unsolicited bulk or machine-generated content: {facts}. \
             Volume, repetition and the absence of engagement are the facts that carry this."
        }
        StatementGround::Copyright => {
            "The material at {location} reproduces {work} without authorisation. {facts}. \
             The notice was assessed under {legal_ground}; the complainant's good-faith statement and signature are \
             recorded with the notice."
        }
    }
}

/// The placeholders a template uses, so a caller can fill them rather than guess.
fn placeholders_for(ground: StatementGround) -> Vec<&'static str> {
    let mut names = vec!["facts", "location"];
    if ground.requires_legal_ground() {
        names.push("legal_ground");
    }
    if matches!(ground, StatementGround::Copyright) {
        names.push("work");
    }
    names
}

#[derive(Debug, Deserialize)]
struct SubjectAccessQuery {
    /// Why this read is permitted. Required, from a closed set — an operator read with no recorded basis is the thing
    /// this route exists not to allow.
    #[serde(default)]
    basis: String,
}

/// Bases an operator may invoke. GDPR Article 6's grounds, minus the ones that cannot apply to an operator reading a
/// reader's data on the reader's behalf.
const SUBJECT_ACCESS_BASES: [&str; 4] = [
    "legal_obligation",
    "legal_claim",
    "vital_interests",
    "consent",
];

/// An operator's subject-access read, with its basis recorded.
async fn subject_access(
    State(state): State<AppState>,
    session: RequireSession,
    Path(account): Path<String>,
    Query(query): Query<SubjectAccessQuery>,
) -> ApiResult<Response> {
    // **The operator's, not a moderator's.** This route returns a subject's assembled personal data — a full DSAR dump — and it used to be gated
    // by `Privilege::ResolveReports`, i.e. by trust level alone: a trust-6 moderator could read anybody's account. Resolving reports and
    // disclosing a DSAR are different acts, and the second is one the instance can be answerable for, so it takes `require_admin_of` (ADR 0030:
    // `404` to a caller with no standing, because confirming the surface exists is itself a disclosure). The legal-basis requirement and the audit
    // receipt below are unchanged — they were the right shape, attached to the wrong gate.
    crate::routes::admin_scope::require_admin_of(&state, &session.0, None).await?;
    let actor = session.0.account_id.to_string();

    let basis = query.basis.trim();
    if basis.is_empty() {
        return Err(ApiError(darkhold_domain::AppError::field(
            "basis",
            format!(
                "a subject-access read must name its legal basis: one of {}",
                SUBJECT_ACCESS_BASES.join(", ")
            ),
        )));
    }
    if !SUBJECT_ACCESS_BASES.contains(&basis) {
        return Err(ApiError(darkhold_domain::AppError::field(
            "basis",
            format!(
                "unknown basis `{basis}`: one of {}",
                SUBJECT_ACCESS_BASES.join(", ")
            ),
        )));
    }

    // The read itself is the one M45-51 built for the reader's own copy. Reusing it is the point: two assemblers would
    // drift, and the one the operator read would be the one nobody tested.
    let data = darkhold_db::erasure::subject_data(state.db(), &account)
        .await
        .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e)))?;

    // **The audit entry is not optional and not best-effort**: a read that cannot be recorded is a read that must not
    // happen, so a failed append refuses the disclosure rather than answering it silently.
    darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &actor,
            actor_pseud: session.0.pseud_id.map(|id| id.to_string()).as_deref(),
            reason: Some(basis),
            action: "subject.access",
            subject_type: "account",
            subject_id: &account,
            document: &json!({ "basis": basis }).to_string(),
        },
    )
    .await
    .map_err(|e| ApiError(darkhold_domain::AppError::Internal(e.into())))?;

    // The answer is a **document**, not a page: a subject-access response is handed to somebody, archived, and produced
    // again if the regulator asks, so it carries a filename and a date. The reader's own copy (`GET /me/data`) keeps the
    // same body without the attachment header — same assembler, two audiences, one shape.
    let body = json!({ "basis": basis, "subject": data }).to_string();
    let filename = format!("dsar-{account}-{}.json", chrono_stamp());
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    Ok(response)
}

/// A date stamp for the filename, from the same clock the rest of the tree reads.
fn chrono_stamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map(|stamp| stamp.chars().take(10).collect())
        .unwrap_or_else(|_| "undated".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ground_has_a_template_that_carries_the_facts() {
        for ground in StatementGround::ALL {
            let template = template_for(ground);
            assert!(
                template.contains("{facts}"),
                "a template without the reviewer's facts is a template that writes the same decision twice: {}",
                ground.as_str()
            );
            assert!(
                !template.trim().is_empty(),
                "{} has no template",
                ground.as_str()
            );
        }
    }

    #[test]
    fn a_lawful_grounds_template_demands_the_instrument() {
        for ground in StatementGround::ALL {
            let placeholders = placeholders_for(ground);
            assert_eq!(
                placeholders.contains(&"legal_ground"),
                ground.requires_legal_ground(),
                "{} and its placeholders disagree about whether law must be named",
                ground.as_str()
            );
        }
    }
}

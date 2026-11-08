//! Who may act on an admin surface, and which site they may act on (ADR 0003).
//!
//! Three authorities exist in this estate and they are not interchangeable:
//!
//! * `role:operator` from polaris-id — reaches every site. A claim, asserted at sign-in.
//! * `role:moderator:<site>` / `role:curator:<site>` — reaches that site and no other. Also a claim.
//! * this instance's earned `trust` level, and the configured operator account — site-local, and the reason a break-glass
//!   login keeps working when the issuer cannot be reached.
//!
//! The site a door belongs to is a **constant on the route**, never a request parameter: a moderator of one site can type
//! any parameter, so scope read from the request would be a suggestion rather than a rule.
//!
//! Refusals are audited through the same `audit_append` every other moderation action uses, so "did anyone try to act on
//! the wrong site" is answerable from the modlog and not from a log file that may have rotated.

use crate::auth::SessionUser;
use crate::http::ApiError;
use crate::state::AppState;
use darkhold_domain::error::AppError;

/// Whether the caller may act on `site` (`None` = the whole estate).
///
/// Returns 403 on refusal, never 401: the caller *is* authenticated, and telling a signed-in moderator to sign in again is
/// the least useful answer available.
pub async fn require_admin_of(
    state: &AppState,
    user: &SessionUser,
    site: Option<&str>,
) -> Result<(), ApiError> {
    if !require_admin_of_decision(state, user, site).await {
        audit_refusal(state, user, site).await;
        return Err(ApiError(AppError::AccessDenied));
    }
    Ok(())
}

/// The decision alone, so it can be tested without writing audit rows.
pub async fn require_admin_of_decision(
    state: &AppState,
    user: &SessionUser,
    site: Option<&str>,
) -> bool {
    let is_operator = user.roles.iter().any(|r| r == "role:operator");
    if is_operator {
        return true;
    }
    // The configured operator account is this instance's break-glass identity: it must not depend on the issuer being
    // reachable, which is the whole point of it existing.
    if state.config().administration.operator_account_id == Some(user.account_id) {
        return true;
    }
    let Some(site) = site else {
        // A cross-site door is the claim's to open. Local standing cannot widen to other sites, or one community's
        // operator would silently govern another's.
        return false;
    };
    let scoped = user
        .roles
        .iter()
        .any(|r| r == &format!("role:moderator:{site}") || r == &format!("role:curator:{site}"));
    if scoped {
        return true;
    }
    match darkhold_db::governance::trust_for(state.db(), &user.account_id.to_string()).await {
        Ok(level) => level >= 5,
        // A trust lookup that fails must not grant authority. Failing closed here is the only safe answer: the alternative
        // is that a database hiccup escalates every moderator.
        Err(_) => false,
    }
}

async fn audit_refusal(state: &AppState, user: &SessionUser, site: Option<&str>) {
    let actor = user.account_id.to_string();
    let pseud = user.pseud_id.map(|p| p.to_string());
    let document = serde_json::json!({
        "required_site": site.unwrap_or("estate"),
        "roles": user.roles,
    })
    .to_string();
    let _ = darkhold_db::governance::audit_append(
        state.db(),
        &darkhold_db::governance::AuditEntry {
            actor: &actor,
            actor_pseud: pseud.as_deref(),
            reason: Some("wrong_site"),
            action: "admin.refused",
            subject_type: "site",
            subject_id: site.unwrap_or("estate"),
            document: &document,
        },
    )
    .await;
}

/// The synchronous decision for a **cross-site** door: `role:operator` or the break-glass account (ADR 0003).
///
/// Sync because a role lives on the session and needs no I/O; the trust fallback is only for site-local doors, which take the
/// async `require_admin_of` instead. This exists because fourteen files each carried their own six-line copy of "is this the
/// configured operator", and the configured operator is unset on this host — so every one of those doors answered 404 to
/// everyone. One decision, one place, and a door that can now be opened by the person appointed to open it.
///
/// Refusals here are logged rather than written to the modlog: the audit write is async and these call sites are not. The
/// destructive surfaces (retention, sanctions, succession) use the async door and DO audit — see `require_admin_of`.
pub fn estate_admin_allowed(state: &AppState, user: &SessionUser) -> Result<(), ApiError> {
    if holds_estate_claim(state, user) {
        return Ok(());
    }
    let configured = state.config().administration.operator_account_id;
    tracing::debug!(
        operator_configured = configured.is_some(),
        roles = ?user.roles,
        "a cross-site admin door was reached by a caller with no estate claim"
    );
    // Audited without making the signature async: the decision needs no I/O, but the refusal must reach the modlog, and a
    // refusal that only exists in a log file is not answerable six weeks later. Spawning is safe here because the caller is
    // inside a request handler on a tokio runtime; with no runtime (a unit test calling this directly) the write is skipped
    // rather than panicking, and the refusal is still returned.
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let db = state.db().clone();
        let actor = user.account_id.to_string();
        let pseud = user.pseud_id.map(|p| p.to_string());
        let roles = user.roles.clone();
        handle.spawn(async move {
            let document =
                serde_json::json!({ "required_site": "estate", "roles": roles }).to_string();
            let _ = darkhold_db::governance::audit_append(
                &db,
                &darkhold_db::governance::AuditEntry {
                    actor: &actor,
                    actor_pseud: pseud.as_deref(),
                    reason: Some("wrong_site"),
                    action: "admin.refused",
                    subject_type: "site",
                    subject_id: "estate",
                    document: &document,
                },
            )
            .await;
        });
    }
    Err(ApiError(AppError::AccessDenied))
}

/// The same decision, refusing with `NotFound` so the door does not disclose that it exists.
///
/// **This exists because centralising the authority lost the shape.** `estate_admin_allowed` answers `403`, which is right for
/// a door that is allowed to admit it exists (`/admin/retention` is in the manual). A door whose existence *is* the disclosure —
/// the decision audit, the economy dashboard — must answer `404`, and two of them documented exactly that while delegating to
/// the 403 helper, because the refactor that removed fourteen copies of the predicate kept only one refusal shape. The tests
/// that had asserted `404` went red on the next full run, which is the only reason it was noticed at all.
///
/// The resource name is deliberately generic: `AppError::NotFound { resource }` renders the name into the message, so naming
/// the surface here would put the disclosure back in the body of the refusal.
pub fn require_estate_hidden(state: &AppState, user: &SessionUser) -> Result<(), ApiError> {
    if holds_estate_claim(state, user) {
        return Ok(());
    }
    tracing::debug!(
        operator_configured = state.config().administration.operator_account_id.is_some(),
        roles = ?user.roles,
        "a hidden operator door was reached by a caller with no estate claim"
    );
    Err(ApiError(AppError::NotFound { resource: "page" }))
}

/// The same decision as a plain predicate, for doors that must refuse with `NotFound` rather than `403` because confirming
/// their existence to a stranger would map the operator's configuration (see `operator_hit_rate`'s module doc). One
/// authority, two refusal shapes — the shape is a property of the door, the authority is not.
pub fn holds_estate_claim(state: &AppState, user: &SessionUser) -> bool {
    user.roles.iter().any(|r| r == "role:operator")
        || state.config().administration.operator_account_id == Some(user.account_id)
}

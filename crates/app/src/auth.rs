//! Sessions, cookies and CSRF.
//!
//! Spec §3.5 fixes the shape:
//!
//! * opaque server-managed sessions in secure cookies,
//! * `HttpOnly`, `Secure` in production, appropriate `SameSite`, narrow path,
//!   explicit expiry and revocation,
//! * CSRF protection for state-changing cookie-authenticated requests.
//!
//! The design here is the "double submit with a server-side record" pattern:
//! the CSRF token lives in a **readable** cookie so the client can send it back
//! in a header, and the *hash* of that token is stored on the session row. A
//! cross-site attacker can cause the cookie to be sent, but cannot read it, so
//! it cannot produce the matching header. Storing the hash server-side means a
//! token lifted from one session cannot be replayed against another.
//!
//! Cookies are parsed and written by hand rather than with a helper crate,
//! because the rules above are the whole reason this module exists and it is
//! worth being able to read them in one place.

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use darkhold_db::identity::Account;
use darkhold_db::sessions::{self, Session};
use darkhold_domain::policy::AgeState;
use darkhold_domain::{AccountId, AppError, PseudId, SessionId};
use std::str::FromStr;

use crate::crypto;
use crate::http::ApiError;
use crate::state::AppState;

/// Cookie carrying the session token. `HttpOnly`.
pub const SESSION_COOKIE: &str = "darkhold_session";

/// Cookie carrying the CSRF token. Deliberately readable by scripts.
pub const CSRF_COOKIE: &str = "darkhold_csrf";

/// Header the client must echo the CSRF token in.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// Whether this account holds this privilege on this instance.
///
/// **The one trust gate.** A route asks for the *privilege* it needs and does
/// not know what number that is — the level is the ladder's business and the
/// ladder is configuration (spec §14.4). A route that compared against a
/// constant would be a route whose gate could only be changed by changing the
/// code.
///
/// **The effective requirement, not the nominal one.** The stored requirement
/// is the one the elastic allocation produced: when nobody holds a privilege,
/// the requirement falls (spec §14.4), and a route that used the nominal
/// level would refuse the level-2 account a small site relies on. The stored
/// requirement is what the instance actually decided, and it is read here.
///
/// The stored requirement is used when there is one, and the **configured**
/// requirement when there is not — an instance that has never evaluated its
/// requirements falls back to the ladder rather than refusing everything.
pub async fn holds_privilege(
    state: &AppState,
    account: &str,
    privilege: darkhold_domain::trust::Privilege,
) -> Result<bool, sqlx::Error> {
    let (level, required) = privilege_check(state, account, privilege).await?;
    Ok(level >= required)
}

/// The account's level and the level this privilege requires.
///
/// **Both numbers, because a refusal that names neither the level found nor
/// the bar required is not actionable** — and spec §0.3 makes the thresholds
/// visible by design. A reader told "trust level 2; this account is at 1"
/// knows what to work toward; a reader told "access denied" knows nothing.
pub async fn privilege_check(
    state: &AppState,
    account: &str,
    privilege: darkhold_domain::trust::Privilege,
) -> Result<(i64, i64), sqlx::Error> {
    let level = darkhold_db::governance::trust_for(state.db(), account).await?;
    let config = state.config();
    let requirement = darkhold_db::governance::effective_requirement(
        state.db(),
        config.trust.ladder(),
        &config.trust.require,
        &config.trust.elastic_policy(),
        privilege,
    )
    .await?;
    Ok((level, requirement.level))
}

/// The authenticated actor behind a request.
#[derive(Debug, Clone)]
pub struct SessionUser {
    /// The account that owns the session.
    pub account_id: AccountId,
    /// The session itself, so it can be revoked or switched.
    pub session_id: SessionId,
    /// The pseud this session is acting as, when one has been selected.
    pub pseud_id: Option<PseudId>,
    /// The account's age-policy state, loaded with the session.
    pub age_state: AgeState,
    /// Storage form of the CSRF token bound to this session.
    pub csrf_token_hash: String,
    /// The assurance level the issuer asserted at sign-in, verbatim. `None` for a password session.
    pub acr: Option<String>,
    /// The roles the issuer asserted at sign-in, verbatim (ADR 0003).
    ///
    /// Empty for a password session and for sessions written before migration 0179. Both are "no claim" to every check,
    /// which is why this is a `Vec` and not an `Option`.
    pub roles: Vec<String>,
}

impl SessionUser {
    /// Build the domain actor for a policy decision.
    ///
    /// Returns `None` when no pseud has been resolved, because a policy
    /// decision about "who is this" is meaningless without a public face.
    #[must_use]
    pub fn actor(&self, pseud_id: PseudId) -> darkhold_domain::policy::Actor {
        darkhold_domain::policy::Actor {
            account_id: self.account_id,
            pseud_id,
            age_state: self.age_state,
            trusted_reviewer: false,
        }
    }

    /// Build the **identity** actor: who this request is, in one type.
    ///
    /// This is the Darkhold actor model (spec §6.1, §12.6), and it is a
    /// different thing from [`SessionUser::actor`], which is the *content
    /// check's* actor. The identity actor says **who is asking** — anonymous, a
    /// guest, or a reader acting as a pseudonym — and carries the tier the
    /// request is served on.
    ///
    /// **The tier is computed here, once, from the instance's own
    /// configuration.** The account's trust level and resonance are read from
    /// the account row, and the supporter state from the entitlement; the
    /// boundaries come from `[tiers]`. The result is put on the actor, and
    /// **the reader of the field never computes one** — a second computation
    /// would be a second opinion, and a tier and an explanation that disagree
    /// is the failure spec §12.6 rule 6 exists to prevent.
    #[must_use]
    pub fn identity_actor(
        &self,
        account: &AccountFacts,
        tier_config: &darkhold_domain::actor::TierConfig,
    ) -> darkhold_domain::actor::Actor {
        let kind = if account.is_guest {
            darkhold_domain::actor::ActorKind::Guest
        } else {
            darkhold_domain::actor::ActorKind::Pseud
        };
        let mut actor = darkhold_domain::actor::Actor {
            kind,
            account_id: Some(self.account_id),
            pseud_id: self.pseud_id,
            age_state: self.age_state,
            trust_level: account.trust_level,
            tier: darkhold_domain::actor::Tier::House,
        };
        actor.tier = darkhold_domain::actor::tier_for(&actor, tier_config);
        actor
    }
}

/// The account facts the identity actor needs.
///
/// They live in the store, because they are the instance's copy of them and not
/// a request's claim: [`darkhold_db::identity::AccountFacts`].
pub use darkhold_db::identity::AccountFacts;

/// Resolve a session from the request's cookies, if there is one.
///
/// Never fails: an absent, malformed or expired session simply means the
/// request is anonymous, and the handler decides whether that is acceptable.
/// Returning an error here would turn "not signed in" into a 500.
pub async fn resolve_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<(SessionUser, Session)> {
    let token = read_cookie(headers, SESSION_COOKIE)?;
    let token_hash = crypto::hash_token(&token);
    let now = sessions::now();

    let session =
        match sessions::find_live_session_by_token_hash(state.db(), &token_hash, &now).await {
            Ok(Some(session)) => session,
            Ok(None) => return None,
            Err(error) => {
                // A database fault must not become an authentication bypass, so we
                // treat it as anonymous and record it loudly.
                tracing::error!(%error, "session lookup failed; treating request as anonymous");
                return None;
            }
        };

    let account = match darkhold_db::identity::find_account(state.db(), session.account_id).await {
        Ok(Some(account)) => account,
        Ok(None) => return None,
        Err(error) => {
            tracing::error!(%error, "account lookup failed; treating request as anonymous");
            return None;
        }
    };

    if !matches!(account.status, darkhold_db::identity::AccountStatus::Active) {
        tracing::debug!(
            account = %account.id,
            status = account.status.as_str(),
            "session belongs to a non-active account"
        );
        return None;
    }

    let user = SessionUser {
        account_id: session.account_id,
        session_id: session.id,
        pseud_id: session.active_pseud_id,
        age_state: account.age_state,
        csrf_token_hash: session.csrf_token_hash.clone(),
        acr: session.acr.clone(),
        // Parsed from the stored JSON once, here, so no check has to know how it is stored.
        roles: serde_json::from_str(&session.roles).unwrap_or_default(),
    };
    Some((user, session))
}

/// Whether this instance can ask for a second factor at all.
///
/// Step-up comes from the issuer: this site does not run a second factor of its own, and manufacturing an assurance
/// level locally would be a claim nobody can audit. An instance without `POLARIS_ID_ISSUER` therefore has no step-up
/// to offer.
#[must_use]
pub fn step_up_configured() -> bool {
    std::env::var("POLARIS_ID_ISSUER")
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

/// Refuse an operation whose requirement this session's assurance does not meet (M70-08).
///
/// Three outcomes, and the third is the one worth reading twice:
///
/// * satisfied — carry on;
/// * the session carries an `acr` below the bar — 403 `STEP_UP_REQUIRED`, naming both levels, because "sign in again
///   with your second factor" is actionable and "forbidden" is not;
/// * the session carries **no** `acr`. If the instance has an issuer, that session was not signed in with a second
///   factor and is refused. If it has none, there is nothing to step up to, and refusing would lock every operator
///   out of their own tools — so the guard is *absent* there rather than permissive, which is a decision recorded in
///   ADR 0005 and in M70-08's notes.
pub fn require_step_up(
    session: &SessionUser,
    requirement: darkhold_domain::step_up::Requirement,
) -> Result<(), crate::http::ApiError> {
    use darkhold_domain::step_up::{check, Verdict};
    match check(session.acr.as_deref(), requirement) {
        Verdict::Satisfied => Ok(()),
        Verdict::StepUp { have, need } => Err(crate::http::ApiError(AppError::StepUpRequired {
            have: Some(have),
            need,
        })),
        Verdict::Unknown { need } => {
            if step_up_configured() {
                Err(crate::http::ApiError(AppError::StepUpRequired {
                    have: None,
                    need,
                }))
            } else {
                Ok(())
            }
        }
    }
}

/// Middleware attaching [`SessionUser`] to requests that carry a live session.
pub async fn load_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some((user, _session)) = resolve_session(&state, request.headers()).await {
        // Touch best-effort: activity tracking must never fail a request.
        let now = sessions::now();
        if let Err(error) = sessions::touch_session(state.db(), user.session_id, &now).await {
            tracing::debug!(%error, "could not record session activity");
        }
        request.extensions_mut().insert(user);
    }
    next.run(request).await
}

/// Is this method required to carry a CSRF token?
#[must_use]
pub fn is_state_changing(method: &Method) -> bool {
    !matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

/// CSRF middleware.
///
/// Runs after [`load_session`]. Anonymous requests are not checked: there is no
/// session to ride, and the endpoint will reject them for being unauthenticated
/// anyway. What matters is that a *cookie-authenticated* write cannot be
/// triggered from another origin.
pub async fn verify_csrf(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if !state.config().security.csrf_required {
        return next.run(request).await;
    }
    if !is_state_changing(request.method()) {
        return next.run(request).await;
    }

    let Some(user) = request.extensions().get::<SessionUser>() else {
        return next.run(request).await;
    };

    let Some(supplied) = request
        .headers()
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        tracing::debug!("state-changing request carried no CSRF header");
        return csrf_failure();
    };

    if crypto::hash_token(supplied) != user.csrf_token_hash {
        tracing::warn!("CSRF token did not match the session");
        return csrf_failure();
    }

    next.run(request).await
}

fn csrf_failure() -> Response {
    // Deliberately not `AUTH_REQUIRED`: the client is authenticated, and telling
    // it otherwise would send it to a login page it does not need.
    let body = serde_json::json!({
        "error": {
            "code": "ACCESS_DENIED",
            "message": "This request did not include a valid CSRF token. Reload the page and try again.",
            "request_id": crate::http::current_request_id()
                .map_or_else(|| "unavailable".to_owned(), |id| id.to_string()),
        }
    });
    let mut response = (StatusCode::FORBIDDEN, axum::Json(body)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

/// An extractor for endpoints that require a signed-in account.
///
/// Extracting this *is* the authentication check: a handler that names it
/// cannot be reached anonymously, and there is no way to forget the check
/// because there is no check to forget.
pub struct RequireSession(pub SessionUser);

impl axum::extract::FromRequestParts<AppState> for RequireSession {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<SessionUser>()
            .cloned()
            .map(Self)
            .ok_or_else(|| ApiError(AppError::AuthRequired))
    }
}

/// An extractor that yields the session when there is one and never rejects.
///
/// Used by endpoints a visitor may reach without signing in — reading a
/// published work — where the same URL must answer a contributor with their
/// draft and a stranger with a refusal. Written out rather than borrowing
/// `Option<RequireSession>`, because the middleware has already attached the
/// session to the request and a second, silently-swallowing fallback would be
/// one more thing to reason about.
pub struct MaybeSession(pub Option<SessionUser>);

impl axum::extract::FromRequestParts<AppState> for MaybeSession {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.extensions.get::<SessionUser>().cloned()))
    }
}

/// An extractor for endpoints that require a signed-in account *and* an
/// explicitly selected active pseud.
pub struct RequirePseud {
    /// The account behind the request.
    pub user: SessionUser,
    /// The pseud the request is acting as.
    pub pseud_id: PseudId,
}

impl axum::extract::FromRequestParts<AppState> for RequirePseud {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = parts
            .extensions
            .get::<SessionUser>()
            .cloned()
            .ok_or_else(|| ApiError(AppError::AuthRequired))?;

        let pseud_id = user
            .pseud_id
            .ok_or_else(|| ApiError(AppError::AccessDenied))?;

        Ok(Self { user, pseud_id })
    }
}
use darkhold_domain::api_scopes::Scope;

/// Who is acting on this request, whichever credential proved it.
///
/// A session and a bearer token are two different *credentials* and one
/// *actor*. Before this type existed, every handler took `RequireSession` and
/// so was unreachable by a token — which meant all eight of §23.2's bot actions
/// had real API doors that no bot could walk through, and `MaybeToken` was used
/// in exactly one handler in the tree.
#[derive(Debug, Clone)]
pub struct Actor {
    /// The account the action belongs to. A scope says what this account may
    /// do; the handlers still have to check that the row in question is
    /// *this account's* row.
    pub account_id: AccountId,
    /// The pseud the action is taken as.
    ///
    /// Always resolved, never optional. A token's acting pseud is looked up and
    /// confirmed to belong to the account; a session's is its selected pseud.
    pub pseud_id: PseudId,
    /// Which credential proved it, for handlers that treat the two differently.
    pub via: Credential,
    /// The scopes this credential carries. Empty for a session: a session's
    /// authority was settled at login, so there is no scope set to check
    /// against, and pretending otherwise would mean inventing one.
    pub scopes: Vec<Scope>,
}

impl Actor {
    /// Refuse unless this credential carries `required`.
    ///
    /// A session carries no scope set, so it passes: its authority was settled
    /// at login, and re-checking it against a scope set it never had would lock
    /// every contributor out of their own library. Only a token is limited.
    ///
    /// Named on `Actor` rather than on `RequireActorScoped` so a handler that
    /// destructures the extractor — which is how axum wants it written — can
    /// reach the check without keeping the wrapper alive for it.
    pub fn require(self, required: Scope) -> Result<Self, ApiError> {
        if self.via == Credential::Token
            && !darkhold_domain::api_scopes::has_scope(&self.scopes, &required)
        {
            return Err(missing_scope(required));
        }
        Ok(self)
    }
}

/// The credential behind an [`Actor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    /// A signed-in session. Authorization was settled at login, so scopes do
    /// not apply — this is what `media.rs` already did, and what the browser
    /// relies on.
    Session,
    /// A bearer token, which carries its own scope set.
    Token,
}

/// An extractor for doors that a *bot* must be able to reach.
///
/// It accepts either credential and resolves both to one [`Actor`], so a
/// handler stops caring which arrived. Extracting this *is* the authentication
/// check: a handler naming it cannot be reached anonymously.
///
/// **A token with no acting pseud is refused, not defaulted.** A token belongs
/// to an account but acts as a pseud, and those are different identities the
/// schema keeps strictly apart — a reader may wear several faces, and a bot
/// posting under a face the reader never chose is exactly the confusion
/// §23.1's "explicit acting pseud" exists to prevent. Falling back to the
/// account's default pseud would be the convenient answer and the wrong one: it
/// is a silent substitution of identity, and a token minted before this column
/// existed would get it too.
pub struct RequireActor(pub Actor);

impl axum::extract::FromRequestParts<AppState> for RequireActor {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // A session, when there is one, wins. The browser never sends a bearer
        // token, so this ordering only matters for a client that sends both —
        // and there the session is the more specific, already-authorized
        // credential.
        if let Some(session) = parts.extensions.get::<SessionUser>().cloned() {
            let pseud_id = session
                .pseud_id
                .ok_or_else(|| ApiError(AppError::AccessDenied))?;
            return Ok(Self(Actor {
                account_id: session.account_id,
                pseud_id,
                via: Credential::Session,
                scopes: Vec::new(),
            }));
        }

        let token = resolve_bearer(parts, state).await?;
        let token = token.ok_or_else(|| ApiError(AppError::AuthRequired))?;

        let acting = token
            .acting_pseud_id
            .as_deref()
            .ok_or_else(|| ApiError(AppError::AccessDenied))?;
        let pseud_id = PseudId::from_str(acting).map_err(|_| ApiError(AppError::AccessDenied))?;

        // Confirm the pseud belongs to the account. The token row already ties
        // them together, so this is not paranoia about the token — it is about
        // the pseud having since been deleted or moved, in which case acting as
        // it would attach the request to an identity that no longer exists.
        let pseud = darkhold_db::identity::find_pseud(state.db(), pseud_id)
            .await
            .map_err(|e| ApiError(AppError::internal("looking up a token's acting pseud", e)))?
            .ok_or_else(|| ApiError(AppError::AccessDenied))?;
        if pseud.account_id != token.account_id {
            // A token whose acting pseud belongs to somebody else. The link
            // flow cannot produce this — it validates at issue time — so it means
            // the column was written directly. Refuse rather than act.
            return Err(ApiError(AppError::AccessDenied));
        }

        Ok(Self(Actor {
            account_id: token.account_id,
            pseud_id,
            via: Credential::Token,
            scopes: token.scopes,
        }))
    }
}

/// As [`RequireActor`], and the token must additionally carry `required`.
///
/// The scope check lives here rather than in each handler, so a door cannot
/// forget it — the same argument that made `RequireSession` an extractor in the
/// first place. A session caller bypasses it, exactly as `media.rs` already
/// does: a session's authority was settled at login, and re-checking it against
/// a scope set the session never had would lock every contributor out of their
/// own library.
pub struct RequireActorScoped {
    /// The resolved actor.
    pub actor: Actor,
}

impl axum::extract::FromRequestParts<AppState> for RequireActorScoped {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let RequireActor(actor) = RequireActor::from_request_parts(parts, state).await?;
        Ok(Self { actor })
    }
}

/// Who is asking a door that a bot reaches with its own credential.
///
/// A bot's registration credential is deliberately **not** an [`Actor`]: an actor acts as a *pseud*, and a bot has none — the
/// registration is issued through `issue_token` with `acting_pseud_id` NULL, which is why [`RequireActor`] refuses it and why
/// the first version of the bot-initiated link door answered every bot with `403 ACCESS_DENIED`. The two callers are different
/// kinds of principal, so this enum names them instead of flattening one into the other: a session asks *may I act on this
/// bot*, and the handler answers from the registration's owner; the bot asks *am I this bot*, and the answer is its own id.
#[derive(Debug, Clone)]
pub enum BotCaller {
    /// A signed-in account. Whether it may act on the bot named in the path is the handler's question, answered from the row.
    Session(AccountId),
    /// The bot itself, proven by the credential its registration was created with.
    Bot {
        /// The registration's id — the bot the credential IS.
        bot_id: String,
        /// The account that registered it. Carried so a handler never has to guess whose bot this is.
        account_id: AccountId,
    },
}

/// An extractor for doors a bot must reach with its own credential (§23.2, M45-43).
///
/// Accepts a session **or** the bot's own registration credential, and refuses everything else. It exists because the link
/// flow was owner-initiated only: the browser flow has an owner mint a code for a reader, and a guild bot has no session, so
/// it could not start the flow for the reader in front of it and grew two invented endpoints (`bot/link/start`,
/// `bot/link/exchange` — one of which minted a token the server never agreed to) to fill the gap.
///
/// **A reader's link credential is refused.** `bot_for_token` matches two things on purpose — the registration's own token and
/// a linked reader's `kind = 'bot'` token — because suspension has to reach readers who linked. Minting a code for somebody
/// else is a different act, so this door resolves through `bot_registration_for_token`, which matches the registration alone.
///
/// **A suspended bot is told nothing, and it is not a 401.** A 401 would say "your credential was not recognised as a bot's,
/// try again", when in fact the instance knows exactly what it is and has stopped it; the honest answer is the same `404` an
/// unknown bot gets, because a suspended bot can act on neither fact.
pub struct RequireBotOrOwner(pub BotCaller);

impl axum::extract::FromRequestParts<AppState> for RequireBotOrOwner {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // A session first, for the same reason `RequireActor` prefers it: the browser never sends a bearer token, so a request
        // carrying both is a client that meant the session, and the session is the more specific credential.
        if let Some(session) = parts.extensions.get::<SessionUser>().cloned() {
            return Ok(Self(BotCaller::Session(session.account_id)));
        }

        // The bearer arm is written out rather than reusing `resolve_bearer`, and the difference is the point: `MaybeToken`
        // refuses a suspended bot's token by answering `None`, which reaches the caller as `401 AUTH_REQUIRED` — the one answer
        // this door must not give. Everything else `MaybeToken` does (the hash, the expiry/revocation predicate, the scope and
        // suspended-bot checks, recording the use) has to happen here in the same order, so the two paths cannot disagree about
        // what a token is.
        let Some(raw) = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
        else {
            return Err(ApiError(AppError::AuthRequired));
        };
        let identity =
            darkhold_db::external::resolve_token(state.db(), &crate::crypto::hash_token(raw))
                .await
                .map_err(|e| ApiError(AppError::internal("resolving a bearer token", e)))?
                // Unknown, revoked and expired all answer the same thing: this credential does not authenticate anything.
                .ok_or_else(|| ApiError(AppError::AuthRequired))?;
        if let Err(err) = darkhold_db::external::touch_token(state.db(), &identity.token_id).await {
            // Bookkeeping, not authorization: refusing a valid call because the audit write failed would turn a reporting
            // fault into an outage. Same trade `MaybeToken` makes, for the same reason.
            tracing::error!(error = %err, "recording a token's last use failed");
        }

        let (bot_id, bot_state) =
            darkhold_db::external::bot_registration_for_token(state.db(), &identity.token_id)
                .await
                .map_err(|e| {
                    ApiError(AppError::internal(
                        "resolving a token's bot registration",
                        e,
                    ))
                })?
                // A personal token, a reader's link credential, or a credential minted before bots existed: authenticated, and not
                // a bot. `403` rather than `401` because the credential *is* good and the caller can be told which question failed,
                // and no scope is named because no scope would have granted it.
                .ok_or_else(|| ApiError(AppError::AccessDenied))?;
        if bot_state != "active" {
            tracing::warn!(%bot_id, %bot_state, "a suspended bot presented its credential at a bot door; answering as unknown");
            return Err(ApiError(AppError::NotFound { resource: "bot" }));
        }
        let account_id = AccountId::from_str(&identity.account_id)
            .map_err(|_| ApiError(AppError::AuthRequired))?;
        Ok(Self(BotCaller::Bot { bot_id, account_id }))
    }
}

/// The 403 for a token that lacks `required`.
///
/// Names the scope on purpose: §23.1's contract is that a token's scope set is
/// the whole of its authority, so the bot already knows the set — and "access
/// denied" would send an operator looking for a permission bug rather than a
/// token that was granted too little at link time.
fn missing_scope(required: Scope) -> ApiError {
    ApiError(AppError::MissingScope {
        scope: required.as_str(),
    })
}

/// The bearer token on this request, if one resolves.
///
/// Shared by [`RequireActor`] and `MaybeToken` so there is one answer to "what
/// does this token say" — two implementations would drift, and the drift would
/// be invisible until one of them accepted something the other refused.
pub(crate) async fn resolve_bearer(
    parts: &mut axum::http::request::Parts,
    state: &AppState,
) -> Result<Option<TokenUser>, ApiError> {
    use axum::extract::FromRequestParts as _;
    // `MaybeToken`'s rejection is `Infallible` by construction — it never
    // refuses, it only resolves or declines to — so the match here is total and
    // the `unreachable!` arm is genuinely unreachable.
    let MaybeToken(token) = match MaybeToken::from_request_parts(parts, state).await {
        Ok(maybe) => maybe,
        Err(never) => match never {},
    };
    Ok(token)
}

/// Identity resolved from a Bearer API token.
#[derive(Debug, Clone)]
pub struct TokenUser {
    pub account_id: AccountId,
    pub scopes: Vec<Scope>,
    /// The token's row id, so `last_used_at` can be recorded against the exact
    /// token that proved it works (D3).
    pub token_id: String,
    /// Spec §23.1's "explicit acting pseud". A token belongs to an account but
    /// acts as a pseud, and the two are different identities the rest of the
    /// schema keeps apart. `None` for a token issued before the column existed,
    /// which is a refusal rather than a fallback to the account's default pseud
    /// — see `RequireActor`.
    pub acting_pseud_id: Option<String>,
}

/// Resolve a Bearer token when present, without rejecting the request.
pub struct MaybeToken(pub Option<TokenUser>);

impl axum::extract::FromRequestParts<AppState> for MaybeToken {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(header) = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        else {
            return Ok(Self(None));
        };
        let hash = crate::crypto::hash_token(header);
        // `resolve_token` is the single predicate (D4's expiry arm lives there),
        // and the row id comes back with it so this path can also record the
        // use. Both are properties of the token, not of the caller.
        let resolved = match darkhold_db::external::resolve_token(state.db(), &hash).await {
            Ok(resolved) => resolved,
            // A database error here is not "no token". Returning `None` would
            // downgrade a fault to an anonymous request, which is how a broken
            // database turns into mysterious 401s instead of a visible error.
            Err(err) => {
                tracing::error!(error = %err, "resolving a bearer token failed");
                return Ok(Self(None));
            }
        };
        let Some(identity) = resolved else {
            return Ok(Self(None));
        };
        // D6: an unrecognised scope refuses the request rather than being
        // dropped. The previous `.filter_map(..ok())` silently *narrowed* a
        // token — a row holding `content.read` and a scope this build does not
        // know resolved as if it held only the former — so an operator
        // revoking a scope by renaming it, or a row written by a newer Darkhold
        // and read by an older one, quietly produced a token that was not what
        // it said. §0.4's rule is that an unrecognised value stops startup, not
        // that it disappears. Narrowing was at least safe; it was still a lie
        // about the token's authority, and §23.1's whole contract is that the
        // scope set is what the client was granted.
        let scopes = match darkhold_domain::api_scopes::parse_all(&identity.scopes) {
            Ok(scopes) => scopes,
            Err(unknown) => {
                tracing::warn!(
                    scopes = %unknown,
                    "a token carries a scope this build does not recognise; refusing it"
                );
                return Ok(Self(None));
            }
        };
        // A bot's token stops working when the bot is suspended. `bot_for_token` returns `None` for a token that is not a
        // bot's, which is the ordinary case and not a refusal — only a row whose state is not `active` refuses.
        match darkhold_db::external::bot_for_token(state.db(), &identity.token_id).await {
            Ok(Some((bot_id, bot_state))) if bot_state != "active" => {
                tracing::warn!(%bot_id, %bot_state, "a suspended bot presented its token; refusing");
                return Ok(Self(None));
            }
            Ok(_) => {}
            Err(err) => {
                // A lookup that failed is not a suspension: refusing every bot call because the database hiccuped would turn
                // a bookkeeping fault into an outage. The token's own checks already passed.
                tracing::error!(error = %err, "resolving a token's bot registration failed");
            }
        }
        let account_id = match AccountId::from_str(&identity.account_id) {
            Ok(account_id) => account_id,
            Err(_) => return Ok(Self(None)),
        };
        // D3: the column existed since migration 0001 and was read by
        // `list_tokens` while nothing wrote it, so a token's age was
        // unobservable. Recorded here, on the one path where a token has just
        // proved it works. A failure is logged rather than propagated: refusing
        // a valid call because the audit write failed would turn a bookkeeping
        // problem into an outage, which is the wrong trade for a column that
        // only reports.
        if let Err(err) = darkhold_db::external::touch_token(state.db(), &identity.token_id).await {
            tracing::error!(error = %err, "recording a token's last use failed");
        }
        Ok(Self(Some(TokenUser {
            account_id,
            scopes,
            token_id: identity.token_id,
            acting_pseud_id: identity.acting_pseud_id,
        })))
    }
}

/// Reject requests without a valid token holding the required scope.
pub struct RequireToken {
    pub token: TokenUser,
}

impl axum::extract::FromRequestParts<AppState> for RequireToken {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let maybe = MaybeToken::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError(AppError::AuthRequired))?;
        match maybe.0 {
            Some(token) => Ok(Self { token }),
            None => Err(ApiError(AppError::AuthRequired)),
        }
    }
}

/// Read a cookie value from a header map.
///
/// Tolerates the usual malformations (extra spaces, a `=` inside a value) and
/// returns `None` rather than panicking on them.
#[must_use]
pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key.trim() == name {
            return Some(value.trim().to_owned());
        }
    }
    None
}

/// How a cookie should be scoped.
#[derive(Debug, Clone, Copy)]
pub struct CookiePolicy {
    /// Whether to add the `Secure` attribute.
    pub secure: bool,
}

impl CookiePolicy {
    /// Derive the policy from configuration.
    #[must_use]
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            secure: config.security.cookie_secure,
        }
    }
}

/// Render a `Set-Cookie` value.
///
/// `http_only` is the only difference between the two cookies this application
/// sets, and it is a parameter rather than two functions so that the two can
/// never drift on `Secure` or `SameSite`.
#[must_use]
pub fn build_cookie(
    name: &str,
    value: &str,
    policy: CookiePolicy,
    max_age: Option<std::time::Duration>,
    http_only: bool,
) -> String {
    let mut cookie = format!("{name}={value}; Path=/; SameSite=Lax");
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if policy.secure {
        cookie.push_str("; Secure");
    }
    match max_age {
        Some(duration) => cookie.push_str(&format!("; Max-Age={}", duration.as_secs())),
        // Without Max-Age the cookie is a session cookie, which is what a
        // cleared login should leave behind.
        None => cookie.push_str("; Max-Age=0"),
    }
    cookie
}

/// The cookies to set after a successful sign-in, and the CSRF token handed to
/// the client (identical to the one inside the readable cookie).
pub struct IssuedSession {
    /// The opaque session token.
    pub token: String,
    /// The CSRF token.
    pub csrf_token: String,
    /// The session's identifier.
    pub session_id: SessionId,
    /// When it expires.
    pub expires_at: String,
}

/// Create a session for an account and return its credentials.
pub async fn issue_session(
    state: &AppState,
    account: &Account,
    initial_pseud: Option<PseudId>,
    user_agent: Option<&str>,
    acr: Option<&str>,
    roles: &[String],
) -> anyhow::Result<IssuedSession> {
    let token = crypto::generate_token();
    let csrf_token = crypto::generate_token();
    let ttl = state.config().security.session_ttl;
    let expires_at = sessions::expires_in_hours(i64::try_from(ttl.as_secs() / 3600).unwrap_or(24));

    let session_id = sessions::create_session(
        state.db(),
        account.id,
        &crypto::hash_token(&token),
        &crypto::hash_token(&csrf_token),
        sessions::SessionContext {
            user_agent,
            active_pseud_id: initial_pseud,
            expires_at: &expires_at,
            acr,
            roles,
        },
    )
    .await?;

    Ok(IssuedSession {
        token,
        csrf_token,
        session_id,
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_cookie(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(value).expect("header"),
        );
        headers
    }

    #[test]
    fn cookies_are_read_by_name() {
        let headers = headers_with_cookie("a=1; darkhold_session=abc.def; b=2");
        assert_eq!(
            read_cookie(&headers, SESSION_COOKIE).as_deref(),
            Some("abc.def")
        );
        assert_eq!(read_cookie(&headers, "b").as_deref(), Some("2"));
        assert_eq!(read_cookie(&headers, "missing"), None);
    }

    #[test]
    fn cookie_parsing_survives_malformations() {
        // No cookie header at all.
        assert_eq!(read_cookie(&HeaderMap::new(), SESSION_COOKIE), None);
        // Empty header.
        assert_eq!(read_cookie(&headers_with_cookie(""), SESSION_COOKIE), None);
        // Bare token with no `=`.
        assert_eq!(
            read_cookie(&headers_with_cookie("junk"), SESSION_COOKIE),
            None
        );
        // Extra whitespace around the pair.
        assert_eq!(
            read_cookie(
                &headers_with_cookie("  darkhold_session = xyz  "),
                SESSION_COOKIE
            )
            .as_deref(),
            Some("xyz")
        );
        // A value containing `=` keeps the whole value after the first split.
        assert_eq!(
            read_cookie(&headers_with_cookie("darkhold_session=a=b"), SESSION_COOKIE).as_deref(),
            Some("a=b")
        );
    }

    #[test]
    fn the_session_cookie_is_http_only_and_the_csrf_cookie_is_not() {
        let policy = CookiePolicy { secure: true };

        let session = build_cookie(
            SESSION_COOKIE,
            "token",
            policy,
            Some(std::time::Duration::from_secs(3600)),
            true,
        );
        assert!(session.contains("HttpOnly"), "{session}");
        assert!(session.contains("Secure"), "{session}");
        assert!(session.contains("Path=/"), "{session}");
        assert!(session.contains("SameSite=Lax"), "{session}");
        assert!(session.contains("Max-Age=3600"), "{session}");

        let csrf = build_cookie(
            CSRF_COOKIE,
            "csrf",
            policy,
            Some(std::time::Duration::from_secs(3600)),
            false,
        );
        assert!(
            !csrf.contains("HttpOnly"),
            "the CSRF cookie must be readable by scripts: {csrf}"
        );
    }

    #[test]
    fn clearing_a_cookie_sets_a_zero_max_age() {
        // A logout must actively expire the cookie, not merely stop sending it.
        let cleared = build_cookie(
            SESSION_COOKIE,
            "",
            CookiePolicy { secure: false },
            None,
            true,
        );
        assert!(cleared.contains("Max-Age=0"), "{cleared}");
    }

    #[test]
    fn development_cookies_are_not_secure_but_production_ones_are() {
        let dev = build_cookie(
            SESSION_COOKIE,
            "t",
            CookiePolicy { secure: false },
            None,
            true,
        );
        assert!(!dev.contains("Secure"));

        let prod = build_cookie(
            SESSION_COOKIE,
            "t",
            CookiePolicy { secure: true },
            None,
            true,
        );
        assert!(prod.contains("Secure"));
    }

    #[test]
    fn only_safe_methods_skip_the_csrf_check() {
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            assert!(!is_state_changing(&method));
        }
        for method in [Method::POST, Method::PATCH, Method::PUT, Method::DELETE] {
            assert!(is_state_changing(&method), "{} must be checked", method);
        }
    }
}

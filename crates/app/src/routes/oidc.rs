//! Sign in with Polaris (W2).
//!
//! ```text
//! GET /api/v1/auth/oidc/start      → 303 to polaris-id /authorize (state + PKCE + nonce)
//! GET /api/v1/auth/oidc/callback   → 303 back into the site, with a local session
//! ```
//!
//! Three rules, each of which is the reason this is server-side rather than a script on the login page:
//!
//! * **The verifier never reaches the browser.** State, PKCE verifier and nonce are generated here and
//!   parked in `oidc_pending` for ten minutes; the callback redeems them with `DELETE … RETURNING`, so a
//!   replayed callback finds nothing instead of minting a second session.
//! * **Links are by verified email, never by handle.** Handles are per-pseud on this site and the estate is
//!   deliberately not a single namespace; an email the provider says it verified is evidence, a handle is
//!   a coincidence.
//! * **An unknown email is not an account.** It bounces to `/login?oidc=no_account` rather than creating
//!   one, because account creation here is a product decision (ages, pseuds, consent) and not something a
//!   redirect handler should decide.
//!
//! Configuration is three environment variables, read per request so a restart is not needed to see a
//! corrected one: `POLARIS_ID_ISSUER`, `POLARIS_ID_CLIENT_ID`, `POLARIS_ID_REDIRECT_URI`.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use base64::Engine as _;
use darkhold_db::identity;
use darkhold_db::oidc;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::auth::{self, CookiePolicy, CSRF_COOKIE, SESSION_COOKIE};
use crate::state::AppState;

const PROVIDER: &str = "polaris-id";
const SCOPE: &str = "profile:read email";
const PENDING_TTL_SECONDS: i64 = 600;

#[derive(Debug, Clone, Deserialize)]
struct ProviderConfig {
    issuer: String,
    client_id: String,
    redirect_uri: String,
}

impl ProviderConfig {
    fn from_env() -> Option<Self> {
        let issuer = std::env::var("POLARIS_ID_ISSUER").ok()?;
        let client_id = std::env::var("POLARIS_ID_CLIENT_ID").ok()?;
        let redirect_uri = std::env::var("POLARIS_ID_REDIRECT_URI").ok()?;
        if issuer.is_empty() || client_id.is_empty() || redirect_uri.is_empty() {
            return None;
        }
        Some(Self {
            issuer: issuer.trim_end_matches('/').to_string(),
            client_id,
            redirect_uri,
        })
    }
}

/// Routes. Mounted inside the auth module, so module gating and the auth rate-limit class apply.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/oidc/start", get(start))
        .route("/auth/oidc/callback", get(callback))
}

fn unavailable() -> Response {
    (
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({
            "error": {"code": "unavailable", "message": "Polaris sign-in is not configured on this site"}
        })),
    )
        .into_response()
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Only a same-site path is accepted as the post-login destination.
fn safe_next(raw: Option<&String>) -> String {
    match raw {
        Some(value) if value.starts_with('/') && !value.starts_with("//") => value.clone(),
        _ => "/".to_string(),
    }
}

/// `GET /api/v1/auth/oidc/start?next=/works/12&prompt=none`
async fn start(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let Some(config) = ProviderConfig::from_env() else {
        return unavailable();
    };

    let attempt_state = crate::crypto::generate_token();
    let verifier = crate::crypto::generate_token();
    let nonce = crate::crypto::generate_token();
    let next = safe_next(params.get("next"));
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));

    let now = identity::now_rfc3339();
    let expires = identity::format_rfc3339(
        time::OffsetDateTime::now_utc() + time::Duration::seconds(PENDING_TTL_SECONDS),
    );

    if let Err(error) = oidc::create_pending(
        state.db(),
        &attempt_state,
        &verifier,
        &nonce,
        &next,
        &expires,
    )
    .await
    {
        tracing::error!(%error, "could not park the OIDC attempt");
        return unavailable();
    }

    let mut url = format!(
        "{}/authorize?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        config.issuer,
        urlencode(&config.client_id),
        urlencode(&config.redirect_uri),
        urlencode(SCOPE),
        urlencode(&attempt_state),
        urlencode(&nonce),
        urlencode(&challenge),
    );
    // `prompt=none` is what makes silent re-auth safe: if the provider has no session it answers
    // login_required instead of showing a form, and the caller treats that as "anonymous".
    if params.get("prompt").map(String::as_str) == Some("none") {
        url.push_str("&prompt=none");
    }
    tracing::info!(next = %next, silent = params.contains_key("prompt"), "starting Polaris sign-in");
    let _ = now;
    Redirect::to(&url).into_response()
}

#[derive(Debug, Deserialize)]
struct TokenReply {
    access_token: String,
    #[serde(default)]
    id_token: String,
}

/// `GET /api/v1/auth/oidc/callback?code=…&state=…`
async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let Some(config) = ProviderConfig::from_env() else {
        return unavailable();
    };

    if let Some(error) = params.get("error") {
        tracing::info!(%error, "the provider refused the sign-in");
        return Redirect::to(&format!("/login?oidc={}", urlencode(error))).into_response();
    }
    let (Some(code), Some(attempt)) = (params.get("code"), params.get("state")) else {
        return Redirect::to("/login?oidc=invalid_response").into_response();
    };

    let now = identity::now_rfc3339();
    let pending = match oidc::take_pending(state.db(), attempt, &now).await {
        Ok(Some(pending)) => pending,
        Ok(None) => {
            tracing::warn!("OIDC callback with an unknown, expired or already-used state");
            return Redirect::to("/login?oidc=expired").into_response();
        }
        Err(error) => {
            tracing::error!(%error, "could not redeem the OIDC attempt");
            return Redirect::to("/login?oidc=error").into_response();
        }
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(_) => return Redirect::to("/login?oidc=error").into_response(),
    };

    let token_reply = client
        .post(format!("{}/token", config.issuer))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("client_id", config.client_id.as_str()),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("code_verifier", pending.verifier.as_str()),
        ])
        .send()
        .await;
    let tokens: TokenReply = match token_reply {
        Ok(response) if response.status().is_success() => match response.json().await {
            Ok(tokens) => tokens,
            Err(error) => {
                tracing::error!(%error, "token response was not JSON");
                return Redirect::to("/login?oidc=error").into_response();
            }
        },
        Ok(response) => {
            tracing::warn!(status = %response.status(), "token exchange refused");
            return Redirect::to("/login?oidc=error").into_response();
        }
        Err(error) => {
            tracing::error!(%error, "token exchange failed");
            return Redirect::to("/login?oidc=unreachable").into_response();
        }
    };

    // The profile comes from /userinfo over TLS rather than from the id_token's payload: one authority
    // decides who the user is, and no product has to grow a JWT verifier to find out.
    let profile: serde_json::Value = match client
        .get(format!("{}/userinfo", config.issuer))
        .bearer_auth(&tokens.access_token)
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => match response.json().await {
            Ok(value) => value,
            Err(_) => return Redirect::to("/login?oidc=error").into_response(),
        },
        Ok(response) => {
            tracing::warn!(status = %response.status(), "userinfo refused");
            return Redirect::to("/login?oidc=error").into_response();
        }
        Err(error) => {
            tracing::error!(%error, "userinfo failed");
            return Redirect::to("/login?oidc=unreachable").into_response();
        }
    };

    let subject = profile
        .get("sub")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let handle = profile.get("handle").and_then(|v| v.as_str());
    let email = profile.get("email").and_then(|v| v.as_str()).filter(|_| {
        profile
            .get("email_verified")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    });
    // Kept beside the link so a page render never has to call the provider: see migration 0148.
    let picture = profile.get("picture").and_then(|v| v.as_str());
    let display_name = profile.get("name").and_then(|v| v.as_str());
    if subject.is_empty() {
        tracing::warn!("userinfo returned no subject");
        return Redirect::to("/login?oidc=invalid_response").into_response();
    }
    let _ = tokens.id_token;

    // 1. An established link wins — it is the only mapping that survives an email change.
    let mut account = match oidc::find_link(state.db(), PROVIDER, subject).await {
        Ok(Some(id)) => {
            // An established link still refreshes the cached profile: a rename at the provider reaches this
            // site on the next sign-in rather than never.
            if let Ok(Some(found)) = identity::find_account(state.db(), id).await {
                let _ = oidc::link(
                    state.db(),
                    PROVIDER,
                    subject,
                    found.id,
                    email,
                    picture,
                    display_name,
                    &now,
                )
                .await;
                Some(found)
            } else {
                None
            }
        }
        _ => None,
    };
    let mut linked_now = false;

    // 2. Otherwise the verified address the provider vouches for.
    if account.is_none() {
        if let Some(email) = email {
            if let Ok(Some(found)) = identity::find_account_by_email(state.db(), email).await {
                if oidc::link(
                    state.db(),
                    PROVIDER,
                    subject,
                    found.id,
                    Some(email),
                    picture,
                    display_name,
                    &now,
                )
                .await
                .is_ok()
                {
                    linked_now = true;
                }
                account = Some(found);
            }
        }
    }

    let Some(account) = account else {
        tracing::info!(
            handle = handle.unwrap_or(""),
            "Polaris sign-in for an account this site does not have"
        );
        return Redirect::to(&format!(
            "/login?oidc=no_account&handle={}",
            urlencode(handle.unwrap_or(""))
        ))
        .into_response();
    };
    if !matches!(account.status, darkhold_db::identity::AccountStatus::Active) {
        tracing::info!(account = %account.id, "Polaris sign-in to a non-active account");
        return Redirect::to("/login?oidc=inactive").into_response();
    }

    let pseuds = identity::pseuds_for_account(state.db(), account.id)
        .await
        .unwrap_or_default();
    let initial = pseuds.first().map(|pseud| pseud.id);
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok());

    let issued = match auth::issue_session(&state, &account, initial, user_agent).await {
        Ok(issued) => issued,
        Err(error) => {
            tracing::error!(%error, "could not issue a session after Polaris sign-in");
            return Redirect::to("/login?oidc=error").into_response();
        }
    };
    if let Err(error) =
        darkhold_db::engagement::record_login(state.db(), &account.id.to_string()).await
    {
        tracing::warn!(account = %account.id, %error, "failed to record login streak");
    }
    tracing::info!(
        account = %account.id,
        subject = %subject,
        linked_now,
        "signed in with Polaris"
    );

    let policy = CookiePolicy::from_config(state.config());
    let ttl = state.config().security.session_ttl;
    let mut response = Redirect::to(&pending.next).into_response();
    for cookie in [
        auth::build_cookie(SESSION_COOKIE, &issued.token, policy, Some(ttl), true),
        auth::build_cookie(CSRF_COOKIE, &issued.csrf_token, policy, Some(ttl), false),
    ] {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

fn urlencode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

//! OIDC link and in-flight login state (W2, one account for the estate).
//!
//! Two shapes, on purpose:
//!
//! * [`find_link`] / [`link`] — the durable fact that a Polaris subject is a local account. Linking is by
//!   **verified email**, never by handle alone: handles are per-pseud and a second site may hold the same
//!   one for somebody else.
//! * [`create_pending`] / [`take_pending`] — one in-flight authorization, carrying the PKCE verifier and
//!   nonce between the redirect out and the redirect back. It is deleted when redeemed, so a replayed
//!   callback finds nothing rather than minting a second session.

use anyhow::{Context, Result};
use darkhold_domain::AccountId;

use crate::Database;

/// What was stashed between `/authorize` and the callback.
#[derive(Debug, Clone)]
pub struct PendingLogin {
    /// PKCE `code_verifier`, generated for this attempt only.
    pub verifier: String,
    /// OIDC nonce, echoed back inside the id_token.
    pub nonce: String,
    /// Where to send the browser once the session exists.
    pub next: String,
}

/// Remember one attempt.
pub async fn create_pending(
    db: &Database,
    state: &str,
    verifier: &str,
    nonce: &str,
    next: &str,
    expires_at: &str,
) -> Result<()> {
    let sql = db.sql(
        "INSERT INTO oidc_pending (state, verifier, nonce, next, expires_at) VALUES (?, ?, ?, ?, ?)",
        "INSERT INTO oidc_pending (state, verifier, nonce, next, expires_at) VALUES (?, ?, ?, ?, ?)",
    );
    match db.backend() {
        crate::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(state)
                .bind(verifier)
                .bind(nonce)
                .bind(next)
                .bind(expires_at)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await
                .context("storing the pending OIDC login")?;
        }
        crate::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(state)
                .bind(verifier)
                .bind(nonce)
                .bind(next)
                .bind(expires_at)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await
                .context("storing the pending OIDC login")?;
        }
    }
    Ok(())
}

/// Redeem one attempt: `DELETE … RETURNING`, so two callbacks racing with the same state cannot both win.
pub async fn take_pending(db: &Database, state: &str, now: &str) -> Result<Option<PendingLogin>> {
    let sql = db.sql(
        "DELETE FROM oidc_pending WHERE state = ? AND expires_at > ? RETURNING verifier, nonce, next",
        "DELETE FROM oidc_pending WHERE state = ? AND expires_at > ? RETURNING verifier, nonce, next",
    );
    type Row = (String, String, String);
    let row: Option<Row> = match db.backend() {
        crate::Backend::Sqlite => sqlx::query_as(&sql)
            .bind(state)
            .bind(now)
            .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
            .await
            .context("redeeming the pending OIDC login")?,
        crate::Backend::Postgres => sqlx::query_as(&sql)
            .bind(state)
            .bind(now)
            .fetch_optional(db.postgres_pool().expect("postgres handle"))
            .await
            .context("redeeming the pending OIDC login")?,
    };
    Ok(row.map(|(verifier, nonce, next)| PendingLogin {
        verifier,
        nonce,
        next,
    }))
}

/// The account a subject is already linked to, if any.
pub async fn find_link(db: &Database, provider: &str, subject: &str) -> Result<Option<AccountId>> {
    let sql = db.sql(
        "SELECT account_id FROM oidc_links WHERE provider = ? AND subject = ?",
        "SELECT account_id FROM oidc_links WHERE provider = ? AND subject = ?",
    );
    let row: Option<(String,)> = match db.backend() {
        crate::Backend::Sqlite => sqlx::query_as(&sql)
            .bind(provider)
            .bind(subject)
            .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
            .await
            .context("looking up an OIDC link")?,
        crate::Backend::Postgres => sqlx::query_as(&sql)
            .bind(provider)
            .bind(subject)
            .fetch_optional(db.postgres_pool().expect("postgres handle"))
            .await
            .context("looking up an OIDC link")?,
    };
    Ok(row.and_then(|(id,)| id.parse().ok()))
}

/// Record a link. Idempotent: the same subject seen twice updates the email it was matched by.
pub async fn link(
    db: &Database,
    provider: &str,
    subject: &str,
    account_id: AccountId,
    email: Option<&str>,
    picture: Option<&str>,
    display_name: Option<&str>,
    at: &str,
) -> Result<()> {
    // The name and picture are refreshed on every sign-in: they are a cache of what `/userinfo` said, so a
    // rename at the provider reaches every site on the next visit without a background sync.
    let sql = db.sql(
        "INSERT INTO oidc_links (provider, subject, account_id, email, picture, display_name, linked_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (provider, subject) DO UPDATE SET account_id = excluded.account_id, email = excluded.email,
           picture = excluded.picture, display_name = excluded.display_name",
        "INSERT INTO oidc_links (provider, subject, account_id, email, picture, display_name, linked_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (provider, subject) DO UPDATE SET account_id = excluded.account_id, email = excluded.email,
           picture = excluded.picture, display_name = excluded.display_name",
    );
    let account = account_id.to_string();
    match db.backend() {
        crate::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(provider)
                .bind(subject)
                .bind(&account)
                .bind(email)
                .bind(picture)
                .bind(display_name)
                .bind(at)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await
                .context("recording an OIDC link")?;
        }
        crate::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(provider)
                .bind(subject)
                .bind(&account)
                .bind(email)
                .bind(picture)
                .bind(display_name)
                .bind(at)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await
                .context("recording an OIDC link")?;
        }
    }
    Ok(())
}

/// The profile the provider vouches for, as last seen at sign-in.
///
/// Returns `None` when the account has no Polaris link — a local account keeps its own name and picture.
/// Does this account sign in through the identity provider?
///
/// The row in `oidc_links` is the flag (estate W2 §8): once an account has linked, its local password is
/// retired and the login handler refuses it. Deliberately provider-agnostic — the question is "has this
/// account ever linked to *any* provider", not "which one", because the second provider would otherwise
/// silently re-open the password.
pub async fn signs_in_through_provider(db: &Database, account_id: AccountId) -> Result<bool> {
    let sql = db.sql(
        "SELECT 1 FROM oidc_links WHERE account_id = ? LIMIT 1",
        "SELECT 1 FROM oidc_links WHERE account_id = $1 LIMIT 1",
    );
    let account = account_id.to_string();
    let row: Option<(i32,)> = match db.backend() {
        crate::Backend::Sqlite => sqlx::query_as(&sql)
            .bind(&account)
            .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
            .await
            .context("asking whether an account signs in through the estate")?,
        crate::Backend::Postgres => sqlx::query_as(&sql)
            .bind(&account)
            .fetch_optional(db.postgres_pool().expect("postgres handle"))
            .await
            .context("asking whether an account signs in through the estate")?,
    };
    Ok(row.is_some())
}

pub async fn profile_for_account(
    db: &Database,
    provider: &str,
    account_id: AccountId,
) -> Result<Option<(Option<String>, Option<String>)>> {
    let sql = db.sql(
        "SELECT picture, display_name FROM oidc_links WHERE provider = ? AND account_id = ?",
        "SELECT picture, display_name FROM oidc_links WHERE provider = $1 AND account_id = $2",
    );
    let account = account_id.to_string();
    let row = match db.backend() {
        crate::Backend::Sqlite => sqlx::query_as::<_, (Option<String>, Option<String>)>(&sql)
            .bind(provider)
            .bind(&account)
            .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
            .await
            .context("reading an OIDC profile")?,
        crate::Backend::Postgres => sqlx::query_as::<_, (Option<String>, Option<String>)>(&sql)
            .bind(provider)
            .bind(&account)
            .fetch_optional(db.postgres_pool().expect("postgres handle"))
            .await
            .context("reading an OIDC profile")?,
    };
    Ok(row)
}

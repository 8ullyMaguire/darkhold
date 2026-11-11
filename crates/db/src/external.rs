//! M18 — External repository: tokens, bots, feeds, push, federation, AI.

use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{Backend, Database};
use darkhold_domain::api_scopes::Scope;

// ---------------------------------------------------------------------------
// Tokens (uses existing api_tokens table from migration 0001)
// ---------------------------------------------------------------------------

/// Issue a token with no expiry and no acting pseud.
///
/// Kept because it is what every pre-existing call site means: a personal token
/// the reader revokes by hand. It is *not* what a bot token should be — see
/// [`issue_token_acting`].
pub async fn issue_token(
    db: &Database,
    account: &str,
    kind: &str,
    name: &str,
    token_hash: &str,
    scopes: &[Scope],
) -> Result<String, sqlx::Error> {
    issue_token_acting(db, account, kind, name, token_hash, scopes, None, None).await
}


/// Issue a token bound to a specific acting pseud.
///
/// The full form, and the only INSERT that creates an `api_tokens` row. The two
/// wrappers above delegate here rather than each carrying their own statement,
/// so there is exactly one place where the column list can drift from the schema
/// — which is how `expires_at` came to be read by `list_tokens` while nothing
/// ever wrote it. Three copies of this INSERT would be three chances to be wrong
/// about the schema, and only one of them would be covered by a test.
#[allow(clippy::too_many_arguments)]
pub async fn issue_token_acting(
    db: &Database,
    account: &str,
    kind: &str,
    name: &str,
    token_hash: &str,
    scopes: &[Scope],
    expires_at: Option<&str>,
    acting_pseud_id: Option<&str>,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();
    let scopes_json =
        serde_json::to_string(&scopes.iter().map(|c| c.as_str()).collect::<Vec<_>>()).unwrap();

    let sql = db.sql(
        "INSERT INTO api_tokens (id, account_id, name, kind, token_hash, scopes, expires_at, acting_pseud_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        "INSERT INTO api_tokens (id, account_id, name, kind, token_hash, scopes, expires_at, acting_pseud_id, created_at)
         VALUES (?::uuid, ?::uuid, ?, ?, ?, ?, ?, ?::uuid, ?)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(account)
                .bind(name)
                .bind(kind)
                .bind(token_hash)
                .bind(&scopes_json)
                .bind(expires_at)
                .bind(acting_pseud_id)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(account)
                .bind(name)
                .bind(kind)
                .bind(token_hash)
                .bind(&scopes_json)
                .bind(expires_at)
                .bind(acting_pseud_id)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    };
    Ok(id)
}

/// The identity a token resolves to, as `api_tokens` stores it.
///
/// `acting_pseud_id` is spec §23.1's "explicit acting pseud": a token belongs to
/// an account but *acts as* a pseud, and the two are different identities that
/// the rest of the schema keeps strictly apart. It is `Option` because a token
/// issued before the column existed has none, and a token with none must not
/// silently inherit the account's default pseud — that is the conflation §23.1
/// exists to prevent. The caller refuses instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenIdentity {
    /// The token's own row id. Selected with the rest, because the caller needs
    /// it to record `last_used_at` against *this* token — re-querying by hash
    /// to find it would be a second round trip to learn something the first one
    /// already had.
    pub token_id: String,
    pub account_id: String,
    pub scopes: Vec<String>,
    pub acting_pseud_id: Option<String>,
}

/// The predicate every token resolution shares, written out per dialect rather
/// than abstracted, because the two engines disagree about the placeholder
/// syntax and this is the one query where a mistake is a security mistake:
///
/// * `revoked_at IS NULL` — an explicitly revoked token.
/// * `expires_at IS NULL OR expires_at > now` — D4. The column has always
///   existed and was never written, so this arm was unreachable until A2 made it
///   reachable; without it an expired token would still authenticate, which is
///   exactly the "machinery present, wiring absent" shape `docs/goal.md` names
///   for `rec.mode`.
///
/// Timestamps are RFC 3339 in TEXT columns on both engines (ADR 0004), so
/// string comparison is chronological comparison. That is a property of the
/// format, not a convenience: the same text sorts the same way in both engines.
const EXPIRY_ARM: &str = " AND (expires_at IS NULL OR expires_at > ";

async fn resolve_token_sqlite(
    pool: &sqlx::SqlitePool,
    token_hash: &str,
) -> Result<Option<TokenIdentity>, sqlx::Error> {
    // **The account's state is part of the token's validity.**
    //
    // A token is a credential *an account holds*, so it stops working when the
    // account stops: suspended, closed, or **folded** (spec §14.2 — the fold
    // leaves a tombstone, and a tombstone must not act).
    //
    // The join is what makes that true. Without it the resolver asked only
    // about the token, so a **suspended account's token still opened every
    // door** — and a folded guest's token would have done the same, which is
    // the one case the fold exists to prevent.
    //
    // The filter is `status = 'active'` rather than `status <> 'folded'`, so a
    // status nobody has thought about is *not* treated as active. **The default
    // has to be the closed one.**
    let sql = format!(
        "SELECT t.id, t.account_id, t.scopes, t.acting_pseud_id FROM api_tokens t
         JOIN accounts a ON a.id = t.account_id
         WHERE t.token_hash = ? AND t.revoked_at IS NULL AND a.status = 'active'{EXPIRY_ARM}?)"
    );
    let row = sqlx::query(&sql)
        .bind(token_hash)
        .bind(crate::identity::now_rfc3339())
        .fetch_optional(pool)
        .await?;
    match row {
        Some(r) => {
            let scopes_json: String = r.get("scopes");
            let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
            Ok(Some(TokenIdentity {
                token_id: r.get::<String, _>("id"),
                account_id: r.get::<String, _>("account_id"),
                scopes,
                acting_pseud_id: r.get::<Option<String>, _>("acting_pseud_id"),
            }))
        }
        None => Ok(None),
    }
}

async fn resolve_token_postgres(
    pool: &sqlx::postgres::PgPool,
    token_hash: &str,
) -> Result<Option<TokenIdentity>, sqlx::Error> {
    // The same join, and the same reason: a token stops working when the
    // account that holds it stops. See the SQLite arm above.
    let sql = format!(
        "SELECT t.id::text, t.account_id::text, t.scopes, t.acting_pseud_id::text
         FROM api_tokens t JOIN accounts a ON a.id = t.account_id
         WHERE t.token_hash = $1 AND t.revoked_at IS NULL AND a.status = 'active'{EXPIRY_ARM}$2)"
    );
    let row = sqlx::query(&sql)
        .bind(token_hash)
        .bind(crate::identity::now_rfc3339())
        .fetch_optional(pool)
        .await?;
    match row {
        Some(r) => {
            let scopes_json: String = r.get("scopes");
            let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
            Ok(Some(TokenIdentity {
                token_id: r.get::<String, _>("id"),
                account_id: r.get::<String, _>("account_id"),
                scopes,
                acting_pseud_id: r.get::<Option<String>, _>("acting_pseud_id"),
            }))
        }
        None => Ok(None),
    }
}

pub async fn resolve_token(
    db: &Database,
    token_hash: &str,
) -> Result<Option<TokenIdentity>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            resolve_token_sqlite(db.sqlite_pool().expect("sqlite"), token_hash).await
        }
        Backend::Postgres => {
            resolve_token_postgres(db.postgres_pool().expect("postgres"), token_hash).await
        }
    }
}

/// Record that a token was just used.
///
/// `last_used_at` was read by `list_tokens` and written by nothing, so the token
/// list could not distinguish a credential in daily use from one abandoned at
/// issue — which is the question an operator asks when deciding what to revoke.
///
/// Called on the path that resolves the token, so the write happens exactly where
/// the token proved it works and nowhere else. A failure here is deliberately
/// propagated rather than swallowed: silently losing the audit trail is how the
/// column became dead in the first place.
pub async fn touch_token(db: &Database, token_id: &str) -> Result<(), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ?")
                .bind(&now)
                .bind(token_id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query("UPDATE api_tokens SET last_used_at = $1 WHERE id = $2::uuid")
                .bind(&now)
                .bind(token_id)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    };
    Ok(())
}

/// Revoke a token **belonging to `account`**, and report whether one was.
///
/// The account predicate is in the SQL rather than in the handler, and the rows
/// affected are returned, because the version this replaces was
/// `UPDATE api_tokens SET revoked_at = ? WHERE id = ?` reached from a handler
/// that extracted the session and discarded it. That made the endpoint an
/// unauthenticated, cross-account denial of service: anyone could revoke
/// anyone's token, and — since `expires_at` was never written either — nothing
/// ever stopped doing so again.
///
/// A caller must not be able to forget the predicate, so it is a parameter of
/// The link credentials one pseud currently holds for one bot.
///
/// A guild bot serves many readers, so "the bot's token" was never one row: each link is a credential labelled
/// `kind = 'bot'`, `name = <bot id>`, acting as the pseud that granted it. Re-linking the *same* person replaces only
/// theirs — revoking the bot's every credential when one person re-links would log the rest of the server out.
pub async fn bot_link_token_ids(
    db: &Database,
    bot_id: &str,
    pseud_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let sql = db.sql(
        "SELECT id FROM api_tokens
          WHERE kind = 'bot' AND name = ?1 AND acting_pseud_id = ?2 AND revoked_at IS NULL",
        "SELECT id::text FROM api_tokens
          WHERE kind = 'bot' AND name = $1::text AND acting_pseud_id = $2::text AND revoked_at IS NULL",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(bot_id)
                .bind(pseud_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(bot_id)
                .bind(pseud_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await
        }
    }
}

/// the query rather than a check a future caller may omit. The `bool` is what
/// lets the handler answer 404 for someone else's token without a second
/// existence query, which would itself be a disclosure.
pub async fn revoke_token_for_account(
    db: &Database,
    token_id: &str,
    account: &str,
) -> Result<bool, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "UPDATE api_tokens SET revoked_at = ? WHERE id = ? AND account_id = ?",
        "UPDATE api_tokens SET revoked_at = ? WHERE id = ?::uuid AND account_id = ?::uuid",
    );
    match db.backend() {
        Backend::Sqlite => {
            let result = sqlx::query(&sql)
                .bind(&now)
                .bind(token_id)
                .bind(account)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(result.rows_affected() > 0)
        }
        Backend::Postgres => {
            let result = sqlx::query(&sql)
                .bind(&now)
                .bind(token_id)
                .bind(account)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(result.rows_affected() > 0)
        }
    }
}

// ---------------------------------------------------------------------------
// Token listing
// ---------------------------------------------------------------------------

/// The caller's live tokens, newest first.
///
/// `scopes` is stored as a JSON string and returned as one. That is pre-existing
/// and deliberate — it is what `list_tokens` has always done, and a reader's
/// settings page is not the place to discover a second scope encoding — so the
/// new `kind` and `acting_pseud_id` are returned the same way the surrounding
/// fields are rather than introducing a mixed convention in one response.
pub async fn list_tokens(db: &Database, account: &str) -> Result<Vec<Value>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query("SELECT id, account_id, name, kind, scopes, acting_pseud_id, created_at, last_used_at, expires_at, revoked_at FROM api_tokens WHERE account_id = ? AND revoked_at IS NULL ORDER BY created_at DESC")
                .bind(account)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "account_id": r.get::<String, _>("account_id"),
                        "name": r.get::<String, _>("name"),
                        "kind": r.get::<String, _>("kind"),
                        "scopes": r.get::<String, _>("scopes"),
                        "acting_pseud_id": r.get::<Option<String>, _>("acting_pseud_id"),
                        "created_at": r.get::<String, _>("created_at"),
                        "last_used_at": r.get::<Option<String>, _>("last_used_at"),
                        "expires_at": r.get::<Option<String>, _>("expires_at"),
                        "revoked_at": r.get::<Option<String>, _>("revoked_at"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            // Every uuid column is cast to text. `account_id` was left bare,
            // and decoding a `UUID` column into a Rust `String` is a runtime
            // type error on PostgreSQL (`mismatched types; Rust type String
            // (as SQL type TEXT) is not compatible with SQL type UUID`) while
            // the same query is fine on SQLite, where the column is TEXT. So
            // `GET /me/tokens` returned a 500 on every PostgreSQL instance and
            // worked on every SQLite one — and the SQLite suite could not have
            // told me, which is the whole argument for running both arms.
            let rows = sqlx::query("SELECT id::text, account_id::text, name, kind, scopes, acting_pseud_id::text, created_at, last_used_at, expires_at, revoked_at FROM api_tokens WHERE account_id = $1::uuid AND revoked_at IS NULL ORDER BY created_at DESC")
                .bind(account)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "account_id": r.get::<String, _>("account_id"),
                        "name": r.get::<String, _>("name"),
                        "kind": r.get::<String, _>("kind"),
                        "scopes": r.get::<String, _>("scopes"),
                        "acting_pseud_id": r.get::<Option<String>, _>("acting_pseud_id"),
                        "created_at": r.get::<String, _>("created_at"),
                        "last_used_at": r.get::<Option<String>, _>("last_used_at"),
                        "expires_at": r.get::<Option<String>, _>("expires_at"),
                        "revoked_at": r.get::<Option<String>, _>("revoked_at"),
                    })
                })
                .collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Bots
// ---------------------------------------------------------------------------

pub async fn register_bot(
    db: &Database,
    token_id: &str,
    owner: &str,
    contact: &str,
    user_agent: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO bot_registrations (id, token_id, owner, contact, user_agent, state, registered_at)
                 VALUES (?, ?, ?, ?, ?, 'active', ?)"
            )
            .bind(&id).bind(token_id).bind(owner).bind(contact).bind(user_agent).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO bot_registrations (id, token_id, owner, contact, user_agent, state, registered_at)
                 VALUES ($1, $2, $3, $4, $5, 'active', $6)"
            )
            .bind(&id).bind(token_id).bind(owner).bind(contact).bind(user_agent).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Bot link flow (spec §23.2) — migration 0083's `link_challenges`
// ---------------------------------------------------------------------------

/// A link challenge, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkChallenge {
    pub code: String,
    pub state: String,
    pub bot_id: String,
    pub requested_scopes: Vec<String>,
    pub pseud_id: Option<String>,
    pub token_id: Option<String>,
    pub created_at: String,
    pub expires_at: String,
}

/// The default lifetime of a link challenge, in seconds.
///
/// Short on purpose. A challenge is a public nonce that travels through a chat
/// client the operator does not control — someone pastes `/link abc123` into a
/// channel, it sits in someone's scrollback, and it is readable by every member
/// of that channel for as long as the channel retains history. Ten minutes is
/// long enough for a reader to open Darkhold, sign in, read what is being
/// asked for, and decide; it is not long enough for a challenge to be a
/// standing capability handed to whoever finds it.
pub const LINK_CHALLENGE_TTL_SECONDS: i64 = 600;

/// A high-entropy challenge code.
///
/// 32 bytes from the OS, hex-encoded. Hex rather than a uuid v4 for two
/// reasons, both of which are about this value being a *secret* rather than an
/// identifier: a uuid is 122 bits in a format an attacker can enumerate, and
/// `hex` is already a dependency of this crate so the choice costs nothing.
/// Every other id in this file is a `Uuid::new_v4()` and correctly so — those
/// name rows, and this one is the single value standing between a leaked chat
/// message and a token.
fn challenge_code() -> String {
    let mut bytes = [0u8; 32];
    // `thread_rng` rather than a static `OsRng`: the house pattern in
    // `crates/app/src/secrets.rs` and `crypto.rs`, and it is seeded from the OS.
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    hex::encode(bytes)
}

/// Create a pending link challenge.
///
/// The scopes recorded here are what the *bot* asked for, and they are not what
/// gets granted. The reader chooses at confirmation, and `confirm_link_challenge`
/// re-checks the grant against this list. Storing the request is what lets the
/// confirmation page display "Archivist Bot is asking for: read your library,
/// send kudos" rather than an opaque set of identifiers — the disclosure
/// §23.2's security note requires a bot to make before it is granted anything.
pub async fn create_link_challenge(
    db: &Database,
    bot_id: &str,
    requested_scopes: &[String],
    ttl_seconds: i64,
) -> Result<String, sqlx::Error> {
    let code = challenge_code();
    let now = crate::identity::now_rfc3339();
    let expires_at = crate::identity::in_seconds(ttl_seconds);
    let scopes_json = serde_json::to_string(requested_scopes).unwrap_or_else(|_| "[]".to_owned());

    // The PostgreSQL arm needs its own placeholders. It had `?` for all of this function's life, which is a 500 on every
    // insert on the deployed engine and invisible on SQLite — the class of defect that only a two-engine test finds.
    let sql = db.sql(
        "INSERT INTO link_challenges (code, state, bot_id, requested_scopes, created_at, expires_at)
         VALUES (?1, 'pending', ?2, ?3, ?4, ?5)",
        "INSERT INTO link_challenges (code, state, bot_id, requested_scopes, created_at, expires_at)
         VALUES ($1, 'pending', $2, $3, $4, $5)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&code)
                .bind(bot_id)
                .bind(&scopes_json)
                .bind(&now)
                .bind(&expires_at)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&code)
                .bind(bot_id)
                .bind(&scopes_json)
                .bind(&now)
                .bind(&expires_at)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(code)
}

fn parse_challenge(row: &sqlx::sqlite::SqliteRow) -> LinkChallenge {
    let scopes_json: String = row.get("requested_scopes");
    LinkChallenge {
        code: row.get("code"),
        state: row.get("state"),
        bot_id: row.get("bot_id"),
        requested_scopes: serde_json::from_str(&scopes_json).unwrap_or_default(),
        pseud_id: row.get::<Option<String>, _>("pseud_id"),
        token_id: row.get::<Option<String>, _>("token_id"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
    }
}

/// Read a challenge by code, whatever state it is in.
///
/// Deliberately does **not** filter on state or expiry. The caller needs to tell
/// "this challenge does not exist" apart from "this challenge was already used"
/// apart from "this challenge expired", because those are three different
/// messages a bot shows its user and one wrong answer would be a silent failure
/// the reader cannot act on. The predicates belong to the transitions, and
/// `confirm_link_challenge` / `redeem_link_challenge` are the only writers.
pub async fn find_link_challenge(
    db: &Database,
    code: &str,
) -> Result<Option<LinkChallenge>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(
                "SELECT code, state, bot_id, requested_scopes, pseud_id, token_id, created_at, expires_at
                 FROM link_challenges WHERE code = ?",
            )
            .bind(code)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.as_ref().map(parse_challenge))
        }
        Backend::Postgres => {
            let row = sqlx::query(
                "SELECT code, state, bot_id, requested_scopes, pseud_id::text, token_id::text, created_at, expires_at
                 FROM link_challenges WHERE code = $1",
            )
            .bind(code)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.as_ref().map(parse_challenge_pg))
        }
    }
}

fn parse_challenge_pg(row: &sqlx::postgres::PgRow) -> LinkChallenge {
    let scopes_json: String = row.get("requested_scopes");
    LinkChallenge {
        code: row.get("code"),
        state: row.get("state"),
        bot_id: row.get("bot_id"),
        requested_scopes: serde_json::from_str(&scopes_json).unwrap_or_default(),
        pseud_id: row.get::<Option<String>, _>("pseud_id"),
        token_id: row.get::<Option<String>, _>("token_id"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
    }
}

/// Confirm a challenge on behalf of a reader: bind it to a pseud and a token.
///
/// One statement, and the caller checks rows affected. That is the whole point:
/// the predicate `state = 'pending' AND expires_at > now AND pseud_id IS NULL`
/// is what makes "a challenge is confirmed at most once" true under concurrency.
/// A `SELECT` then `UPDATE` pair would let two confirmations race, and a reader
/// clicking twice — or a reader and a stale tab — would mint two tokens from one
/// challenge, which is the failure §23.2's "revocable limited authorization" is
/// supposed to make harmless but the reader would have to revoke by hand.
///
/// The `pseud_id IS NULL` arm is what stops a *second* reader from rebinding an
/// already-bound challenge: the first confirmation fills it, so a second
/// confirmation of the same code matches nothing. A challenge belongs to whoever
/// confirmed it, not to whoever holds the code.
#[allow(clippy::too_many_arguments)]
pub async fn confirm_link_challenge(
    db: &Database,
    code: &str,
    pseud_id: &str,
    token_id: &str,
) -> Result<bool, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "UPDATE link_challenges SET pseud_id = ?, token_id = ?, used_at = ?, state = 'used'
         WHERE code = ? AND state = 'pending' AND pseud_id IS NULL AND expires_at > ?",
        "UPDATE link_challenges SET pseud_id = ?, token_id = ?::uuid, used_at = ?, state = 'used'
         WHERE code = ? AND state = 'pending' AND pseud_id IS NULL AND expires_at > ?",
    );
    match db.backend() {
        Backend::Sqlite => {
            let result = sqlx::query(&sql)
                .bind(pseud_id)
                .bind(token_id)
                .bind(&now)
                .bind(code)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(result.rows_affected() > 0)
        }
        Backend::Postgres => {
            let result = sqlx::query(&sql)
                .bind(pseud_id)
                .bind(token_id)
                .bind(&now)
                .bind(code)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(result.rows_affected() > 0)
        }
    }
}

/// Mark every stale pending challenge expired.
///
/// The sweeper that keeps `state` honest. Without it a `pending` row whose
/// `expires_at` has passed would still read `pending`, and a state column that
/// can say a thing that is not true is the `claims` defect from migration 0080 —
/// the same shape as the expired claim this repository fixed in e864d43, where a
/// row stayed in the state its own expiry matched on.
pub async fn expire_stale_challenges(db: &Database) -> Result<u64, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "UPDATE link_challenges SET state = 'expired' WHERE state = 'pending' AND expires_at <= ?",
        "UPDATE link_challenges SET state = 'expired' WHERE state = 'pending' AND expires_at <= ?",
    );
    match db.backend() {
        Backend::Sqlite => {
            let result = sqlx::query(&sql)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(result.rows_affected())
        }
        Backend::Postgres => {
            let result = sqlx::query(&sql)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(result.rows_affected())
        }
    }
}

/// A bot's registration, by the token it authenticated with.
///
/// The bot is identified by its registration rather than by its token string, so
/// a handler can check the `state` column (D5's second half — the column is
/// written once at insert and never again, so a suspended bot stays active
/// until something writes it). Returns `None` for a token that is not a bot's,
/// which is a refusal rather than an error: a personal token presenting itself
/// at a bot door is not a server fault.
pub async fn bot_for_token(
    db: &Database,
    token_id: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    // Two ways a token can be a bot's: it is the credential the registration was created with, or it is a reader's link
    // credential, which `issue_token_acting` labels with `kind = 'bot'` and `name = <bot id>`. The second arm is what makes
    // suspension cover linked readers — without it a suspended bot's readers would keep working, because `bot_for_token`
    // only ever looked at the registration row.
    let sql = db.sql(
        "SELECT r.id, r.state FROM bot_registrations r
          WHERE r.token_id = ?1
             OR EXISTS (SELECT 1 FROM api_tokens t WHERE t.id = ?1 AND t.kind = 'bot' AND t.name = r.id)",
        "SELECT r.id, r.state FROM bot_registrations r
          WHERE r.token_id = $1::text
             OR EXISTS (SELECT 1 FROM api_tokens t WHERE t.id = $1::uuid AND t.kind = 'bot' AND t.name = r.id)",
    );
    let row = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .bind(token_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .bind(token_id)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(row)
}

/// Whether this account registered this bot.
///
/// A bot id is a UUID and looks like any other; the challenge door needs to know that the id belongs to the caller before it
/// mints a code for it, and a `404` for somebody else's bot is the answer that discloses least (ADR 0030).
pub async fn bot_owned_by(db: &Database, bot_id: &str, account: &str) -> Result<bool, sqlx::Error> {
    let sql = db.sql(
        // `owner`, not `account_id`: migration 0020 named the column before the account model had settled, and it holds the
        // registering account.
        "SELECT EXISTS (SELECT 1 FROM bot_registrations WHERE id = ?1 AND owner = ?2)",
        // `owner` is TEXT (0020 predates the uuid columns), so the bind is cast to text rather than the column to uuid:
        // `owner = $2::uuid` is a type error on the deployed engine and works on SQLite, which is exactly the trap a
        // two-engine test exists to catch.
        // `bot_registrations` has TEXT ids (0020), so neither column is cast: `id = $1::uuid` is
        // `operator does not exist: text = uuid` on the deployed engine, and a 500 that SQLite never shows.
        "SELECT EXISTS (SELECT 1 FROM bot_registrations WHERE id = $1::text AND owner = $2::text)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(bot_id)
                .bind(account)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(bot_id)
                .bind(account)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await
        }
    }
}

/// Point a bot registration at its current credential, returning the token it pointed at before.
///
/// **Linking rotates the credential.** A linked bot's token is the one the reader just granted, and the registration has to
/// point at it or nothing can check whether the bot is suspended — `bot_for_token` looks the registration up *by token*, so a
/// token the registration does not name is a token whose bot is invisible. The previous token comes back so the caller can
/// revoke it: two live credentials for one bot is one more than the suspension check can reason about.
pub async fn rebind_bot_token(
    db: &Database,
    bot_id: &str,
    token_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let select = db.sql(
        "SELECT token_id FROM bot_registrations WHERE id = ?1",
        "SELECT token_id FROM bot_registrations WHERE id = $1::text",
    );
    let previous: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&select)
                .bind(bot_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&select)
                .bind(bot_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let update = db.sql(
        "UPDATE bot_registrations SET token_id = ?1 WHERE id = ?2",
        "UPDATE bot_registrations SET token_id = $1::text WHERE id = $2::text",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&update)
                .bind(token_id)
                .bind(bot_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&update)
                .bind(token_id)
                .bind(bot_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(previous)
}

/// Resolve a short feed handle to the subject it names.
///
/// `kind` says how the subject is to be read — `tag` is a search term, `user` an author — because a handle is a *name* and the
/// same string means different things in different kinds. Returns `None` for an unknown handle, which is a `404` rather than an
/// empty feed: a feed that answers with nothing forever looks like a quiet subject and is usually a typo.
pub async fn feed_handle(
    db: &Database,
    handle: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let sql = db.sql(
        "SELECT kind, subject FROM feed_handles WHERE handle = ?1",
        "SELECT kind, subject FROM feed_handles WHERE handle = $1",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .bind(handle)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await
        }
        Backend::Postgres => {
            sqlx::query_as::<_, (String, String)>(&sql)
                .bind(handle)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await
        }
    }
}

/// A bot registration's state, by id.
pub async fn bot_by_id(db: &Database, bot_id: &str) -> Result<Option<String>, sqlx::Error> {
    let sql = db.sql(
        "SELECT state FROM bot_registrations WHERE id = ?1",
        "SELECT state FROM bot_registrations WHERE id = $1::text",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(bot_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(bot_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await
        }
    }
}

/// Suspend or reactivate a bot registration.
///
/// The only writer of `state` after insert, which is what makes `bot_for_token`'s
/// check worth having.
pub async fn set_bot_state(db: &Database, bot_id: &str, state: &str) -> Result<bool, sqlx::Error> {
    let sql = db.sql(
        "UPDATE bot_registrations SET state = ? WHERE id = ?",
        "UPDATE bot_registrations SET state = $1 WHERE id = $2::text",
    );
    match db.backend() {
        Backend::Sqlite => {
            let result = sqlx::query(&sql)
                .bind(state)
                .bind(bot_id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(result.rows_affected() > 0)
        }
        Backend::Postgres => {
            let result = sqlx::query(&sql)
                .bind(state)
                .bind(bot_id)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(result.rows_affected() > 0)
        }
    }
}

// ---------------------------------------------------------------------------
// Feeds
// ---------------------------------------------------------------------------

pub async fn upsert_feed_handle(
    db: &Database,
    kind: &str,
    subject: &str,
    handle: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO feed_handles (id, kind, subject, handle, created_at)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(handle) DO UPDATE SET subject = excluded.subject",
            )
            .bind(&id)
            .bind(kind)
            .bind(subject)
            .bind(handle)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO feed_handles (id, kind, subject, handle, created_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT(handle) DO UPDATE SET subject = EXCLUDED.subject",
            )
            .bind(&id)
            .bind(kind)
            .bind(subject)
            .bind(handle)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Push
// ---------------------------------------------------------------------------

pub async fn register_push_subscription(
    db: &Database,
    account: &str,
    endpoint: &str,
    keys: &str,
    device_name: Option<&str>,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO push_subscriptions (id, account, endpoint, keys, device_name, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)"
            )
            .bind(&id).bind(account).bind(endpoint).bind(keys).bind(device_name).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO push_subscriptions (id, account, endpoint, keys, device_name, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6)"
            )
            .bind(&id).bind(account).bind(endpoint).bind(keys).bind(device_name).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Federation
// ---------------------------------------------------------------------------

pub async fn record_inbound(
    db: &Database,
    peer_host: &str,
    object_type: &str,
    object_id: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO federation_inbound (id, peer_host, object_type, object_id, received_at, state)
                 VALUES (?, ?, ?, ?, ?, 'quarantined')"
            )
            .bind(&id).bind(peer_host).bind(object_type).bind(object_id).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO federation_inbound (id, peer_host, object_type, object_id, received_at, state)
                 VALUES ($1, $2, $3, $4, $5, 'quarantined')"
            )
            .bind(&id).bind(peer_host).bind(object_type).bind(object_id).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(id)
}

/// One quarantined intake row as the operator sees it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InboundRecord {
    pub id: String,
    pub peer_host: String,
    pub object_type: String,
    pub object_id: String,
    pub received_at: String,
    pub state: String,
    pub note: Option<String>,
}

/// Recent intake rows, newest first, optionally filtered to one state.
///
/// The ledger exists to be read: `record_inbound` writes `quarantined`, and something has to answer "what is waiting" or the
/// queue is a table pretending to be a policy.
pub async fn inbound_recent(
    db: &Database,
    state: Option<&str>,
    limit: i64,
) -> Result<Vec<InboundRecord>, sqlx::Error> {
    let filter = if state.is_some() {
        " AND state = ?2"
    } else {
        ""
    };
    let pg_filter = if state.is_some() {
        " AND state = $2"
    } else {
        ""
    };
    // Bound to locals first: `db.sql` borrows both strings, and two temporaries inside the one call are dropped before the
    // borrow ends (E0716) — the shape this workspace has been bitten by before.
    let sqlite_sql = format!(
        "SELECT id, peer_host, object_type, object_id, received_at, state, note
         FROM federation_inbound WHERE 1 = 1{filter}
         ORDER BY received_at DESC, id DESC LIMIT ?1"
    );
    let postgres_sql = format!(
        "SELECT id, peer_host, object_type, object_id, received_at, state, note
         FROM federation_inbound WHERE 1 = 1{pg_filter}
         ORDER BY received_at DESC, id DESC LIMIT $1"
    );
    let sql = db.sql(&sqlite_sql, &postgres_sql);
    type InboundRow = (
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
    );
    let rows: Vec<InboundRow> = match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as(&sql).bind(limit);
            if let Some(state) = state {
                query = query.bind(state);
            }
            query
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as(&sql).bind(limit);
            if let Some(state) = state {
                query = query.bind(state);
            }
            query
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(id, peer_host, object_type, object_id, received_at, state, note)| InboundRecord {
                id,
                peer_host,
                object_type,
                object_id,
                received_at,
                state,
                note,
            },
        )
        .collect())
}

/// Move an intake row to a decided state, returning `false` if there is no such row.
///
/// Only `accepted` and `rejected` are reachable: `quarantined` is where intake *starts*, and a door that can put a row back
/// there would be a door that erases the decision without erasing the row.
pub async fn set_inbound_state(
    db: &Database,
    id: &str,
    state: &str,
    note: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let sql = db.sql(
        "UPDATE federation_inbound SET state = ?1, note = ?2 WHERE id = ?3",
        "UPDATE federation_inbound SET state = $1, note = $2 WHERE id = $3",
    );
    let changed = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(state)
            .bind(note)
            .bind(id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(state)
            .bind(note)
            .bind(id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(changed > 0)
}

// ---------------------------------------------------------------------------
// AI
// ---------------------------------------------------------------------------

pub async fn record_ai_request(
    db: &Database,
    work_id: &str,
    provider: &str,
    purpose: &str,
    charged_transaction: Option<&str>,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO ai_requests (id, work_id, provider, account, purpose, charged_transaction, requested_at)
                 VALUES (?, ?, ?, NULL, ?, ?, ?)"
            )
            .bind(&id).bind(work_id).bind(provider).bind(purpose).bind(charged_transaction).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO ai_requests (id, work_id, provider, account, purpose, charged_transaction, requested_at)
                 VALUES ($1, $2, $3, NULL, $4, $5, $6)"
            )
            .bind(&id).bind(work_id).bind(provider).bind(purpose).bind(charged_transaction).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(id)
}

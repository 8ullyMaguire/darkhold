//! M19 — Admin repository: admin actions, feature flags, abuse tracking, privacy requests.

use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{Backend, Database};

// ---------------------------------------------------------------------------
// Admin Actions
// ---------------------------------------------------------------------------

pub async fn record_admin_action(
    db: &Database,
    actor: &str,
    action: &str,
    subject_type: &str,
    subject_id: &str,
    document: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO admin_actions (id, actor, action, subject_type, subject_id, document, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&id).bind(actor).bind(action).bind(subject_type).bind(subject_id).bind(document).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO admin_actions (id, actor, action, subject_type, subject_id, document, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)"
            )
            .bind(&id).bind(actor).bind(action).bind(subject_type).bind(subject_id).bind(document).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Abuse Counters
// ---------------------------------------------------------------------------

pub async fn increment_abuse_counter(
    db: &Database,
    key: &str,
    window: &str,
) -> Result<i64, sqlx::Error> {
    let count = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(
                "INSERT INTO abuse_counters (key, \"window\", count) VALUES (?, ?, 1)
                 ON CONFLICT(key, \"window\") DO UPDATE SET count = count + 1
                 RETURNING count",
            )
            .bind(key)
            .bind(window)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(
                "INSERT INTO abuse_counters (key, \"window\", count) VALUES ($1, $2, 1)
                 ON CONFLICT(key, \"window\") DO UPDATE SET count = abuse_counters.count + 1
                 RETURNING count::bigint",
            )
            .bind(key)
            .bind(window)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await?
        }
    };
    Ok(count)
}

// ---------------------------------------------------------------------------
// Privacy Requests
// ---------------------------------------------------------------------------

pub async fn create_privacy_request(
    db: &Database,
    account: &str,
    kind: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO privacy_requests (id, account, kind, state, requested_at)
                 VALUES (?, ?, ?, 'pending', ?)",
            )
            .bind(&id)
            .bind(account)
            .bind(kind)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO privacy_requests (id, account, kind, state, requested_at)
                 VALUES ($1, $2, $3, 'pending', $4)",
            )
            .bind(&id)
            .bind(account)
            .bind(kind)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(id)
}

/// Close a privacy request, and answer whether this call is what closed it.
///
/// `WHERE state = 'pending'` is the whole point: `completed_at` and `result_ref` are the two columns that make the record
/// evidence, and a second completion would rewrite them to say whichever operator came last. `false` means "this call changed
/// nothing", which the route turns into a `404` or a `422` by asking [`privacy_request_state`] why.
pub async fn complete_privacy_request(
    db: &Database,
    request_id: &str,
    result_ref: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "UPDATE privacy_requests SET state = 'done', completed_at = ?, result_ref = ?
                  WHERE id = ? AND state = 'pending'",
        )
        .bind(&now)
        .bind(result_ref)
        .bind(request_id)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await?
        .rows_affected(),
        Backend::Postgres => sqlx::query(
            "UPDATE privacy_requests SET state = 'done', completed_at = $1, result_ref = $2
                  WHERE id = $3 AND state = 'pending'",
        )
        .bind(&now)
        .bind(result_ref)
        .bind(request_id)
        .execute(db.postgres_pool().expect("postgres"))
        .await?
        .rows_affected(),
    };
    Ok(affected > 0)
}

/// The request's current state, or `None` when no request has that id.
///
/// Separate from the completion so the route can tell "never existed" (`404`) from "already done" (`422`) — two answers a caller
/// acts on differently, and neither of which a bare `false` carries.
pub async fn privacy_request_state(
    db: &Database,
    request_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let sql = db.sql(
        "SELECT state FROM privacy_requests WHERE id = ?",
        "SELECT state FROM privacy_requests WHERE id = $1",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(request_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(request_id)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await
        }
    }
}

/// Every account's requests, newest first, optionally one state — the operator's queue.
///
/// `list_privacy_requests` is per account and cannot answer this: a queue shows one person their own obligation, which is the
/// wrong question for whoever has to drain it. `state` is `None` for the whole record, because "what did we ever promise" is a
/// question the audit asks and the pending filter would hide the answer.
pub async fn list_all_privacy_requests(
    db: &Database,
    state: Option<&str>,
    limit: i64,
) -> Result<Vec<Value>, sqlx::Error> {
    // Two strings, not `db.sql(…)`: the filter binds the state twice on SQLite (no `?1` numbering) and once on PostgreSQL.
    // Mapped per arm, because `SqliteRow` and `PgRow` are different types and only the JSON is shared.
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT id, account, kind, state, requested_at, completed_at, result_ref
                   FROM privacy_requests
                  WHERE (? IS NULL OR state = ?)
                  ORDER BY requested_at DESC, id DESC
                  LIMIT ?",
            )
            .bind(state)
            .bind(state)
            .bind(limit)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "account": r.get::<String, _>("account"),
                        "kind": r.get::<String, _>("kind"),
                        "state": r.get::<String, _>("state"),
                        "requested_at": r.get::<String, _>("requested_at"),
                        "completed_at": r.get::<Option<String>, _>("completed_at"),
                        "result_ref": r.get::<Option<String>, _>("result_ref"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT id, account, kind, state, requested_at, completed_at, result_ref
                   FROM privacy_requests
                  WHERE ($1::text IS NULL OR state = $1)
                  ORDER BY requested_at DESC, id DESC
                  LIMIT $2",
            )
            .bind(state)
            .bind(limit)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "account": r.get::<String, _>("account"),
                        "kind": r.get::<String, _>("kind"),
                        "state": r.get::<String, _>("state"),
                        "requested_at": r.get::<String, _>("requested_at"),
                        "completed_at": r.get::<Option<String>, _>("completed_at"),
                        "result_ref": r.get::<Option<String>, _>("result_ref"),
                    })
                })
                .collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Privacy request listing (one account)
// ---------------------------------------------------------------------------

/// One account's own requests, newest first — the person's view of what they asked for.
///
/// The operator's queue is [`list_all_privacy_requests`]; this one is deliberately per account, because a person's list must not
/// be able to answer "what have other people asked for".
pub async fn list_privacy_requests(
    db: &Database,
    account: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    let select = "SELECT id, account, kind, state, requested_at, completed_at, result_ref
                    FROM privacy_requests
                   WHERE account = ";
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&format!("{select}? ORDER BY requested_at DESC, id DESC"))
                .bind(account)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "account": r.get::<String, _>("account"),
                        "kind": r.get::<String, _>("kind"),
                        "state": r.get::<String, _>("state"),
                        "requested_at": r.get::<String, _>("requested_at"),
                        "completed_at": r.get::<Option<String>, _>("completed_at"),
                        "result_ref": r.get::<Option<String>, _>("result_ref"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(&format!("{select}$1 ORDER BY requested_at DESC, id DESC"))
                .bind(account)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "account": r.get::<String, _>("account"),
                        "kind": r.get::<String, _>("kind"),
                        "state": r.get::<String, _>("state"),
                        "requested_at": r.get::<String, _>("requested_at"),
                        "completed_at": r.get::<Option<String>, _>("completed_at"),
                        "result_ref": r.get::<Option<String>, _>("result_ref"),
                    })
                })
                .collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Abuse status
// ---------------------------------------------------------------------------

/// One key's abuse signal, summed across the hourly windows it was recorded in.
#[derive(Debug, Clone)]
pub struct AbuseCounter {
    /// The key the limiter counted: `ip:<class>:<address>` or `user:<class>:<account>`.
    pub key: String,
    /// Every refusal recorded against this key, across every window.
    pub count: i64,
    /// Whether somebody has put a temporary mitigation on the key. A count never sets this.
    pub blocked: bool,
    /// The most recent window with a refusal, so an operator can tell a live pattern from an old one.
    pub last_window: String,
}

/// The busiest keys, summed across windows.
///
/// **Why a listing and not just a lookup.** §24.4's operator view is "repeated limit violations, temporary mitigations and expiry" — plural, and in
/// order of what is happening now. `check_abuse_status` answers about a key you already suspect, which is the wrong shape for an operator who does
/// not yet know which key to ask about: the middle of a burst is exactly when nobody knows the key.
///
/// Summed rather than per-window, for the same reason the lookup sums: the counters age out hourly, and a report that showed one row per hour
/// would bury a sustained flood under a list of forty quiet ones. `last_window` carries the recency that summing discards.
pub async fn list_abuse_counters(
    db: &Database,
    limit: i64,
) -> Result<Vec<AbuseCounter>, sqlx::Error> {
    let sql = db.sql(
        "SELECT key, SUM(count) AS count, MAX(blocked_until) AS blocked_until, MAX(\"window\") AS last_window
           FROM abuse_counters GROUP BY key ORDER BY SUM(count) DESC, key ASC LIMIT ?",
        "SELECT key, SUM(count)::bigint AS count, MAX(blocked_until) AS blocked_until, MAX(\"window\") AS last_window
           FROM abuse_counters GROUP BY key ORDER BY SUM(count) DESC, key ASC LIMIT $1",
    );
    // Mapped inside each arm: a row type is parameterised by its database, so a shared `Vec` of rows across the two arms cannot be typed —
    // the same shape of mistake that makes `Query` un-shareable between dialects.
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&sql)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows
                .iter()
                .map(|row| AbuseCounter {
                    key: row.get::<String, _>("key"),
                    count: row.get::<i64, _>("count"),
                    blocked: row.get::<Option<String>, _>("blocked_until").is_some(),
                    last_window: row.get::<Option<String>, _>("last_window").unwrap_or_default(),
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(&sql)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows
                .iter()
                .map(|row| AbuseCounter {
                    key: row.get::<String, _>("key"),
                    count: row.get::<i64, _>("count"),
                    blocked: row.get::<Option<String>, _>("blocked_until").is_some(),
                    last_window: row.get::<Option<String>, _>("last_window").unwrap_or_default(),
                })
                .collect())
        }
    }
}

pub async fn check_abuse_status(db: &Database, key: &str) -> Result<(bool, i64), sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            let row = // Summed across windows: the counters are written per hour, so reading a single row answered a lifetime question with an
            // hourly number — and which row came back was the database's choice.
            sqlx::query("SELECT COALESCE(SUM(count), 0) AS count, MAX(blocked_until) AS blocked_until FROM abuse_counters WHERE key = ?")
                .bind(key)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?;
            match row {
                Some(r) => {
                    let blocked = r.get::<Option<String>, _>("blocked_until").is_some();
                    let count = r.get::<i64, _>("count");
                    Ok((blocked, count))
                }
                None => Ok((false, 0)),
            }
        }
        Backend::Postgres => {
            let row = sqlx::query("SELECT COALESCE(SUM(count), 0)::bigint AS count, MAX(blocked_until) AS blocked_until FROM abuse_counters WHERE key = $1")
                .bind(key)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?;
            match row {
                Some(r) => Ok((
                    r.get::<Option<String>, _>("blocked_until").is_some(),
                    r.get::<i64, _>("count"),
                )),
                None => Ok((false, 0)),
            }
        }
    }
}

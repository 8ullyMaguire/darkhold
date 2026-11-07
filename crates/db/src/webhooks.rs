//! M16-14: webhook subscription operations — rotation, rate limits, the breaker.
//!
//! The arithmetic lives in `darkhold_domain::webhook` (`Breaker`, `RateLimit`, `admissible_keys`); this
//! module only reads and writes the rows those decisions describe. No function here decides a threshold.

use anyhow::Result;
use sqlx::Row as _;
use serde_json::{json, Value};

use crate::{Backend, Database};

/// Rotate an endpoint's secret, returning the new one — once.
///
/// The old secret is kept in `previous_secret` with `rotated_at` so deliveries keep verifying against it
/// for the grace window (see `darkhold_domain::webhook::ROTATION_GRACE_SECONDS`). Two statements rather
/// than one because the new secret has to be generated in Rust: a `RETURNING` would report the old one.
///
/// `None` means the endpoint does not exist or is not this owner's — the two are the same answer on
/// purpose, so a caller cannot probe for another subscriber's endpoint ids.
pub async fn rotate_secret(
    db: &Database,
    endpoint_id: &str,
    owner: &str,
    new_secret: &str,
    rotated_at: &str,
) -> Result<Option<String>> {
    match db.backend() {
        Backend::Sqlite => {
            let pool = db.sqlite_pool().expect("sqlite");
            let current: Option<String> = sqlx::query_scalar(
                "SELECT secret FROM webhook_endpoints WHERE id = ? AND owner = ?",
            )
            .bind(endpoint_id)
            .bind(owner)
            .fetch_optional(pool)
            .await?;
            let Some(current) = current else {
                return Ok(None);
            };
            sqlx::query(
                "UPDATE webhook_endpoints SET previous_secret = ?, rotated_at = ?, secret = ? WHERE id = ?",
            )
            .bind(&current)
            .bind(rotated_at)
            .bind(new_secret)
            .bind(endpoint_id)
            .execute(pool)
            .await?;
            Ok(Some(new_secret.to_owned()))
        }
        Backend::Postgres => {
            let pool = db.postgres_pool().expect("postgres");
            let current: Option<String> = sqlx::query_scalar(
                "SELECT secret FROM webhook_endpoints WHERE id = $1 AND owner = $2",
            )
            .bind(endpoint_id)
            .bind(owner)
            .fetch_optional(pool)
            .await?;
            let Some(current) = current else {
                return Ok(None);
            };
            sqlx::query(
                "UPDATE webhook_endpoints SET previous_secret = $1, rotated_at = $2, secret = $3 WHERE id = $4",
            )
            .bind(&current)
            .bind(rotated_at)
            .bind(new_secret)
            .bind(endpoint_id)
            .execute(pool)
            .await?;
            Ok(Some(new_secret.to_owned()))
        }
    }
}

/// How many deliveries this endpoint was actually sent since `since`, excluding throttled ones.
///
/// Throttled rows are excluded because they did not reach the receiver: counting them would let the
/// limiter consume its own budget, so a burst would extend its own silence.
pub async fn delivered_since(db: &Database, endpoint_id: &str, since: &str) -> Result<i64> {
    match db.backend() {
        Backend::Sqlite => {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM webhook_deliveries WHERE endpoint_id = ? AND attempted_at >= ? AND status <> 'throttled'",
            )
            .bind(endpoint_id)
            .bind(since)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(count)
        }
        Backend::Postgres => {
            let count: i64 = sqlx::query_scalar(
                "SELECT CAST(COUNT(*) AS BIGINT) FROM webhook_deliveries WHERE endpoint_id = $1 AND attempted_at >= $2 AND status <> 'throttled'",
            )
            .bind(endpoint_id)
            .bind(since)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(count)
        }
    }
}

/// Record a successful delivery: the consecutive-failure count goes to zero.
///
/// A success is the only thing that resets the count. Clearing it on a `retrying` row would make the
/// breaker count attempts instead of episodes, and an endpoint that fails twice then times out would
/// never trip.
pub async fn record_success(db: &Database, endpoint_id: &str) -> Result<()> {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("UPDATE webhook_endpoints SET consecutive_failures = 0 WHERE id = ?")
                .bind(endpoint_id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query("UPDATE webhook_endpoints SET consecutive_failures = 0 WHERE id = $1")
                .bind(endpoint_id)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(())
}

/// Apply a failed delivery to the breaker, switching the endpoint off at the threshold.
///
/// Returns the reason when it just switched off (so the caller can log one line per disable rather than
/// per failure), and `None` otherwise. The threshold is passed in — this function does not own the
/// number, `darkhold_domain::webhook::Breaker` does.
pub async fn record_failure(
    db: &Database,
    endpoint_id: &str,
    threshold: u32,
    now: &str,
) -> Result<Option<String>> {
    use darkhold_domain::webhook::{Breaker, BreakerDecision};

    let previous: Option<i64> = match db.backend() {
        Backend::Sqlite => sqlx::query_scalar(
            "SELECT consecutive_failures FROM webhook_endpoints WHERE id = ?",
        )
        .bind(endpoint_id)
        .fetch_optional(db.sqlite_pool().expect("sqlite"))
        .await?,
        Backend::Postgres => sqlx::query_scalar(
            "SELECT CAST(consecutive_failures AS BIGINT) FROM webhook_endpoints WHERE id = $1",
        )
        .bind(endpoint_id)
        .fetch_optional(db.postgres_pool().expect("postgres"))
        .await?,
    };
    let Some(previous) = previous else {
        return Ok(None);
    };

    let decision = Breaker { threshold }.after_failure(u32::try_from(previous).unwrap_or(u32::MAX));
    match decision {
        BreakerDecision::Continue {
            consecutive_failures,
        } => {
            match db.backend() {
                Backend::Sqlite => {
                    sqlx::query("UPDATE webhook_endpoints SET consecutive_failures = ? WHERE id = ?")
                        .bind(i64::from(consecutive_failures))
                        .bind(endpoint_id)
                        .execute(db.sqlite_pool().expect("sqlite"))
                        .await?;
                }
                Backend::Postgres => {
                    sqlx::query(
                        "UPDATE webhook_endpoints SET consecutive_failures = $1 WHERE id = $2",
                    )
                    .bind(i64::from(consecutive_failures))
                    .bind(endpoint_id)
                    .execute(db.postgres_pool().expect("postgres"))
                    .await?;
                }
            }
            Ok(None)
        }
        BreakerDecision::Disable {
            consecutive_failures,
            reason,
        } => {
            match db.backend() {
                Backend::Sqlite => {
                    sqlx::query(
                        "UPDATE webhook_endpoints SET consecutive_failures = ?, active = 0, disabled_at = ?, disabled_reason = ? WHERE id = ?",
                    )
                    .bind(i64::from(consecutive_failures))
                    .bind(now)
                    .bind(&reason)
                    .bind(endpoint_id)
                    .execute(db.sqlite_pool().expect("sqlite"))
                    .await?;
                }
                Backend::Postgres => {
                    sqlx::query(
                        "UPDATE webhook_endpoints SET consecutive_failures = $1, active = 0, disabled_at = $2, disabled_reason = $3 WHERE id = $4",
                    )
                    .bind(i64::from(consecutive_failures))
                    .bind(now)
                    .bind(&reason)
                    .bind(endpoint_id)
                    .execute(db.postgres_pool().expect("postgres"))
                    .await?;
                }
            }
            Ok(Some(reason))
        }
    }
}

/// Switch a disabled endpoint back on, resetting the breaker.
///
/// Re-enabling is deliberately an explicit act: an endpoint that switched itself off did so because its
/// receiver was gone, and turning it on again a minute later would only re-run the same twenty failures.
/// The reason is cleared with the flag — a stale reason on an active endpoint reads as a current fault.
pub async fn reenable(db: &Database, endpoint_id: &str, owner: Option<&str>) -> Result<bool> {
    let affected = match db.backend() {
        Backend::Sqlite => {
            let sql = if owner.is_some() {
                "UPDATE webhook_endpoints SET active = 1, consecutive_failures = 0, disabled_at = NULL, disabled_reason = NULL WHERE id = ? AND owner = ?"
            } else {
                "UPDATE webhook_endpoints SET active = 1, consecutive_failures = 0, disabled_at = NULL, disabled_reason = NULL WHERE id = ?"
            };
            let mut q = sqlx::query(sql).bind(endpoint_id);
            if let Some(owner) = owner {
                q = q.bind(owner);
            }
            q.execute(db.sqlite_pool().expect("sqlite")).await?.rows_affected()
        }
        Backend::Postgres => {
            let sql = if owner.is_some() {
                "UPDATE webhook_endpoints SET active = 1, consecutive_failures = 0, disabled_at = NULL, disabled_reason = NULL WHERE id = $1 AND owner = $2"
            } else {
                "UPDATE webhook_endpoints SET active = 1, consecutive_failures = 0, disabled_at = NULL, disabled_reason = NULL WHERE id = $1"
            };
            let mut q = sqlx::query(sql).bind(endpoint_id);
            if let Some(owner) = owner {
                q = q.bind(owner);
            }
            q.execute(db.postgres_pool().expect("postgres"))
                .await?
                .rows_affected()
        }
    };
    Ok(affected > 0)
}

/// Every subscription with its operational state — the operator's view.
///
/// The secret is not in this projection. An operator needs to know an endpoint is failing, not to be
/// able to forge its deliveries: only the owner may read a secret, and only at rotation time.
pub async fn subscriptions(db: &Database) -> Result<Vec<Value>> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT id, owner, url, events, active, rate_per_minute, consecutive_failures, disabled_at, disabled_reason, rotated_at, created_at FROM webhook_endpoints ORDER BY owner, created_at",
            )
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    json!({
                        "id": r.get::<String, _>("id"),
                        "owner": r.get::<String, _>("owner"),
                        "url": r.get::<String, _>("url"),
                        "events": r.get::<String, _>("events"),
                        "active": r.get::<i64, _>("active") == 1,
                        "rate_per_minute": r.get::<i64, _>("rate_per_minute"),
                        "consecutive_failures": r.get::<i64, _>("consecutive_failures"),
                        "disabled_at": r.get::<Option<String>, _>("disabled_at"),
                        "disabled_reason": r.get::<Option<String>, _>("disabled_reason"),
                        "rotated_at": r.get::<Option<String>, _>("rotated_at"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT id, owner, url, events, CAST(active AS BIGINT) AS active, CAST(rate_per_minute AS BIGINT) AS rate_per_minute, CAST(consecutive_failures AS BIGINT) AS consecutive_failures, disabled_at, disabled_reason, rotated_at, created_at FROM webhook_endpoints ORDER BY owner, created_at",
            )
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    json!({
                        "id": r.get::<String, _>("id"),
                        "owner": r.get::<String, _>("owner"),
                        "url": r.get::<String, _>("url"),
                        "events": r.get::<String, _>("events"),
                        "active": r.get::<i64, _>("active") == 1,
                        "rate_per_minute": r.get::<i64, _>("rate_per_minute"),
                        "consecutive_failures": r.get::<i64, _>("consecutive_failures"),
                        "disabled_at": r.get::<Option<String>, _>("disabled_at"),
                        "disabled_reason": r.get::<Option<String>, _>("disabled_reason"),
                        "rotated_at": r.get::<Option<String>, _>("rotated_at"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
    }
}

//! Storage for §21.5's review workflow (M16-07).
//!
//! The decisions are made in `darkhold_domain::extension_review`; this module reads a row, hands it to that state machine, and writes back
//! what it said. Two consequences worth stating, because both are load-bearing:
//!
//! * **No function here decides a stage.** If a route tried to set `stage = 'published'` directly it would still have to write a row this
//!   module's readers interpret — but the reason the state machine is a value is that a test can prove the rule without a database, and
//!   the reason this module is thin is that there is only one place a rule could be added twice.
//! * **A missing review is not an error.** `review_for` returns `None` for a package nobody has submitted to review, which is what every
//!   package published before this table existed looks like.

use sqlx::Row;

use darkhold_domain::extension_review::{Check, Review, Stage};

use crate::{Backend, Database};

/// Read one submission's review, or `None` when nobody has submitted it.
pub async fn review_for(
    db: &Database,
    manifest_id: &str,
    version: &str,
) -> Result<Option<Review>, sqlx::Error> {
    // Extracted inside each arm: the two pools are different types, so only plain values are common between them.
    let row: Option<ReviewRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "SELECT stage, submitter, checks, permission_reviewer, security_reviewer, approvals
                 FROM extension_reviews WHERE manifest_id = ? AND version = ?",
            )
            .bind(manifest_id)
            .bind(version)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?
            .map(review_row_sqlite)
        }
        Backend::Postgres => {
            sqlx::query(
                "SELECT stage, submitter, checks, permission_reviewer, security_reviewer, approvals
                 FROM extension_reviews WHERE manifest_id = $1 AND version = $2",
            )
            .bind(manifest_id)
            .bind(version)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
            .map(review_row_pg)
        }
    };
    let Some((stage, submitter, checks, permission_reviewer, security_reviewer, approvals)) = row else {
        return Ok(None);
    };
    Ok(Some(Review {
        stage: Stage::from_str(&stage).unwrap_or_default(),
        submitter,
        checks: decode_checks(checks),
        permission_reviewer,
        security_reviewer,
        approvals: decode_list(approvals),
    }))
}

/// Create or replace a submission's review row.
///
/// Called at submission (starting at `Submitted`) and after every accepted action, so the row is always what the state machine last said.
pub async fn save_review(
    db: &Database,
    manifest_id: &str,
    version: &str,
    review: &Review,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let checks = encode_checks(&review.checks);
    let approvals = serde_json::to_string(&review.approvals).unwrap_or_else(|_| "[]".to_owned());
    let stage = review.stage.as_str();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO extension_reviews
                     (manifest_id, version, stage, submitter, checks, permission_reviewer,
                      security_reviewer, approvals, reason, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(manifest_id, version) DO UPDATE SET
                     stage=excluded.stage, checks=excluded.checks,
                     permission_reviewer=excluded.permission_reviewer,
                     security_reviewer=excluded.security_reviewer,
                     approvals=excluded.approvals, reason=excluded.reason, updated_at=excluded.updated_at",
            )
            .bind(manifest_id)
            .bind(version)
            .bind(stage)
            .bind(&review.submitter)
            .bind(&checks)
            .bind(&review.permission_reviewer)
            .bind(&review.security_reviewer)
            .bind(&approvals)
            .bind(reason)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO extension_reviews
                     (manifest_id, version, stage, submitter, checks, permission_reviewer,
                      security_reviewer, approvals, reason, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT(manifest_id, version) DO UPDATE SET
                     stage=excluded.stage, checks=excluded.checks,
                     permission_reviewer=excluded.permission_reviewer,
                     security_reviewer=excluded.security_reviewer,
                     approvals=excluded.approvals, reason=excluded.reason, updated_at=excluded.updated_at",
            )
            .bind(manifest_id)
            .bind(version)
            .bind(stage)
            .bind(&review.submitter)
            .bind(&checks)
            .bind(&review.permission_reviewer)
            .bind(&review.security_reviewer)
            .bind(&approvals)
            .bind(reason)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(())
}

/// The queue: everything at one stage, oldest first, so a reviewer works the backlog rather than the newest arrival.
pub async fn queue(
    db: &Database,
    stage: Stage,
    limit: i64,
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    // Per-engine extraction rather than a shared generic: `impl Row` cannot be used here because the two engines' `Database` types differ,
    // and the honest common denominator is the finished JSON.
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT manifest_id, version, stage, submitter, updated_at FROM extension_reviews
                 WHERE stage = ? ORDER BY updated_at ASC LIMIT ?",
            )
            .bind(stage.as_str())
            .bind(limit)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows.iter().map(queue_row_sqlite).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT manifest_id, version, stage, submitter, updated_at FROM extension_reviews
                 WHERE stage = $1 ORDER BY updated_at ASC LIMIT $2",
            )
            .bind(stage.as_str())
            .bind(limit)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows.iter().map(queue_row_pg).collect())
        }
    }
}

/// Set a published package version's state — `approved` on publish, `revoked` on emergency revocation.
pub async fn set_manifest_state(
    db: &Database,
    manifest_id: &str,
    version: &str,
    state: &str,
) -> Result<bool, sqlx::Error> {
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query("UPDATE extension_manifests SET state = ? WHERE id = ? AND version = ?")
                .bind(state)
                .bind(manifest_id)
                .bind(version)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?
                .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query("UPDATE extension_manifests SET state = $1 WHERE id = $2 AND version = $3")
                .bind(state)
                .bind(manifest_id)
                .bind(version)
                .execute(db.postgres_pool().expect("postgres"))
                .await?
                .rows_affected()
        }
    };
    Ok(affected > 0)
}

/// The version a revocation falls back to: the newest `approved` version of the same package that is not `version`.
///
/// `None` means there is nothing to fall back to, and the package is simply gone — which is the honest answer for a first release, and the
/// one a caller has to handle rather than be told a version exists when it does not.
pub async fn previous_published(
    db: &Database,
    manifest_id: &str,
    version: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<String> = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "SELECT version FROM extension_manifests
             WHERE id = ? AND state = 'approved' AND version <> ?
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(manifest_id)
        .bind(version)
        .fetch_optional(db.sqlite_pool().expect("sqlite"))
        .await?
        .map(|row| row.get::<String, _>("version")),
        Backend::Postgres => sqlx::query(
            "SELECT version FROM extension_manifests
             WHERE id = $1 AND state = 'approved' AND version <> $2
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(manifest_id)
        .bind(version)
        .fetch_optional(db.postgres_pool().expect("postgres"))
        .await?
        .map(|row| row.get::<String, _>("version")),
    };
    Ok(row)
}

/// Invalidate the grants a new version's capabilities outgrow — §21.5's "renewed consent when permissions grow".
///
/// Version-independent on purpose: a grant is one row per (account, package) that names the version it was granted for, so a reader who
/// consented to `1.0.0`'s `work.read` and is now offered `1.1.0`'s `work.read` + `network` has not consented to what is on offer. Their
/// grant is revoked rather than quietly widened, and the count is returned so the publish response can say how many readers must be asked
/// again.
///
/// Rows are compared in Rust, not in SQL: the capability set is a JSON array and `needs_renewed_consent` is the one place that decides
/// what "grew" means, so the database is not asked to hold a second opinion about it in a dialect-specific query.
pub async fn renewal_on_growth(
    db: &Database,
    manifest_id: &str,
    new_version: &str,
    requested: &std::collections::BTreeSet<String>,
) -> Result<u64, sqlx::Error> {
    if requested.is_empty() {
        // A version that asks for nothing cannot outgrow a consent: asking every reader again to run a pure module would train them to
        // approve prompts without reading them.
        return Ok(0);
    }
    let now = crate::identity::now_rfc3339();
    let rows: Vec<(String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT account, version, capabilities FROM extension_grants
                 WHERE manifest_id = ? AND revoked_at IS NULL",
            )
            .bind(manifest_id)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            rows.iter()
                .map(|row| (row.get("account"), row.get("version"), row.get("capabilities")))
                .collect()
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT account, version, capabilities FROM extension_grants
                 WHERE manifest_id = $1 AND revoked_at IS NULL",
            )
            .bind(manifest_id)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            rows.iter()
                .map(|row| (row.get("account"), row.get("version"), row.get("capabilities")))
                .collect()
        }
    };
    let mut renewed = 0u64;
    for (account, version, capabilities) in rows {
        // The grant that produced the published version is not a growth: publishing over a grant the reader just gave for this version
        // would revoke the consent that made the publish possible.
        if version == new_version {
            continue;
        }
        let granted: std::collections::BTreeSet<String> =
            serde_json::from_str(&capabilities).unwrap_or_default();
        if !darkhold_domain::extension_review::Review::needs_renewed_consent(&granted, requested) {
            continue;
        }
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(
                    "UPDATE extension_grants SET revoked_at = ?
                     WHERE account = ? AND manifest_id = ? AND revoked_at IS NULL",
                )
                .bind(&now)
                .bind(&account)
                .bind(manifest_id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
            }
            Backend::Postgres => {
                sqlx::query(
                    "UPDATE extension_grants SET revoked_at = $1
                     WHERE account = $2 AND manifest_id = $3 AND revoked_at IS NULL",
                )
                .bind(&now)
                .bind(&account)
                .bind(manifest_id)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
            }
        }
        renewed += 1;
    }
    Ok(renewed)
}

/// Mark every grant of one package **version** as needing fresh consent.
///
/// Version-scoped rather than package-scoped: a reader who granted `1.0.0` has consented to `1.0.0`'s capabilities. `1.1.0` that asks for
/// more is a different consent, and leaving the old grant active would let the new version inherit it by accident — which is exactly the
/// failure §21.5's "renewed consent when permissions grow" exists to prevent.
pub async fn require_renewal(
    db: &Database,
    manifest_id: &str,
    version: &str,
) -> Result<u64, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "UPDATE extension_grants SET revoked_at = ?
                 WHERE manifest_id = ? AND version = ? AND revoked_at IS NULL",
            )
            .bind(&now)
            .bind(manifest_id)
            .bind(version)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query(
                "UPDATE extension_grants SET revoked_at = $1
                 WHERE manifest_id = $2 AND version = $3 AND revoked_at IS NULL",
            )
            .bind(&now)
            .bind(manifest_id)
            .bind(version)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected()
        }
    };
    Ok(affected)
}

fn encode_checks(checks: &[Check]) -> String {
    let list: Vec<serde_json::Value> = checks
        .iter()
        .map(|check| {
            serde_json::json!({
                "name": check.name,
                "passed": check.outcome.is_ok(),
                "reason": check.outcome.as_ref().err(),
            })
        })
        .collect();
    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_owned())
}

fn decode_checks(text: String) -> Vec<Check> {
    serde_json::from_str::<Vec<serde_json::Value>>(&text)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| {
            let name = value.get("name")?.as_str()?.to_owned();
            let passed = value.get("passed").and_then(serde_json::Value::as_bool)?;
            let reason = value
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            Some(Check {
                name,
                outcome: if passed {
                    Ok(())
                } else {
                    Err(reason.unwrap_or_default())
                },
            })
        })
        .collect()
}

fn decode_list(text: String) -> Vec<String> {
    serde_json::from_str(&text).unwrap_or_default()
}

type ReviewRow = (String, String, String, Option<String>, Option<String>, String);

fn review_row_sqlite(row: sqlx::sqlite::SqliteRow) -> ReviewRow {
    (
        row.get("stage"),
        row.get("submitter"),
        row.get("checks"),
        row.get("permission_reviewer"),
        row.get("security_reviewer"),
        row.get("approvals"),
    )
}

fn review_row_pg(row: sqlx::postgres::PgRow) -> ReviewRow {
    (
        row.get("stage"),
        row.get("submitter"),
        row.get("checks"),
        row.get("permission_reviewer"),
        row.get("security_reviewer"),
        row.get("approvals"),
    )
}

fn queue_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> serde_json::Value {
    serde_json::json!({
        "manifest_id": row.get::<String, _>("manifest_id"),
        "version": row.get::<String, _>("version"),
        "stage": row.get::<String, _>("stage"),
        "submitter": row.get::<String, _>("submitter"),
        "updated_at": row.get::<String, _>("updated_at"),
    })
}

fn queue_row_pg(row: &sqlx::postgres::PgRow) -> serde_json::Value {
    serde_json::json!({
        "manifest_id": row.get::<String, _>("manifest_id"),
        "version": row.get::<String, _>("version"),
        "stage": row.get::<String, _>("stage"),
        "submitter": row.get::<String, _>("submitter"),
        "updated_at": row.get::<String, _>("updated_at"),
    })
}

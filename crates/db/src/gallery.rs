//! Installations and ratings (M16-08).
//!
//! The invariants are in `darkhold_domain::gallery`; this module makes them true in the store. Three of them cannot be enforced by a
//! convention and are therefore enforced by the schema or by this file:
//!
//! * **Idempotence under concurrency.** `install` inserts with `ON CONFLICT DO NOTHING` against the partial unique index on
//!   `(scope, manifest_id) WHERE uninstalled_at IS NULL`, then reads back whichever row is active. Two simultaneous installs produce one
//!   row and both callers get the same answer; a check-then-insert would produce one row and one 500.
//! * **A count that cannot be inflated.** `install_count` is `COUNT(*) WHERE uninstalled_at IS NULL` — a census at read time. Nothing
//!   increments a counter, so reinstalling cannot move it.
//! * **No per-account history escapes.** `installed_by` is deliberately not a function: the only reader-facing installation read is
//!   `my_installations(account)`, which is the caller's own.

use serde_json::{json, Value};
use sqlx::Row;

use darkhold_domain::gallery::{InstallOutcome, Installation, Score, UpdatePolicy};

use crate::{Backend, Database};

const INSTALL_COLUMNS: &str = "id, manifest_id, version, update_policy, uninstalled_at";

/// Install a package version for a scope, idempotently.
///
/// Returns `AlreadyInstalled` **with** the existing installation when one is active — the caller's answer is the same either way, and the
/// count does not move. The `id` is derived from the scope and the package so a retry that loses the race still resolves to one row.
pub async fn install(
    db: &Database,
    scope: &str,
    manifest_id: &str,
    version: &str,
    policy: UpdatePolicy,
) -> Result<(InstallOutcome, Installation), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let id = format!("{scope}:{manifest_id}");
    let inserted = match db.backend() {
        Backend::Sqlite => sqlx::query(
            // Three outcomes in one statement: a new row, a no-op on an active row (the idempotent case), and a reactivation when the
            // row exists but was uninstalled. The `WHERE` on the upsert is what makes the no-op silent -- without it a second install
            // would rewrite `installed_at` and the count would be right while the record was a lie.
            "INSERT INTO extension_installations
                 (id, scope, manifest_id, version, update_policy, installed_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                 version=excluded.version, update_policy=excluded.update_policy,
                 installed_at=excluded.installed_at, updated_at=excluded.updated_at,
                 uninstalled_at=NULL
             WHERE extension_installations.uninstalled_at IS NOT NULL",
        )
        .bind(&id)
        .bind(scope)
        .bind(manifest_id)
        .bind(version)
        .bind(policy.as_str())
        .bind(&now)
        .bind(&now)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await?
        .rows_affected(),
        Backend::Postgres => sqlx::query(
            "INSERT INTO extension_installations
                 (id, scope, manifest_id, version, update_policy, installed_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT(id) DO UPDATE SET
                 version=excluded.version, update_policy=excluded.update_policy,
                 installed_at=excluded.installed_at, updated_at=excluded.updated_at,
                 uninstalled_at=NULL
             WHERE extension_installations.uninstalled_at IS NOT NULL",
        )
        .bind(&id)
        .bind(scope)
        .bind(manifest_id)
        .bind(version)
        .bind(policy.as_str())
        .bind(&now)
        .bind(&now)
        .execute(db.postgres_pool().expect("postgres"))
        .await?
        .rows_affected(),
    };
    let active = active_installation(db, scope, manifest_id)
        .await?
        .ok_or_else(|| sqlx::Error::RowNotFound)?;
    Ok((
        if inserted == 1 {
            InstallOutcome::Installed
        } else {
            InstallOutcome::AlreadyInstalled
        },
        active,
    ))
}

/// The scope's active installation of one package, if any.
pub async fn active_installation(
    db: &Database,
    scope: &str,
    manifest_id: &str,
) -> Result<Option<Installation>, sqlx::Error> {
    let sql_sqlite = format!(
        "SELECT {INSTALL_COLUMNS} FROM extension_installations
         WHERE scope = ? AND manifest_id = ? AND uninstalled_at IS NULL"
    );
    let sql_pg = format!(
        "SELECT {INSTALL_COLUMNS} FROM extension_installations
         WHERE scope = $1 AND manifest_id = $2 AND uninstalled_at IS NULL"
    );
    match db.backend() {
        Backend::Sqlite => Ok(sqlx::query(&sql_sqlite)
            .bind(scope)
            .bind(manifest_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?
            .map(installation_from_sqlite)),
        Backend::Postgres => Ok(sqlx::query(&sql_pg)
            .bind(scope)
            .bind(manifest_id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
            .map(installation_from_pg)),
    }
}

/// Uninstall (deactivate) a scope's installation. `false` when there was nothing active — a second uninstall is not an error, it is the
/// same state reached twice.
pub async fn uninstall(db: &Database, scope: &str, manifest_id: &str) -> Result<bool, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "UPDATE extension_installations SET uninstalled_at = ?, updated_at = ?
             WHERE scope = ? AND manifest_id = ? AND uninstalled_at IS NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(scope)
        .bind(manifest_id)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await?
        .rows_affected(),
        Backend::Postgres => sqlx::query(
            "UPDATE extension_installations SET uninstalled_at = $1, updated_at = $2
             WHERE scope = $3 AND manifest_id = $4 AND uninstalled_at IS NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(scope)
        .bind(manifest_id)
        .execute(db.postgres_pool().expect("postgres"))
        .await?
        .rows_affected(),
    };
    Ok(affected > 0)
}

/// Move an active installation to `version`.
///
/// The caller has already decided, with `UpdatePolicy::newest_allowed` and the manifest's capabilities, that this move is allowed; this
/// function only records it. The version moves and nothing else does — reinstalling is not what an update is.
pub async fn set_version(
    db: &Database,
    scope: &str,
    manifest_id: &str,
    version: &str,
    policy: UpdatePolicy,
) -> Result<bool, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "UPDATE extension_installations SET version = ?, update_policy = ?, updated_at = ?
             WHERE scope = ? AND manifest_id = ? AND uninstalled_at IS NULL",
        )
        .bind(version)
        .bind(policy.as_str())
        .bind(&now)
        .bind(scope)
        .bind(manifest_id)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await?
        .rows_affected(),
        Backend::Postgres => sqlx::query(
            "UPDATE extension_installations SET version = $1, update_policy = $2, updated_at = $3
             WHERE scope = $4 AND manifest_id = $5 AND uninstalled_at IS NULL",
        )
        .bind(version)
        .bind(policy.as_str())
        .bind(&now)
        .bind(scope)
        .bind(manifest_id)
        .execute(db.postgres_pool().expect("postgres"))
        .await?
        .rows_affected(),
    };
    Ok(affected > 0)
}

/// A scope's own installations. The only installation read that names a scope, and it is the caller's own.
pub async fn my_installations(db: &Database, scope: &str) -> Result<Vec<Value>, sqlx::Error> {
    let sql_sqlite = format!(
        "SELECT {INSTALL_COLUMNS}, installed_at, updated_at FROM extension_installations
         WHERE scope = ? AND uninstalled_at IS NULL ORDER BY installed_at DESC"
    );
    let sql_pg = format!(
        "SELECT {INSTALL_COLUMNS}, installed_at, updated_at FROM extension_installations
         WHERE scope = $1 AND uninstalled_at IS NULL ORDER BY installed_at DESC"
    );
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&sql_sqlite)
                .bind(scope)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows.iter().map(installation_json_sqlite).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(&sql_pg)
                .bind(scope)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows.iter().map(installation_json_pg).collect())
        }
    }
}

/// How many scopes have this package installed right now.
///
/// A census, not a total: `uninstalled_at IS NULL`. The number a package's page shows is "installed by N readers", which is a statement
/// about the present and can go down.
pub async fn install_count(db: &Database, manifest_id: &str) -> Result<u64, sqlx::Error> {
    let count: i64 = match db.backend() {
        Backend::Sqlite => sqlx::query_scalar(
            "SELECT COUNT(*) FROM extension_installations WHERE manifest_id = ? AND uninstalled_at IS NULL",
        )
        .bind(manifest_id)
        .fetch_one(db.sqlite_pool().expect("sqlite"))
        .await?,
        Backend::Postgres => sqlx::query_scalar(
            "SELECT COUNT(*) FROM extension_installations WHERE manifest_id = $1 AND uninstalled_at IS NULL",
        )
        .bind(manifest_id)
        .fetch_one(db.postgres_pool().expect("postgres"))
        .await?,
    };
    Ok(u64::try_from(count).unwrap_or(0))
}

/// Every version of a package that readers may install, newest first.
pub async fn published_versions(db: &Database, manifest_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let versions: Vec<String> = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "SELECT version FROM extension_manifests WHERE id = ? AND state = 'approved'
             ORDER BY created_at DESC",
        )
        .bind(manifest_id)
        .fetch_all(db.sqlite_pool().expect("sqlite"))
        .await?
        .iter()
        .map(|row| row.get::<String, _>("version"))
        .collect(),
        Backend::Postgres => sqlx::query(
            "SELECT version FROM extension_manifests WHERE id = $1 AND state = 'approved'
             ORDER BY created_at DESC",
        )
        .bind(manifest_id)
        .fetch_all(db.postgres_pool().expect("postgres"))
        .await?
        .iter()
        .map(|row| row.get::<String, _>("version"))
        .collect(),
    };
    Ok(versions)
}

/// Record or replace a scope's rating of a package.
///
/// One row per (scope, package): re-rating replaces the opinion and keeps `created_at`, so a package page can say how long a review has
/// stood. The version being rated is stored because a review of 1.0.0 must not be read as a review of 2.0.0.
pub async fn rate(
    db: &Database,
    scope: &str,
    manifest_id: &str,
    version_rated: &str,
    stars: i64,
    review: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO extension_ratings
                     (scope, manifest_id, version_rated, stars, review, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(scope, manifest_id) DO UPDATE SET
                     version_rated=excluded.version_rated, stars=excluded.stars,
                     review=excluded.review, updated_at=excluded.updated_at",
            )
            .bind(scope)
            .bind(manifest_id)
            .bind(version_rated)
            .bind(stars)
            .bind(review)
            .bind(&now)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO extension_ratings
                     (scope, manifest_id, version_rated, stars, review, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT(scope, manifest_id) DO UPDATE SET
                     version_rated=excluded.version_rated, stars=excluded.stars,
                     review=excluded.review, updated_at=excluded.updated_at",
            )
            .bind(scope)
            .bind(manifest_id)
            .bind(version_rated)
            .bind(stars)
            .bind(review)
            .bind(&now)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(())
}

/// A package's aggregate score.
pub async fn score(db: &Database, manifest_id: &str) -> Result<Score, sqlx::Error> {
    let (total, rated): (i64, i64) = match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(
                "SELECT COALESCE(SUM(stars), 0) AS total, COUNT(*) AS rated
                 FROM extension_ratings WHERE manifest_id = ?",
            )
            .bind(manifest_id)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await?;
            (row.get("total"), row.get("rated"))
        }
        Backend::Postgres => {
            let row = sqlx::query(
                "SELECT COALESCE(SUM(stars), 0)::BIGINT AS total, COUNT(*) AS rated
                 FROM extension_ratings WHERE manifest_id = $1",
            )
            .bind(manifest_id)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await?;
            (row.get("total"), row.get("rated"))
        }
    };
    Ok(Score::from_sum(total, u64::try_from(rated).unwrap_or(0)))
}

/// Written reviews, newest first, with the reviewer's public handle.
///
/// The handle is published because a review is attributed speech; the scope is not published directly, only the name the reader chose to
/// read under. A rating with no text is not a review and is not listed here.
pub async fn reviews(
    db: &Database,
    manifest_id: &str,
    limit: i64,
) -> Result<Vec<Value>, sqlx::Error> {
    let sql_sqlite = "SELECT r.version_rated, r.stars, r.review, r.created_at, r.updated_at, p.handle
         FROM extension_ratings r
         LEFT JOIN pseuds p ON p.id = r.scope
         WHERE r.manifest_id = ? AND r.review IS NOT NULL AND trim(r.review) <> ''
         ORDER BY r.updated_at DESC LIMIT ?";
    // `pseuds.id` is a uuid on PostgreSQL and the scope is text; without the cast the join is `uuid = text`, which PostgreSQL refuses to
    // plan rather than coercing. `stars` is INT4 there and INT8 here, so it is cast widening: one decode type for both engines.
    let sql_pg = "SELECT r.version_rated, r.stars::BIGINT AS stars, r.review, r.created_at, r.updated_at, p.handle
         FROM extension_ratings r
         LEFT JOIN pseuds p ON p.id::text = r.scope
         WHERE r.manifest_id = $1 AND r.review IS NOT NULL AND trim(r.review) <> ''
         ORDER BY r.updated_at DESC LIMIT $2";
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(sql_sqlite)
                .bind(manifest_id)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows.iter().map(review_json_sqlite).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(sql_pg)
                .bind(manifest_id)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows.iter().map(review_json_pg).collect())
        }
    }
}

fn policy_from(text: &str) -> UpdatePolicy {
    UpdatePolicy::from_str(text).unwrap_or_default()
}

fn installation_from_sqlite(row: sqlx::sqlite::SqliteRow) -> Installation {
    Installation {
        manifest_id: row.get("manifest_id"),
        version: row.get("version"),
        policy: policy_from(&row.get::<String, _>("update_policy")),
        active: row.get::<Option<String>, _>("uninstalled_at").is_none(),
    }
}

fn installation_from_pg(row: sqlx::postgres::PgRow) -> Installation {
    Installation {
        manifest_id: row.get("manifest_id"),
        version: row.get("version"),
        policy: policy_from(&row.get::<String, _>("update_policy")),
        active: row.get::<Option<String>, _>("uninstalled_at").is_none(),
    }
}

fn installation_json_sqlite(row: &sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "manifest_id": row.get::<String, _>("manifest_id"),
        "version": row.get::<String, _>("version"),
        "update_policy": row.get::<String, _>("update_policy"),
        "installed_at": row.get::<String, _>("installed_at"),
        "updated_at": row.get::<String, _>("updated_at"),
    })
}

fn installation_json_pg(row: &sqlx::postgres::PgRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "manifest_id": row.get::<String, _>("manifest_id"),
        "version": row.get::<String, _>("version"),
        "update_policy": row.get::<String, _>("update_policy"),
        "installed_at": row.get::<String, _>("installed_at"),
        "updated_at": row.get::<String, _>("updated_at"),
    })
}

fn review_json_sqlite(row: &sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "version_rated": row.get::<String, _>("version_rated"),
        "stars": row.get::<i64, _>("stars"),
        "review": row.get::<Option<String>, _>("review"),
        "handle": row.get::<Option<String>, _>("handle"),
        "created_at": row.get::<String, _>("created_at"),
        "updated_at": row.get::<String, _>("updated_at"),
    })
}

fn review_json_pg(row: &sqlx::postgres::PgRow) -> Value {
    json!({
        "version_rated": row.get::<String, _>("version_rated"),
        "stars": row.get::<i64, _>("stars"),
        "review": row.get::<Option<String>, _>("review"),
        "handle": row.get::<Option<String>, _>("handle"),
        "created_at": row.get::<String, _>("created_at"),
        "updated_at": row.get::<String, _>("updated_at"),
    })
}

//! §11.11a — the succession plan and the redirect registry (M45-39).
//!
//! One row for the plan: an instance has one succession, and a second row would be a second opinion
//! nobody can act on. The redirect registry is keyed on `(from_host, from_path)` and resolves by the
//! **longest matching prefix**, so `/works` can point at an archive while `/works/1234` points at one
//! rescued work.

use anyhow::Result;

use darkhold_domain::succession::{normalise_host, normalise_path, path_matches, Plan, PlanDraft};

use crate::{Backend, Database};

/// The singleton row's id.
const PLAN_ID: &str = "instance";

/// The plan this instance publishes, if it has one.
pub async fn plan(db: &Database) -> Result<Option<Plan>> {
    let sql = db.sql(
        "SELECT destination, export_url, contact, review_days, confirmed_at
           FROM succession_plan WHERE id = ?",
        "SELECT destination, export_url, contact, review_days, confirmed_at
           FROM succession_plan WHERE id = ?",
    );
    let row: Option<(String, String, String, i64, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(PLAN_ID)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(PLAN_ID)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(
        |(destination, export_url, contact, review_days, confirmed_at)| Plan {
            destination,
            export_url,
            contact,
            review_days,
            confirmed_at,
        },
    ))
}

/// Set the plan, stamping it as confirmed now.
///
/// Setting and confirming are the same act: an operator who changes the destination has just told the
/// instance they are still alive. `confirm` exists for the case where nothing changed.
pub async fn set_plan(db: &Database, draft: &PlanDraft) -> Result<()> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO succession_plan
             (id, destination, export_url, contact, review_days, confirmed_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET
             destination = excluded.destination,
             export_url = excluded.export_url,
             contact = excluded.contact,
             review_days = excluded.review_days,
             confirmed_at = excluded.confirmed_at,
             updated_at = excluded.updated_at",
        "INSERT INTO succession_plan
             (id, destination, export_url, contact, review_days, confirmed_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET
             destination = excluded.destination,
             export_url = excluded.export_url,
             contact = excluded.contact,
             review_days = excluded.review_days,
             confirmed_at = excluded.confirmed_at,
             updated_at = excluded.updated_at",
    );
    // The query is built per arm: `sqlx::query` is typed by its backend, so one value cannot be
    // executed against both pools. This is the same shape `badges::award` uses.
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(PLAN_ID)
                .bind(&draft.destination)
                .bind(&draft.export_url)
                .bind(&draft.contact)
                .bind(draft.review_days)
                .bind(&now)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(PLAN_ID)
                .bind(&draft.destination)
                .bind(&draft.export_url)
                .bind(&draft.contact)
                .bind(draft.review_days)
                .bind(&now)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Re-confirm an existing plan without changing it.
///
/// Returns `false` when there is no plan to confirm — a confirmation of nothing is not a plan, and
/// silently creating one would publish a blank document.
pub async fn confirm(db: &Database, review_days: Option<i64>) -> Result<bool> {
    let now = crate::identity::now_rfc3339();
    let sql = match review_days {
        Some(_) => db.sql(
            "UPDATE succession_plan SET confirmed_at = ?, updated_at = ?, review_days = ? WHERE id = ?",
            "UPDATE succession_plan SET confirmed_at = ?, updated_at = ?, review_days = ? WHERE id = ?",
        ),
        None => db.sql(
            "UPDATE succession_plan SET confirmed_at = ?, updated_at = ? WHERE id = ?",
            "UPDATE succession_plan SET confirmed_at = ?, updated_at = ? WHERE id = ?",
        ),
    };
    let affected = match db.backend() {
        Backend::Sqlite => {
            let pool = db.sqlite_pool().expect("sqlite handle");
            let mut query = sqlx::query(&sql).bind(&now).bind(&now);
            if let Some(days) = review_days {
                query = query.bind(days);
            }
            query.bind(PLAN_ID).execute(pool).await?.rows_affected()
        }
        Backend::Postgres => {
            let pool = db.postgres_pool().expect("postgres handle");
            let mut query = sqlx::query(&sql).bind(&now).bind(&now);
            if let Some(days) = review_days {
                query = query.bind(days);
            }
            query.bind(PLAN_ID).execute(pool).await?.rows_affected()
        }
    };
    Ok(affected > 0)
}

/// Withdraw the plan.
pub async fn clear(db: &Database) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM succession_plan WHERE id = ?",
        "DELETE FROM succession_plan WHERE id = ?",
    );
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(PLAN_ID)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?
                .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(PLAN_ID)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?
                .rows_affected()
        }
    };
    Ok(affected > 0)
}

/// Register (or re-point) one redirect.
///
/// Re-pointing is an update rather than a conflict: an operator who has moved an archive once will
/// move it again, and refusing the second move leaves a registry that sends readers to a dead URL it
/// knows is dead.
pub async fn add_redirect(db: &Database, host: &str, path: &str, to_url: &str) -> Result<bool> {
    let Some(host) = normalise_host(host) else {
        return Ok(false);
    };
    let Some(path) = normalise_path(path) else {
        return Ok(false);
    };
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO succession_redirect (id, from_host, from_path, to_url, created_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (from_host, from_path) DO UPDATE SET to_url = excluded.to_url",
        "INSERT INTO succession_redirect (id, from_host, from_path, to_url, created_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (from_host, from_path) DO UPDATE SET to_url = excluded.to_url",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&host)
                .bind(&path)
                .bind(to_url)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&host)
                .bind(&path)
                .bind(to_url)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(true)
}

/// Resolve a path on a host, by the longest matching prefix.
///
/// The longest-prefix rule is what lets a specific rescue override a host-wide pointer. It is applied
/// here rather than in SQL because both engines sort prefixes differently, and a tie in the database
/// would be resolved by row order — which is not a rule.
pub async fn resolve(db: &Database, host: &str, path: &str) -> Result<Option<String>> {
    let Some(host) = normalise_host(host) else {
        return Ok(None);
    };
    let path = normalise_path(path).unwrap_or_else(|| "/".to_owned());
    let sql = db.sql(
        "SELECT from_path, to_url FROM succession_redirect WHERE from_host = ?",
        "SELECT from_path, to_url FROM succession_redirect WHERE from_host = ?",
    );
    let rows: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(&host)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(&host)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .filter(|(prefix, _)| path_matches(prefix, &path))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, to_url)| to_url))
}

/// Every redirect on a host, longest prefix first — the operator's view of the registry.
pub async fn redirects_for(db: &Database, host: &str) -> Result<Vec<(String, String)>> {
    let Some(host) = normalise_host(host) else {
        return Ok(Vec::new());
    };
    let sql = db.sql(
        "SELECT from_path, to_url FROM succession_redirect WHERE from_host = ?",
        "SELECT from_path, to_url FROM succession_redirect WHERE from_host = ?",
    );
    let rows: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(&host)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(&host)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let mut rows = rows;
    rows.sort_by_key(|(prefix, _)| std::cmp::Reverse(prefix.len()));
    Ok(rows)
}

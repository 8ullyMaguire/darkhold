//! M45-50 — a taste vector that applies for a window, then stops.
//!
//! The instance's own theme (`instance_theme`) is what this instance likes; a schedule is what it
//! likes **for a season**. Keeping the two apart is the whole point: ending an event must not require
//! reconstructing the instance's own vector from memory or recomputing it from bookmarks that have
//! moved on in the meantime.
//!
//! The window rule lives in `darkhold_domain::theme_window` (half-open, offset required), so the
//! refusal an operator reads is the same one the store enforces.

use crate::{Backend, Database};
use anyhow::Result;
use serde_json::Value as JsonValue;

/// A row as the store reads it: `(id, instance_id, label, theme_vector, starts_at, ends_at,
/// created_at, revoked_at)`.
type ScheduleRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

/// One scheduled gravity window.
#[derive(Debug, Clone)]
pub struct ThemeSchedule {
    pub id: String,
    pub instance_id: String,
    pub label: String,
    pub theme_vector: JsonValue,
    pub starts_at: String,
    pub ends_at: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

impl ThemeSchedule {
    /// Whether this row is live at `at` — not revoked and inside its window.
    #[must_use]
    pub fn covers(&self, at: &str) -> bool {
        self.revoked_at.is_none()
            && darkhold_domain::theme_window::Window {
                starts_at: self.starts_at.clone(),
                ends_at: self.ends_at.clone(),
            }
            .covers(at)
    }
}

/// Store a window. The caller has already validated it with
/// [`darkhold_domain::theme_window::parse_window`]; this refuses a backwards one anyway, because a
/// store that trusts its callers is a store whose rules hold only by luck.
pub async fn create(
    db: &Database,
    instance_id: &str,
    label: &str,
    theme_vector: &JsonValue,
    starts_at: &str,
    ends_at: &str,
    now: &str,
) -> Result<String> {
    let window = darkhold_domain::theme_window::parse_window(starts_at, ends_at)
        .map_err(|error| anyhow::anyhow!(error))?;
    let id = uuid::Uuid::new_v4().to_string();
    let vector = serde_json::to_string(theme_vector)?;
    let sql = "INSERT INTO theme_schedules (id, instance_id, label, theme_vector, starts_at, ends_at, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?)";
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(sql)
                .bind(&id)
                .bind(instance_id)
                .bind(label)
                .bind(&vector)
                .bind(&window.starts_at)
                .bind(&window.ends_at)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(sql)
                .bind(&id)
                .bind(instance_id)
                .bind(label)
                .bind(&vector)
                .bind(&window.starts_at)
                .bind(&window.ends_at)
                .bind(now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(id)
}

/// Every window this instance has ever written, newest start first — including revoked ones, because
/// "the event that was called off" is a fact an operator may need to read back.
pub async fn list_for_instance(db: &Database, instance_id: &str) -> Result<Vec<ThemeSchedule>> {
    let sql =
        "SELECT id, instance_id, label, theme_vector, starts_at, ends_at, created_at, revoked_at
               FROM theme_schedules
               WHERE instance_id = ?
               ORDER BY starts_at DESC, created_at DESC";
    let rows: Vec<ScheduleRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(instance_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(instance_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(row_to_schedule).collect())
}

/// The window that applies at `at`, if any.
///
/// When several cover the instant, the one that **started last** wins: a Christmas event that
/// overlaps a generic "winter" window is the more specific intent, and "last written wins" would make
/// the answer depend on the order an operator happened to type.
pub async fn active_at(
    db: &Database,
    instance_id: &str,
    at: &str,
) -> Result<Option<ThemeSchedule>> {
    let sql =
        "SELECT id, instance_id, label, theme_vector, starts_at, ends_at, created_at, revoked_at
               FROM theme_schedules
               WHERE instance_id = ?
                 AND starts_at <= ?
                 AND revoked_at IS NULL
                 AND ends_at > ?
               ORDER BY starts_at DESC
               LIMIT 1";
    let row: Option<ScheduleRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(instance_id)
                .bind(at)
                .bind(at)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(instance_id)
                .bind(at)
                .bind(at)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(row_to_schedule))
}

/// Call a window off. Returns whether a live row was revoked — a second revoke is not an error, it is
/// a no-op that says so.
pub async fn revoke(db: &Database, instance_id: &str, id: &str, now: &str) -> Result<bool> {
    let sql = "UPDATE theme_schedules SET revoked_at = ?
               WHERE id = ? AND instance_id = ? AND revoked_at IS NULL";
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(sql)
            .bind(now)
            .bind(id)
            .bind(instance_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(sql)
            .bind(now)
            .bind(id)
            .bind(instance_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

fn row_to_schedule(row: ScheduleRow) -> ThemeSchedule {
    let (id, instance_id, label, vector, starts_at, ends_at, created_at, revoked_at) = row;
    ThemeSchedule {
        id,
        instance_id,
        label,
        // A vector that will not parse is an empty one, not an error: the row is a document whose
        // shape this build may not know, and refusing to list it would hide the window entirely.
        theme_vector: serde_json::from_str(&vector).unwrap_or(JsonValue::Null),
        starts_at,
        ends_at,
        created_at,
        revoked_at,
    }
}

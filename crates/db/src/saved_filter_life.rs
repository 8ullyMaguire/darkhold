//! M69-21 — the store for a Saved Filter's public life: keybinds, subscriptions, blends.
//!
//! Three relations, each with one rule that belongs in the database rather than in a route:
//!
//! * a **keybind** is unique per account (a second filter on one digit is a filter the reader cannot reach);
//! * a **subscription** is one row per (reader, view), so following twice is following once;
//! * a **blend** cannot have the same view on both sides, and its name is unique per account.
//!
//! None of the three touches `saved_views.query_json`: a keybind is a column, and a subscription and a blend are relations to
//! views that already exist and have already been validated by their own writer.

use anyhow::Result;
use sqlx::Row as _;

use crate::{Backend, Database};

/// One filter a reader follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Followed {
    /// The view.
    pub view_id: String,
    /// Its name, as its owner wrote it.
    pub name: String,
    /// Its slug, so a reader can open it.
    pub slug: Option<String>,
    /// The account that owns it.
    pub owner_account_id: String,
    /// When this reader started following it.
    pub since: String,
}

/// Two filters answering one question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blend {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The first filter.
    pub left_view_id: String,
    /// The second.
    pub right_view_id: String,
    /// `interleave` or `union`.
    pub mode: String,
    /// Whether it is pinned beside the sidebar's views.
    pub pinned: bool,
    /// When it was created.
    pub created_at: String,
}

/// Bind a saved filter to a digit, or clear the binding with `None`.
///
/// Returns `Ok(false)` when the view does not belong to this account — a 404 at the door, because "somebody else's filter is
/// bound to 3" is not information this reader may have. A collision on the digit is an error with the digit in it: the reader
/// has to choose which filter keeps the key, and the refusal is what makes that a choice rather than a silent swap.
pub async fn set_keybind(
    db: &Database,
    account_id: &str,
    view_id: &str,
    keybind: Option<i64>,
    now: &str,
) -> Result<bool> {
    if let Some(bind) = keybind {
        if !darkhold_domain::saved_filter::keybind_is_valid(bind) {
            anyhow::bail!("a keybind is one digit from 1 to 9, not {bind}");
        }
        let holder_sql = db.sql(
            "SELECT id FROM saved_views WHERE account_id = ? AND keybind = ?",
            "SELECT id::text FROM saved_views WHERE account_id = $1::uuid AND keybind = $2::bigint",
        );
        let taken: Option<String> = match db.backend() {
            Backend::Sqlite => {
                sqlx::query_scalar(&holder_sql)
                    .bind(account_id)
                    .bind(bind)
                    .fetch_optional(db.sqlite_pool().expect("sqlite"))
                    .await?
            }
            Backend::Postgres => {
                sqlx::query_scalar(&holder_sql)
                    .bind(account_id)
                    .bind(bind)
                    .fetch_optional(db.postgres_pool().expect("postgres"))
                    .await?
            }
        };
        if let Some(taken) = taken {
            if taken != view_id {
                anyhow::bail!(
                    "digit {bind} is already bound to another of your filters; clear it first or choose another digit"
                );
            }
        }
    }
    let sql = db.sql(
        "UPDATE saved_views SET keybind = ?, updated_at = ? WHERE id = ? AND account_id = ?",
        "UPDATE saved_views SET keybind = $1, updated_at = $2 WHERE id = $3::uuid AND account_id = $4::uuid",
    );
    let changed = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(keybind)
            .bind(now)
            .bind(view_id)
            .bind(account_id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(keybind)
            .bind(now)
            .bind(view_id)
            .bind(account_id)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected(),
    };
    Ok(changed > 0)
}

/// Every keybinding this account has, ordered by digit.
pub async fn keybinds(db: &Database, account_id: &str) -> Result<Vec<(i64, String, String)>> {
    let sql = db.sql(
        "SELECT keybind, id, name FROM saved_views WHERE account_id = ? AND keybind IS NOT NULL ORDER BY keybind",
        "SELECT CAST(keybind AS BIGINT) AS keybind, id::text, name FROM saved_views WHERE account_id = $1::uuid AND keybind IS NOT NULL ORDER BY keybind",
    );
    let rows: Vec<(i64, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows)
}

/// Follow a public filter. Returns `Ok(false)` when it is already followed (following twice is following once).
///
/// A private filter cannot be followed: the caller must have checked visibility, and this function refuses anyway, because a
/// subscription to a private view would let its owner's next edit publish into a stranger's feed.
pub async fn subscribe(
    db: &Database,
    subscriber_account_id: &str,
    view_id: &str,
    now: &str,
) -> Result<bool> {
    let is_public: Option<i64> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar("SELECT is_public FROM saved_views WHERE id = ?")
                .bind(view_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(
                "SELECT CAST(CASE WHEN is_public THEN 1 ELSE 0 END AS BIGINT) FROM saved_views WHERE id = $1::uuid",
            )
                .bind(view_id)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    match is_public {
        None => anyhow::bail!("no such filter"),
        Some(0) => anyhow::bail!("that filter is private; only a public filter can be followed"),
        Some(_) => {}
    }
    let sql = db.sql(
        "INSERT INTO saved_view_subscriptions (subscriber_account_id, view_id, created_at)
         VALUES (?, ?, ?) ON CONFLICT (subscriber_account_id, view_id) DO NOTHING",
        "INSERT INTO saved_view_subscriptions (subscriber_account_id, view_id, created_at)
         VALUES ($1::uuid, $2::uuid, $3) ON CONFLICT (subscriber_account_id, view_id) DO NOTHING",
    );
    let changed = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(subscriber_account_id)
            .bind(view_id)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(subscriber_account_id)
            .bind(view_id)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected(),
    };
    Ok(changed > 0)
}

/// Stop following a filter.
pub async fn unsubscribe(db: &Database, subscriber: &str, view_id: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM saved_view_subscriptions WHERE subscriber_account_id = ? AND view_id = ?",
        "DELETE FROM saved_view_subscriptions WHERE subscriber_account_id = $1::uuid AND view_id = $2::uuid",
    );
    let changed = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(subscriber)
            .bind(view_id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(subscriber)
            .bind(view_id)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected(),
    };
    Ok(changed > 0)
}

/// The filters this reader follows, newest first.
pub async fn followed(db: &Database, subscriber: &str) -> Result<Vec<Followed>> {
    let sql = db.sql(
        "SELECT s.view_id, v.name, v.slug, v.account_id, s.created_at \
           FROM saved_view_subscriptions s \
           JOIN saved_views v ON v.id = s.view_id \
          WHERE s.subscriber_account_id = ? \
          ORDER BY s.created_at DESC",
        "SELECT s.view_id::text AS view_id, v.name, v.slug, v.account_id::text, s.created_at \
           FROM saved_view_subscriptions s \
           JOIN saved_views v ON v.id = s.view_id \
          WHERE s.subscriber_account_id = $1::uuid \
          ORDER BY s.created_at DESC",
    );
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&sql)
                .bind(subscriber)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows.iter().map(followed_from_sqlite).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(&sql)
                .bind(subscriber)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows.iter().map(followed_from_postgres).collect())
        }
    }
}

fn followed_from_sqlite(row: &sqlx::sqlite::SqliteRow) -> Followed {
    Followed {
        view_id: row.get("view_id"),
        name: row.get("name"),
        slug: row.get("slug"),
        owner_account_id: row.get("account_id"),
        since: row.get("created_at"),
    }
}

fn followed_from_postgres(row: &sqlx::postgres::PgRow) -> Followed {
    Followed {
        view_id: row.get("view_id"),
        name: row.get("name"),
        slug: row.get("slug"),
        owner_account_id: row.get("account_id"),
        since: row.get("created_at"),
    }
}

/// How many readers follow one filter.
pub async fn subscriber_count(db: &Database, view_id: &str) -> Result<i64> {
    let sql = db.sql(
        "SELECT CAST(COUNT(*) AS BIGINT) FROM saved_view_subscriptions WHERE view_id = ?",
        "SELECT CAST(COUNT(*) AS BIGINT) FROM saved_view_subscriptions WHERE view_id = $1::uuid",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(view_id)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(view_id)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(count)
}

/// What a caller asks a blend to be.
///
/// A struct rather than six positional parameters because two of them are view ids and swapping them would compile and produce a
/// blend pointing at the wrong filters — the defect that a named struct makes impossible to write.
#[derive(Debug, Clone, Copy)]
pub struct NewBlend<'a> {
    /// Its name.
    pub name: &'a str,
    /// The first filter.
    pub left_view_id: &'a str,
    /// The second.
    pub right_view_id: &'a str,
    /// `interleave` or `union`.
    pub mode: &'a str,
    /// Whether it is pinned beside the sidebar's views.
    pub pinned: bool,
}

/// Create a blend of two of this account's filters.
///
/// Both sides must belong to the caller: a blend is a reader's own arrangement of their own (or followed) filters, and letting
/// it name somebody else's private view would be a read of that view through the blend's door.
pub async fn create_blend(
    db: &Database,
    account_id: &str,
    request: NewBlend<'_>,
    now: &str,
) -> Result<Blend> {
    let NewBlend {
        name,
        left_view_id,
        right_view_id,
        mode,
        pinned,
    } = request;
    if name.trim().is_empty() {
        anyhow::bail!("a blend needs a name");
    }
    darkhold_domain::saved_filter::BlendMode::parse(mode).map_err(anyhow::Error::msg)?;
    if left_view_id == right_view_id {
        anyhow::bail!("a blend of a filter with itself is that filter; pick two different ones");
    }
    for view in [left_view_id, right_view_id] {
        let owned: Option<String> = match db.backend() {
            Backend::Sqlite => {
                sqlx::query_scalar("SELECT id FROM saved_views WHERE id = ? AND account_id = ?")
                    .bind(view)
                    .bind(account_id)
                    .fetch_optional(db.sqlite_pool().expect("sqlite"))
                    .await?
            }
            Backend::Postgres => sqlx::query_scalar(
                "SELECT id::text FROM saved_views WHERE id = $1::uuid AND account_id = $2::uuid",
            )
            .bind(view)
            .bind(account_id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?,
        };
        if owned.is_none() {
            anyhow::bail!("a blend may only name your own filters");
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let sql = db.sql(
        "INSERT INTO saved_view_blends (id, account_id, name, left_view_id, right_view_id, mode, pinned, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        "INSERT INTO saved_view_blends (id, account_id, name, left_view_id, right_view_id, mode, pinned, created_at, updated_at)
         VALUES ($1::uuid, $2::uuid, $3, $4::uuid, $5::uuid, $6, $7, $8, $9)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(account_id)
                .bind(name.trim())
                .bind(left_view_id)
                .bind(right_view_id)
                .bind(mode)
                .bind(i64::from(pinned))
                .bind(now)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(account_id)
                .bind(name.trim())
                .bind(left_view_id)
                .bind(right_view_id)
                .bind(mode)
                .bind(pinned)
                .bind(now)
                .bind(now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    find_blend(db, account_id, &id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the blend was written and cannot be read back"))
}

fn blend_from_sqlite(row: &sqlx::sqlite::SqliteRow) -> Blend {
    Blend {
        id: row.get("id"),
        name: row.get("name"),
        left_view_id: row.get("left_view_id"),
        right_view_id: row.get("right_view_id"),
        mode: row.get("mode"),
        pinned: row.get::<i64, _>("pinned") != 0,
        created_at: row.get("created_at"),
    }
}

fn blend_from_postgres(row: &sqlx::postgres::PgRow) -> Blend {
    Blend {
        id: row.get("id"),
        name: row.get("name"),
        left_view_id: row.get("left_view_id"),
        right_view_id: row.get("right_view_id"),
        mode: row.get("mode"),
        pinned: row.get("pinned"),
        created_at: row.get("created_at"),
    }
}

const BLEND_COLUMNS_SQLITE: &str =
    "id, name, left_view_id, right_view_id, mode, pinned, created_at";

/// One blend, if it belongs to this account.
pub async fn find_blend(db: &Database, account_id: &str, id: &str) -> Result<Option<Blend>> {
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&format!(
                "SELECT {BLEND_COLUMNS_SQLITE} FROM saved_view_blends WHERE id = ? AND account_id = ?"
            ))
            .bind(id)
            .bind(account_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.as_ref().map(blend_from_sqlite))
        }
        Backend::Postgres => {
            let row = sqlx::query(
                "SELECT id::text AS id, name, left_view_id::text AS left_view_id, \
                        right_view_id::text AS right_view_id, mode, pinned, created_at \
                   FROM saved_view_blends WHERE id = $1::uuid AND account_id = $2::uuid",
            )
            .bind(id)
            .bind(account_id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.as_ref().map(blend_from_postgres))
        }
    }
}

/// This account's blends, pinned first, then by name.
pub async fn blends(db: &Database, account_id: &str) -> Result<Vec<Blend>> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&format!(
                "SELECT {BLEND_COLUMNS_SQLITE} FROM saved_view_blends WHERE account_id = ? \
                 ORDER BY pinned DESC, name, id"
            ))
            .bind(account_id)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows.iter().map(blend_from_sqlite).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT id::text AS id, name, left_view_id::text AS left_view_id, \
                        right_view_id::text AS right_view_id, mode, pinned, created_at \
                   FROM saved_view_blends WHERE account_id = $1::uuid \
                  ORDER BY pinned DESC, name, id",
            )
            .bind(account_id)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows.iter().map(blend_from_postgres).collect())
        }
    }
}

/// Delete a blend. Returns whether anything was deleted.
pub async fn delete_blend(db: &Database, account_id: &str, id: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM saved_view_blends WHERE id = ? AND account_id = ?",
        "DELETE FROM saved_view_blends WHERE id = $1::uuid AND account_id = $2::uuid",
    );
    let changed = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(id)
            .bind(account_id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(id)
            .bind(account_id)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected(),
    };
    Ok(changed > 0)
}

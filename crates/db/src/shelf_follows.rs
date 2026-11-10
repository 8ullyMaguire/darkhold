//! Following a shelf (M64-11).
//!
//! A shelf is a list, and following one is a subscription: the follower wants to see what gets added to it. Three rules, and
//! each one is a refusal rather than a silent no-op:
//!
//! * **you cannot follow a shelf that is not public.** You could not see it; following it would be a subscription to a
//!   document you are not allowed to read, and the first item added would leak the list's existence.
//! * **you cannot follow your own shelf.** It is already yours; the row would make "who follows this" count its owner.
//! * **following twice is one follow.** The route is idempotent because a client retrying a POST is not a second reader.
//!
//! A shelf made private **after** it was followed keeps its follower rows and disappears from every follower's read
//! (`followed_shelves` joins `is_public`). Deleting the rows instead would silently unsubscribe people for a decision their
//! own owner made; keeping the row and hiding the contents is the reversible one.

use crate::{identity::now_rfc3339, Backend, Database, Result};
use serde::Serialize;

/// One shelf a reader follows, as the follower sees it.
#[derive(Debug, Clone, Serialize)]
pub struct FollowedShelf {
    /// Shelf identifier.
    pub shelf_id: String,
    /// The owning account, so a follower can tell whose list this is without a second lookup per row.
    pub account_id: String,
    /// Display name.
    pub name: String,
    /// Owner handle, when the owning account still has a pseud.
    pub owner_handle: Option<String>,
    /// How many items the shelf holds.
    pub item_count: i64,
    /// When this reader started following it, RFC 3339.
    pub followed_at: String,
}

/// Why a follow was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowRefusal {
    /// No such shelf, or it belongs to somebody else and is private — deliberately the same answer.
    NotFound,
    /// The shelf is the caller's own.
    OwnShelf,
    /// The shelf exists and is the caller's to see, but is not published.
    NotPublic,
}

/// Follow a shelf. Idempotent: a second call succeeds and changes nothing.
pub async fn follow(
    db: &Database,
    account: &str,
    shelf_id: &str,
) -> Result<std::result::Result<(), FollowRefusal>> {
    let found: Option<(String, i64)> = {
        let sql = db.sql(
            "SELECT account_id, CAST(is_public AS INTEGER) FROM shelves WHERE id = ?1",
            "SELECT account_id::text, (is_public::int)::bigint FROM shelves WHERE id = $1",
        );
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query_as(&sql)
                    .bind(shelf_id)
                    .fetch_optional(db.sqlite_pool().expect("sqlite pool"))
                    .await?
            }
            Backend::Postgres => {
                sqlx::query_as(&sql)
                    .bind(shelf_id)
                    .fetch_optional(db.postgres_pool().expect("postgres pool"))
                    .await?
            }
        }
    };

    let Some((owner, is_public)) = found else {
        return Ok(Err(FollowRefusal::NotFound));
    };
    if owner == account {
        return Ok(Err(FollowRefusal::OwnShelf));
    }
    if is_public == 0 {
        return Ok(Err(FollowRefusal::NotPublic));
    }

    let sql = db.sql(
        "INSERT INTO shelf_follows (follower_account_id, shelf_id, created_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT (follower_account_id, shelf_id) DO NOTHING",
        "INSERT INTO shelf_follows (follower_account_id, shelf_id, created_at)
         VALUES ($1::uuid, $2::uuid, $3::timestamptz)
         ON CONFLICT (follower_account_id, shelf_id) DO NOTHING",
    );
    let now = now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(account)
                .bind(shelf_id)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite pool"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(account)
                .bind(shelf_id)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres pool"))
                .await?;
        }
    }
    Ok(Ok(()))
}

/// Stop following a shelf. Returns whether a row was removed, so the caller can tell "unfollowed" from "was not following".
pub async fn unfollow(db: &Database, account: &str, shelf_id: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM shelf_follows WHERE follower_account_id = ?1 AND shelf_id = ?2",
        "DELETE FROM shelf_follows WHERE follower_account_id = $1 AND shelf_id = $2",
    );
    let removed = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(account)
            .bind(shelf_id)
            .execute(db.sqlite_pool().expect("sqlite pool"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(account)
            .bind(shelf_id)
            .execute(db.postgres_pool().expect("postgres pool"))
            .await?
            .rows_affected(),
    };
    Ok(removed > 0)
}

/// The shelves this account follows, published ones only, newest follow first.
///
/// `is_public` is in the predicate rather than in the caller: a shelf made private after it was followed must stop appearing
/// here, and that is a property of the read, not of any particular route.
pub async fn followed_shelves(db: &Database, account: &str) -> Result<Vec<FollowedShelf>> {
    let sql = db.sql(
        "SELECT CAST(sf.shelf_id AS TEXT), CAST(s.account_id AS TEXT), s.name,
                (SELECT p.handle FROM pseuds p WHERE p.account_id = s.account_id
                  ORDER BY p.created_at LIMIT 1),
                (SELECT CAST(COUNT(*) AS INTEGER) FROM shelf_items si WHERE si.shelf_id = s.id),
                sf.created_at
           FROM shelf_follows sf
           JOIN shelves s ON s.id = sf.shelf_id AND s.is_public = 1
          WHERE sf.follower_account_id = ?1
          ORDER BY sf.created_at DESC, sf.shelf_id",
        "SELECT sf.shelf_id::text, s.account_id::text, s.name,
                (SELECT p.handle FROM pseuds p WHERE p.account_id = s.account_id
                  ORDER BY p.created_at LIMIT 1),
                (SELECT COUNT(*)::bigint FROM shelf_items si WHERE si.shelf_id = s.id),
                sf.created_at
           FROM shelf_follows sf
           JOIN shelves s ON s.id = sf.shelf_id AND s.is_public = true
          WHERE sf.follower_account_id = $1
          ORDER BY sf.created_at DESC, sf.shelf_id",
    );
    let rows: Vec<(String, String, String, Option<String>, i64, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account)
                .fetch_all(db.sqlite_pool().expect("sqlite pool"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account)
                .fetch_all(db.postgres_pool().expect("postgres pool"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(shelf_id, account_id, name, owner_handle, item_count, followed_at)| FollowedShelf {
                shelf_id,
                account_id,
                name,
                owner_handle,
                item_count,
                followed_at,
            },
        )
        .collect())
}

/// Whether this account follows this shelf.
pub async fn follows(db: &Database, account: &str, shelf_id: &str) -> Result<bool> {
    let sql = db.sql(
        "SELECT COUNT(*) FROM shelf_follows WHERE follower_account_id = ?1 AND shelf_id = ?2",
        "SELECT COUNT(*) FROM shelf_follows WHERE follower_account_id = $1 AND shelf_id = $2",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(account)
                .bind(shelf_id)
                .fetch_one(db.sqlite_pool().expect("sqlite pool"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(account)
                .bind(shelf_id)
                .fetch_one(db.postgres_pool().expect("postgres pool"))
                .await?
        }
    };
    Ok(count > 0)
}

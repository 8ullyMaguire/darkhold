//! The reader's library, as stored: shelves, bookmarks, private tags, reading
//! statuses, saved views, update checks and the storage they occupy (spec §16
//! as the plan numbers it; §14 in the spec text).
//!
//! Conventions are the same as every other module in this crate: DML is written
//! once with `?` placeholders and the PostgreSQL half carries its casts, reads
//! alias native UUID columns to text, integers the domain decodes as `i64` are
//! `BIGINT`, and booleans are read through `::int::bigint` — because PostgreSQL
//! reaches bigint from boolean only via `int`. See ADR 0004.
//!
//! # Three rules this module enforces rather than documents
//!
//! * **Ownership is part of every statement.** Not one function here takes an
//!   identifier on trust: a shelf is addressed `(account_id, shelf_id)`, a
//!   bookmark `(account_id, bookmark_id)`, and the shelf-item insert joins both
//!   the shelf and the library item against the account so a mismatched pair
//!   inserts nothing rather than succeeding. A check-then-write would be a race
//!   and a place to forget the check.
//! * **A private tag is never joined into anything another account can read.**
//!   Every tag query carries an `account_id` predicate of its own rather than
//!   relying on a caller's join to have supplied one.
//! * **This module never deletes a blob.** [`remove_library_items`] returns the
//!   checksums that *became* unreferenced and leaves the file to
//!   [`crate::storage::BlobStore::delete_if_unreferenced`], the same rule
//!   `crate::imports` follows.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use darkhold_domain::library::{
    BatchFailure, BatchOutcome, LibraryQuery, LibrarySort, ReadingStatus, ViewScope,
};

use crate::identity::now_rfc3339;
use crate::imports::{
    decode_library_item, LibraryItem, LibraryItemRow, LIBRARY_COLUMNS, LIBRARY_COLUMNS_PG,
    LIBRARY_UNIT_COUNT,
};
use crate::{rewrite_placeholders, sql_owned, Backend, Database};

/// Run one statement against whichever driver this handle speaks.
///
/// A macro rather than a function because `sqlx::query`'s type is parameterised
/// by the backend, so a `match` whose arms both end in `?` will not unify. The
/// body is expanded once per arm, which means the bind chain is written once in
/// the source and cannot drift between the dialects — the failure ADR 0004's
/// rule exists to prevent.
macro_rules! run {
    ($db:expr, $sql:expr, |$query:ident| $body:expr) => {
        async {
            match $db.backend() {
                Backend::Sqlite => {
                    let $query = sqlx::query(&$sql);
                    let affected = ($body)
                        .execute($db.sqlite_pool().expect("sqlite handle"))
                        .await?
                        .rows_affected();
                    Ok::<u64, sqlx::Error>(affected)
                }
                Backend::Postgres => {
                    let $query = sqlx::query(&$sql);
                    let affected = ($body)
                        .execute($db.postgres_pool().expect("postgres handle"))
                        .await?
                        .rows_affected();
                    Ok::<u64, sqlx::Error>(affected)
                }
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Shelves
// ---------------------------------------------------------------------------

/// A reader's shelf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shelf {
    /// Identifier.
    pub id: String,
    /// The owning account.
    pub account_id: String,
    /// Display name, unique within the account.
    pub name: String,
    /// Free text.
    pub description: String,
    /// Whether the shelf is visible to anyone but its owner.
    pub is_public: bool,
    /// Sort order in the sidebar.
    pub position: i64,
    /// When it was created, RFC 3339.
    pub created_at: String,
    /// When it was last changed, RFC 3339.
    pub updated_at: String,
    /// Optimistic-concurrency version.
    pub version: i64,
}

#[derive(FromRow)]
struct ShelfRow {
    id: String,
    account_id: String,
    name: String,
    description: String,
    is_public: i64,
    position: i64,
    created_at: String,
    updated_at: String,
    version: i64,
}

const SHELF_COLUMNS: &str = "id, account_id, name, description, is_public, position, \
     created_at, updated_at, version";

const SHELF_COLUMNS_PG: &str = "id::text AS id, account_id::text AS account_id, name, \
     description, is_public::int::bigint AS is_public, position, created_at, updated_at, version";

fn decode_shelf(row: ShelfRow) -> Shelf {
    Shelf {
        id: row.id,
        account_id: row.account_id,
        name: row.name,
        description: row.description,
        is_public: row.is_public != 0,
        position: row.position,
        created_at: row.created_at,
        updated_at: row.updated_at,
        version: row.version,
    }
}

/// Create a shelf.
///
/// The account's first shelf lands at position 0; later ones go after the last.
pub async fn create_shelf(
    db: &Database,
    account_id: &str,
    name: &str,
    description: &str,
    is_public: bool,
) -> Result<Shelf> {
    let now = now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let next: i64 =
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query_scalar(
                    "SELECT COALESCE(MAX(position), -10) + 10 FROM shelves \
                                WHERE account_id = ?",
                )
                .bind(account_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
            }
            // `$1`, not `?`: this call never goes through `db.sql`, so nothing
            // rewrites the placeholder, and PostgreSQL does not accept `?`.
            Backend::Postgres => sqlx::query_scalar(
                "SELECT COALESCE(MAX(position), -10) + 10 FROM shelves WHERE account_id = $1::uuid",
            )
            .bind(account_id)
            .fetch_one(db.postgres_pool().expect("postgres handle"))
            .await?,
        };
    let is_public_int: i64 = i64::from(is_public);
    let sql = db.sql(
        "INSERT INTO shelves (id, account_id, name, description, is_public, position, \
         created_at, updated_at, version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
        "INSERT INTO shelves (id, account_id, name, description, is_public, position, \
         created_at, updated_at, version) \
         VALUES (?::uuid, ?::uuid, ?, ?, ?::int::boolean, ?, ?, ?, 1)",
    );
    run!(db, sql, |q| q
        .bind(&id)
        .bind(account_id)
        .bind(name)
        .bind(description)
        .bind(is_public_int)
        .bind(next)
        .bind(&now)
        .bind(&now))
    .await?;

    Ok(Shelf {
        id,
        account_id: account_id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        is_public,
        position: next,
        created_at: now.clone(),
        updated_at: now,
        version: 1,
    })
}

/// Every shelf the account owns, in sidebar order.
pub async fn shelves_for(db: &Database, account_id: &str) -> Result<Vec<Shelf>> {
    let sql = sql_owned(
        db,
        format!(
            "SELECT {SHELF_COLUMNS} FROM shelves WHERE account_id = ? \
                 ORDER BY position ASC, name ASC"
        ),
        format!(
            "SELECT {SHELF_COLUMNS_PG} FROM shelves WHERE account_id::text = ? \
                 ORDER BY position ASC, name ASC"
        ),
    );
    let rows: Vec<ShelfRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(decode_shelf).collect())
}

/// One shelf, if this account owns it.
pub async fn find_shelf(db: &Database, account_id: &str, id: &str) -> Result<Option<Shelf>> {
    let sql = sql_owned(
        db,
        format!("SELECT {SHELF_COLUMNS} FROM shelves WHERE id = ? AND account_id = ?"),
        format!(
            "SELECT {SHELF_COLUMNS_PG} FROM shelves WHERE id::text = ? AND account_id::text = ?"
        ),
    );
    let row: Option<ShelfRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(id)
                .bind(account_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(id)
                .bind(account_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(decode_shelf))
}

/// A partial update. Absent fields are left alone.
#[derive(Debug, Clone, Default)]
pub struct ShelfPatch<'a> {
    /// New display name.
    pub name: Option<&'a str>,
    /// New description.
    pub description: Option<&'a str>,
    /// New visibility.
    pub is_public: Option<bool>,
    /// New sidebar position.
    pub position: Option<i64>,
}

/// Update a shelf.
///
/// Returns `true` when a row changed: `false` means the shelf does not exist,
/// is not this account's, or was changed by someone else since `expected_version`
/// — which the caller reports as a conflict rather than a not-found, because the
/// two are different answers.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn update_shelf(
    db: &Database,
    account_id: &str,
    id: &str,
    patch: &ShelfPatch<'_>,
    expected_version: i64,
) -> Result<bool> {
    let now = now_rfc3339();
    let is_public = patch.is_public.map(i64::from);
    let sql = db.sql(
        "UPDATE shelves SET name = COALESCE(?, name), description = COALESCE(?, description), \
         is_public = COALESCE(?, is_public), position = COALESCE(?, position), \
         updated_at = ?, version = version + 1 \
         WHERE id = ? AND account_id = ? AND version = ?",
        "UPDATE shelves SET name = COALESCE(?, name), description = COALESCE(?, description), \
         is_public = COALESCE(?::int::boolean, is_public), position = COALESCE(?, position), \
         updated_at = ?, version = version + 1 \
         WHERE id::text = ? AND account_id::text = ? AND version = ?",
    );
    let affected = run!(db, sql, |q| q
        .bind(patch.name)
        .bind(patch.description)
        .bind(is_public)
        .bind(patch.position)
        .bind(&now)
        .bind(id)
        .bind(account_id)
        .bind(expected_version))
    .await?;
    Ok(affected > 0)
}

/// Delete a shelf.
///
/// Its placements go with it; the library items on it do not (spec §14.1,
/// "Deleting a shelf does not delete its works").
pub async fn delete_shelf(db: &Database, account_id: &str, id: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM shelves WHERE id = ? AND account_id = ?",
        "DELETE FROM shelves WHERE id::text = ? AND account_id::text = ?",
    );
    let affected = run!(db, sql, |q| q.bind(id).bind(account_id)).await?;
    Ok(affected > 0)
}

// ---------------------------------------------------------------------------
// Shelf items
// ---------------------------------------------------------------------------

/// Place a library item on a shelf, or move it if it is already there.
///
/// Ownership of *both* ends is enforced by the statement: the row is built from
/// a join that requires the shelf and the item to belong to the same account, so
/// a caller naming someone else's shelf or someone else's item inserts nothing.
///
/// Returns whether a placement exists afterwards.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn add_shelf_item(
    db: &Database,
    account_id: &str,
    shelf_id: &str,
    library_item_id: &str,
    position: i64,
) -> Result<bool> {
    let now = now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let sql = db.sql(
        "INSERT INTO shelf_items (id, shelf_id, library_item_id, position, created_at) \
         SELECT ?, s.id, l.id, ?, ? \
           FROM shelves s, library_items l \
          WHERE s.id = ? AND s.account_id = ? AND l.id = ? AND l.account_id = ? \
         ON CONFLICT (shelf_id, library_item_id) DO UPDATE SET position = excluded.position",
        "INSERT INTO shelf_items (id, shelf_id, library_item_id, position, created_at) \
         SELECT ?::uuid, s.id, l.id, ?, ? \
           FROM shelves s, library_items l \
          WHERE s.id::text = ? AND s.account_id::text = ? AND l.id::text = ? \
            AND l.account_id::text = ? \
         ON CONFLICT (shelf_id, library_item_id) DO UPDATE SET position = excluded.position",
    );
    let affected = run!(db, sql, |q| q
        .bind(&id)
        .bind(position)
        .bind(&now)
        .bind(shelf_id)
        .bind(account_id)
        .bind(library_item_id)
        .bind(account_id))
    .await?;
    Ok(affected > 0)
}

/// Take a library item off a shelf.
///
/// The item itself is untouched.
pub async fn remove_shelf_item(
    db: &Database,
    account_id: &str,
    shelf_id: &str,
    library_item_id: &str,
) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM shelf_items WHERE shelf_id IN (SELECT id FROM shelves \
         WHERE id = ? AND account_id = ?) AND library_item_id = ?",
        "DELETE FROM shelf_items WHERE shelf_id IN (SELECT id FROM shelves \
         WHERE id::text = ? AND account_id::text = ?) AND library_item_id::text = ?",
    );
    let affected = run!(db, sql, |q| q
        .bind(shelf_id)
        .bind(account_id)
        .bind(library_item_id))
    .await?;
    Ok(affected > 0)
}

/// The library items on a shelf, in the shelf's own order.
pub async fn shelf_item_ids(
    db: &Database,
    account_id: &str,
    shelf_id: &str,
) -> Result<Vec<String>> {
    let sql = db.sql(
        "SELECT si.library_item_id FROM shelf_items si JOIN shelves s ON s.id = si.shelf_id \
         WHERE s.id = ? AND s.account_id = ? ORDER BY si.position ASC, si.created_at ASC",
        "SELECT si.library_item_id::text FROM shelf_items si JOIN shelves s ON s.id = si.shelf_id \
         WHERE s.id::text = ? AND s.account_id::text = ? \
         ORDER BY si.position ASC, si.created_at ASC",
    );
    let rows: Vec<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(shelf_id)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(shelf_id)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

/// Which of the account's shelves hold a given library item.
pub async fn shelves_holding(
    db: &Database,
    account_id: &str,
    library_item_id: &str,
) -> Result<Vec<String>> {
    let sql = db.sql(
        "SELECT s.name FROM shelf_items si JOIN shelves s ON s.id = si.shelf_id \
         WHERE s.account_id = ? AND si.library_item_id = ? ORDER BY s.position ASC",
        "SELECT s.name FROM shelf_items si JOIN shelves s ON s.id = si.shelf_id \
         WHERE s.account_id::text = ? AND si.library_item_id::text = ? ORDER BY s.position ASC",
    );
    let rows: Vec<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(library_item_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(library_item_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Bookmarks
// ---------------------------------------------------------------------------

/// A reader's bookmark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    /// Identifier.
    pub id: String,
    /// The owning account.
    pub account_id: String,
    /// `work` or `library_item`.
    pub subject_type: String,
    /// The bookmarked subject.
    pub subject_id: String,
    /// The unit, when the bookmark points into one.
    pub unit_id: Option<String>,
    /// How far into the unit, in permille.
    pub position_permille: Option<i64>,
    /// The reader's own note.
    pub note: String,
    /// Whether the bookmark may appear in a public list.
    pub is_public: bool,
    /// When it was created, RFC 3339.
    pub created_at: String,
    /// When it was last changed, RFC 3339.
    pub updated_at: String,
    /// Optimistic-concurrency version.
    pub version: i64,
}

#[derive(FromRow)]
struct BookmarkRow {
    id: String,
    account_id: String,
    subject_type: String,
    subject_id: String,
    unit_id: Option<String>,
    position_permille: Option<i64>,
    note: String,
    is_public: i64,
    created_at: String,
    updated_at: String,
    version: i64,
}

const BOOKMARK_COLUMNS: &str = "id, account_id, subject_type, subject_id, unit_id, \
     position_permille, note, is_public, created_at, updated_at, version";

const BOOKMARK_COLUMNS_PG: &str = "id::text AS id, account_id::text AS account_id, subject_type, \
     subject_id::text AS subject_id, unit_id::text AS unit_id, position_permille, note, \
     is_public::int::bigint AS is_public, created_at, updated_at, version";

fn decode_bookmark(row: BookmarkRow) -> Bookmark {
    Bookmark {
        id: row.id,
        account_id: row.account_id,
        subject_type: row.subject_type,
        subject_id: row.subject_id,
        unit_id: row.unit_id,
        position_permille: row.position_permille,
        note: row.note,
        is_public: row.is_public != 0,
        created_at: row.created_at,
        updated_at: row.updated_at,
        version: row.version,
    }
}

/// A bookmark about to be created.
///
/// A struct rather than eight positional arguments, because five of them are
/// strings and a caller who swaps `subject_id` and `unit_id` would compile
/// and then file the bookmark against the wrong row.
#[derive(Debug, Clone, Default)]
pub struct NewBookmark<'a> {
    /// `work` or `library_item`.
    pub subject_type: &'a str,
    /// The subject.
    pub subject_id: &'a str,
    /// The unit, when the bookmark points into one.
    pub unit_id: Option<&'a str>,
    /// How far into the unit, in permille.
    pub position_permille: Option<i64>,
    /// The reader's own note.
    pub note: &'a str,
    /// Whether it may appear in a public list. Defaults to false, and the column
    /// says the same.
    pub is_public: bool,
}

/// Create a bookmark.
///
/// Defaults to private when `is_public` is false, which is what the column's own
/// default says as well (spec §14.1).
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn create_bookmark(
    db: &Database,
    account_id: &str,
    new: &NewBookmark<'_>,
) -> Result<Bookmark> {
    let NewBookmark {
        subject_type,
        subject_id,
        unit_id,
        position_permille,
        note,
        is_public,
    } = *new;
    let now = now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let is_public_int: i64 = i64::from(is_public);
    let sql = db.sql(
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, unit_id, \
         position_permille, note, is_public, created_at, updated_at, version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        "INSERT INTO bookmarks (id, account_id, subject_type, subject_id, unit_id, \
         position_permille, note, is_public, created_at, updated_at, version) \
         VALUES (?::uuid, ?::uuid, ?, ?::uuid, ?::uuid, ?, ?, ?::int::boolean, ?, ?, 1)",
    );
    run!(db, sql, |q| q
        .bind(&id)
        .bind(account_id)
        .bind(subject_type)
        .bind(subject_id)
        .bind(unit_id)
        .bind(position_permille)
        .bind(note)
        .bind(is_public_int)
        .bind(&now)
        .bind(&now))
    .await?;

    Ok(Bookmark {
        id,
        account_id: account_id.to_string(),
        subject_type: subject_type.to_string(),
        subject_id: subject_id.to_string(),
        unit_id: unit_id.map(str::to_string),
        position_permille,
        note: note.to_string(),
        is_public,
        created_at: now.clone(),
        updated_at: now,
        version: 1,
    })
}

/// The account's bookmarks, newest first, optionally narrowed to one subject.
pub async fn bookmarks_for(
    db: &Database,
    account_id: &str,
    subject_type: Option<&str>,
    subject_id: Option<&str>,
) -> Result<Vec<Bookmark>> {
    let sql = sql_owned(
        db,
        format!(
            "SELECT {BOOKMARK_COLUMNS} FROM bookmarks \
             WHERE account_id = ? AND (? IS NULL OR subject_type = ?) \
               AND (? IS NULL OR subject_id = ?) \
             ORDER BY created_at DESC, id DESC"
        ),
        format!(
            "SELECT {BOOKMARK_COLUMNS_PG} FROM bookmarks \
             WHERE account_id::text = ? AND (?::text IS NULL OR subject_type = ?) \
               AND (?::text IS NULL OR subject_id::text = ?) \
             ORDER BY created_at DESC, id DESC"
        ),
    );
    let rows: Vec<BookmarkRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(subject_type)
                .bind(subject_type)
                .bind(subject_id)
                .bind(subject_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(subject_type)
                .bind(subject_type)
                .bind(subject_id)
                .bind(subject_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(decode_bookmark).collect())
}

/// One bookmark, if this account owns it.
pub async fn find_bookmark(db: &Database, account_id: &str, id: &str) -> Result<Option<Bookmark>> {
    let sql = sql_owned(
        db,
        format!("SELECT {BOOKMARK_COLUMNS} FROM bookmarks WHERE id = ? AND account_id = ?"),
        format!(
            "SELECT {BOOKMARK_COLUMNS_PG} FROM bookmarks WHERE id::text = ? AND account_id::text = ?"
        ),
    );
    let row: Option<BookmarkRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(id)
                .bind(account_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(id)
                .bind(account_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(decode_bookmark))
}

/// A partial bookmark update.
#[derive(Debug, Clone, Default)]
pub struct BookmarkPatch<'a> {
    /// New note.
    pub note: Option<&'a str>,
    /// New position.
    pub position_permille: Option<i64>,
    /// New visibility.
    pub is_public: Option<bool>,
}

/// Update a bookmark, honouring `expected_version`.
///
/// Returns whether a row changed.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn update_bookmark(
    db: &Database,
    account_id: &str,
    id: &str,
    patch: &BookmarkPatch<'_>,
    expected_version: i64,
) -> Result<bool> {
    let now = now_rfc3339();
    let is_public = patch.is_public.map(i64::from);
    let sql = db.sql(
        "UPDATE bookmarks SET note = COALESCE(?, note), \
         position_permille = COALESCE(?, position_permille), \
         is_public = COALESCE(?, is_public), updated_at = ?, version = version + 1 \
         WHERE id = ? AND account_id = ? AND version = ?",
        "UPDATE bookmarks SET note = COALESCE(?, note), \
         position_permille = COALESCE(?, position_permille), \
         is_public = COALESCE(?::int::boolean, is_public), updated_at = ?, \
         version = version + 1 \
         WHERE id::text = ? AND account_id::text = ? AND version = ?",
    );
    let affected = run!(db, sql, |q| q
        .bind(patch.note)
        .bind(patch.position_permille)
        .bind(is_public)
        .bind(&now)
        .bind(id)
        .bind(account_id)
        .bind(expected_version))
    .await?;
    Ok(affected > 0)
}

/// Delete a bookmark. Returns whether one was removed.
pub async fn delete_bookmark(db: &Database, account_id: &str, id: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM bookmarks WHERE id = ? AND account_id = ?",
        "DELETE FROM bookmarks WHERE id::text = ? AND account_id::text = ?",
    );
    let affected = run!(db, sql, |q| q.bind(id).bind(account_id)).await?;
    Ok(affected > 0)
}

/// Bookmarks on a subject that their owners have made public.
///
/// The one query in this module that is not scoped to an account, and the reason
/// it takes no `account_id`: it exists to serve other readers, and the
/// `is_public` predicate is what makes that safe. Private bookmarks of the same
/// subject are not returned, which is the acceptance criterion "Public bookmark
/// lists exclude private entries".
pub async fn public_bookmarks_for(
    db: &Database,
    subject_type: &str,
    subject_id: &str,
) -> Result<Vec<Bookmark>> {
    let sql = sql_owned(
        db,
        format!(
            "SELECT {BOOKMARK_COLUMNS} FROM bookmarks \
             WHERE subject_type = ? AND subject_id = ? AND is_public = 1 \
             ORDER BY created_at DESC, id DESC"
        ),
        format!(
            "SELECT {BOOKMARK_COLUMNS_PG} FROM bookmarks \
             WHERE subject_type = ? AND subject_id::text = ? AND is_public \
             ORDER BY created_at DESC, id DESC"
        ),
    );
    let rows: Vec<BookmarkRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(subject_type)
                .bind(subject_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(subject_type)
                .bind(subject_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(decode_bookmark).collect())
}

/// How many bookmarks the account has.
pub async fn bookmark_count(db: &Database, account_id: &str) -> Result<i64> {
    let sql = db.sql(
        "SELECT COUNT(*) FROM bookmarks WHERE account_id = ?",
        "SELECT COUNT(*) FROM bookmarks WHERE account_id::text = ?",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(count)
}

// ---------------------------------------------------------------------------
// Private tags
// ---------------------------------------------------------------------------

/// Tag a subject with one of the reader's own tags.
///
/// Idempotent: tagging twice leaves one row. Never touches M9's public taxonomy
/// — that is a different table in a different migration, and this function has
/// no path to it.
///
/// Returns whether a row was inserted.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn add_private_tag(
    db: &Database,
    account_id: &str,
    subject_type: &str,
    subject_id: &str,
    tag: &str,
) -> Result<bool> {
    let now = now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let sql = db.sql(
        "INSERT INTO private_tags (id, account_id, subject_type, subject_id, tag, created_at) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT (account_id, subject_type, subject_id, tag) DO NOTHING",
        "INSERT INTO private_tags (id, account_id, subject_type, subject_id, tag, created_at) \
         VALUES (?::uuid, ?::uuid, ?, ?::uuid, ?, ?) \
         ON CONFLICT (account_id, subject_type, subject_id, tag) DO NOTHING",
    );
    let affected = run!(db, sql, |q| q
        .bind(&id)
        .bind(account_id)
        .bind(subject_type)
        .bind(subject_id)
        .bind(tag)
        .bind(&now))
    .await?;
    Ok(affected > 0)
}

/// Remove one of the reader's tags from a subject.
pub async fn remove_private_tag(
    db: &Database,
    account_id: &str,
    subject_type: &str,
    subject_id: &str,
    tag: &str,
) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM private_tags WHERE account_id = ? AND subject_type = ? \
         AND subject_id = ? AND tag = ?",
        "DELETE FROM private_tags WHERE account_id::text = ? AND subject_type = ? \
         AND subject_id::text = ? AND tag = ?",
    );
    let affected = run!(db, sql, |q| q
        .bind(account_id)
        .bind(subject_type)
        .bind(subject_id)
        .bind(tag))
    .await?;
    Ok(affected > 0)
}

/// The reader's tags on one subject.
pub async fn tags_for(
    db: &Database,
    account_id: &str,
    subject_type: &str,
    subject_id: &str,
) -> Result<Vec<String>> {
    let sql = db.sql(
        "SELECT tag FROM private_tags WHERE account_id = ? AND subject_type = ? \
         AND subject_id = ? ORDER BY tag ASC",
        "SELECT tag FROM private_tags WHERE account_id::text = ? AND subject_type = ? \
         AND subject_id::text = ? ORDER BY tag ASC",
    );
    let rows: Vec<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(subject_type)
                .bind(subject_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(subject_type)
                .bind(subject_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

/// The wire shape of a reading status.
///
/// Lives here rather than in the route that first needed it, because two routes
/// now return one and a duplicated serialiser is a duplicated contract: a field
/// added to the record and not to one copy of this is a response that is missing
/// something the other route sends.
pub fn status_json(row: &ReadingStatusRecord) -> serde_json::Value {
    serde_json::json!({
        "status": row.status.as_str(),
        "started_at": row.started_at,
        "finished_at": row.finished_at,
        "updated_at": row.updated_at,
        "version": row.version,
    })
}

/// Whether `subject_id` is an item in *this* account's library.
///
/// The status and tag doors are keyed on a subject id, and both used to trust
/// it: a `PUT /library/items/{id}/status` naming any UUID in the instance wrote
/// a `reading_status` row for a subject that does not exist and answered 200.
/// The reader's own dashboard then showed a zero that nothing could explain,
/// because the row it wrote is keyed on a subject the query does not count.
///
/// Scoping to `account_id` is the second half of the same check. Without it a
/// reader who learned another reader's item id could write a reading status
/// against it, which is a write into somebody else's namespace — the same
/// subject id, attributed to the wrong owner.
///
/// Returns `false` rather than erroring, so the route decides the shape of the
/// refusal; this is a predicate, not a door.
pub async fn library_item_exists(db: &Database, account_id: &str, item_id: &str) -> Result<bool> {
    // Both dialects bind the id as text: a non-UUID path segment is a
    // miss, not a 500 from a cast.
    let sql = db.sql(
        "SELECT 1 FROM library_items WHERE account_id = ? AND id = ? LIMIT 1",
        "SELECT 1 FROM library_items WHERE account_id::text = ? AND id::text = ? LIMIT 1",
    );
    let found: Option<i32> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(item_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(item_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(found.is_some())
}

/// The reader's tags with how many subjects each is on.
///
/// Scoped to one account by its own predicate, so it can be rendered as a filter
/// bar and leaked by nothing.
pub async fn tags_for_account(db: &Database, account_id: &str) -> Result<Vec<(String, i64)>> {
    let sql = db.sql(
        "SELECT tag, COUNT(*) FROM private_tags WHERE account_id = ? \
         GROUP BY tag ORDER BY tag ASC",
        "SELECT tag, COUNT(*) FROM private_tags WHERE account_id::text = ? \
         GROUP BY tag ORDER BY tag ASC",
    );
    let rows: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Reading status
// ---------------------------------------------------------------------------

/// The reader's status for one subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingStatusRecord {
    /// Identifier.
    pub id: String,
    /// The owning account.
    pub account_id: String,
    /// `work` or `library_item`.
    pub subject_type: String,
    /// The subject.
    pub subject_id: String,
    /// The status.
    pub status: ReadingStatus,
    /// When the reader first started, RFC 3339.
    pub started_at: Option<String>,
    /// When they finished, RFC 3339.
    pub finished_at: Option<String>,
    /// When it last changed, RFC 3339.
    pub updated_at: String,
    /// Optimistic-concurrency version.
    pub version: i64,
}

#[derive(FromRow)]
struct ReadingStatusRow {
    id: String,
    account_id: String,
    subject_type: String,
    subject_id: String,
    status: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
    version: i64,
}

const READING_STATUS_COLUMNS: &str = "id, account_id, subject_type, subject_id, status, \
     started_at, finished_at, updated_at, version";

const READING_STATUS_COLUMNS_PG: &str = "id::text AS id, account_id::text AS account_id, \
     subject_type, subject_id::text AS subject_id, status, started_at, finished_at, \
     updated_at, version";

fn decode_reading_status(row: ReadingStatusRow) -> Result<ReadingStatusRecord> {
    Ok(ReadingStatusRecord {
        id: row.id,
        account_id: row.account_id,
        subject_type: row.subject_type,
        subject_id: row.subject_id,
        status: ReadingStatus::parse(&row.status).ok_or_else(|| {
            anyhow::anyhow!("reading_status holds an unknown status {:?}", row.status)
        })?,
        started_at: row.started_at,
        finished_at: row.finished_at,
        updated_at: row.updated_at,
        version: row.version,
    })
}

/// Set the reader's status for a subject, creating the row if needed.
///
/// # The two timestamps mean different things
///
/// `started_at` is stamped **once**, the first time the reader moves to a status
/// that means they have begun, and is never rewritten: a reader who drops a work
/// and returns to it started reading the first time, and overwriting that would
/// erase the fact.
///
/// `finished_at` follows the status: set when moving to `finished`, cleared when
/// moving away. A work that was finished and then reopened is not finished, and
/// leaving a stale timestamp behind would make the reader's own statistics lie.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn set_reading_status(
    db: &Database,
    account_id: &str,
    subject_type: &str,
    subject_id: &str,
    status: ReadingStatus,
) -> Result<ReadingStatusRecord> {
    let now = now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let started = status.is_started().then(|| now.clone());
    let finished = status.is_finished().then(|| now.clone());
    let sql = db.sql(
        "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, \
         started_at, finished_at, updated_at, version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1) \
         ON CONFLICT (account_id, subject_type, subject_id) DO UPDATE SET \
             status = excluded.status, \
             started_at = COALESCE(reading_status.started_at, excluded.started_at), \
             finished_at = CASE WHEN excluded.status = 'finished' \
                                THEN COALESCE(reading_status.finished_at, excluded.finished_at) \
                                ELSE NULL END, \
             updated_at = excluded.updated_at, \
             version = reading_status.version + 1",
        "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, \
         started_at, finished_at, updated_at, version) \
         VALUES (?::uuid, ?::uuid, ?, ?::uuid, ?, ?, ?, ?, 1) \
         ON CONFLICT (account_id, subject_type, subject_id) DO UPDATE SET \
             status = excluded.status, \
             started_at = COALESCE(reading_status.started_at, excluded.started_at), \
             finished_at = CASE WHEN excluded.status = 'finished' \
                                THEN COALESCE(reading_status.finished_at, excluded.finished_at) \
                                ELSE NULL END, \
             updated_at = excluded.updated_at, \
             version = reading_status.version + 1",
    );
    run!(db, sql, |q| q
        .bind(&id)
        .bind(account_id)
        .bind(subject_type)
        .bind(subject_id)
        .bind(status.as_str())
        .bind(&started)
        .bind(&finished)
        .bind(&now))
    .await?;

    reading_status_for(db, account_id, subject_type, subject_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the reading status upsert left no row"))
}

/// Set a reading status the reader does not already have, and say whether it
/// was written.
///
/// This is the shelf-import path. Imports bring a library state from another
/// site, and a state the reader has already set here is theirs: the import can
/// add what is missing and must never overwrite what the reader decided. The
/// unique index on `(account_id, subject_type, subject_id)` is what makes
/// "only if absent" exact rather than a read-then-write race.
///
/// `finished_at` comes from the export rather than from the clock, because the
/// reader finished the book in 2019 and imported it today.
pub async fn set_imported_reading_status(
    db: &Database,
    account_id: &str,
    subject_type: &str,
    subject_id: &str,
    status: ReadingStatus,
    finished_at: Option<&str>,
) -> Result<bool> {
    let now = now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let started = status
        .is_started()
        .then(|| finished_at.unwrap_or(&now).to_owned());
    let finished = status
        .is_finished()
        .then(|| finished_at.unwrap_or(&now).to_owned());
    let sql = db.sql(
        "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, \
         started_at, finished_at, updated_at, version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1) \
         ON CONFLICT (account_id, subject_type, subject_id) DO NOTHING",
        "INSERT INTO reading_status (id, account_id, subject_type, subject_id, status, \
         started_at, finished_at, updated_at, version) \
         VALUES (?::uuid, ?::uuid, ?, ?::uuid, ?, ?, ?, ?, 1) \
         ON CONFLICT (account_id, subject_type, subject_id) DO NOTHING",
    );
    let affected = match db.backend() {
        crate::Backend::Sqlite => {
            let query = sqlx::query(&sql)
                .bind(&id)
                .bind(account_id)
                .bind(subject_type)
                .bind(subject_id)
                .bind(status.as_str())
                .bind(&started)
                .bind(&finished)
                .bind(&now);
            query
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?
                .rows_affected()
        }
        crate::Backend::Postgres => {
            let query = sqlx::query(&sql)
                .bind(&id)
                .bind(account_id)
                .bind(subject_type)
                .bind(subject_id)
                .bind(status.as_str())
                .bind(&started)
                .bind(&finished)
                .bind(&now);
            query
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?
                .rows_affected()
        }
    };
    Ok(affected > 0)
}

/// The reader's status for a subject, if they have set one.
pub async fn reading_status_for(
    db: &Database,
    account_id: &str,
    subject_type: &str,
    subject_id: &str,
) -> Result<Option<ReadingStatusRecord>> {
    let sql = sql_owned(
        db,
        format!(
            "SELECT {READING_STATUS_COLUMNS} FROM reading_status \
             WHERE account_id = ? AND subject_type = ? AND subject_id = ?"
        ),
        format!(
            "SELECT {READING_STATUS_COLUMNS_PG} FROM reading_status \
             WHERE account_id::text = ? AND subject_type = ? AND subject_id::text = ?"
        ),
    );
    let row: Option<ReadingStatusRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(subject_type)
                .bind(subject_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(subject_type)
                .bind(subject_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    row.map(decode_reading_status).transpose()
}

/// Clear the reader's status for a subject. Returns whether one was removed.
pub async fn clear_reading_status(
    db: &Database,
    account_id: &str,
    subject_type: &str,
    subject_id: &str,
) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM reading_status WHERE account_id = ? AND subject_type = ? AND subject_id = ?",
        "DELETE FROM reading_status WHERE account_id::text = ? AND subject_type = ? \
         AND subject_id::text = ?",
    );
    let affected = run!(db, sql, |q| q
        .bind(account_id)
        .bind(subject_type)
        .bind(subject_id))
    .await?;
    Ok(affected > 0)
}

/// How many subjects the reader has in each status.
pub async fn reading_status_counts(
    db: &Database,
    account_id: &str,
) -> Result<Vec<(ReadingStatus, i64)>> {
    let sql = db.sql(
        "SELECT status, COUNT(*) FROM reading_status WHERE account_id = ? \
         GROUP BY status ORDER BY status ASC",
        "SELECT status, COUNT(*) FROM reading_status WHERE account_id::text = ? \
         GROUP BY status ORDER BY status ASC",
    );
    let rows: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    // A status this build does not know is dropped rather than guessed at, and
    // the count is reported under the name the database holds.
    Ok(rows
        .into_iter()
        .filter_map(|(name, count)| ReadingStatus::parse(&name).map(|s| (s, count)))
        .collect())
}

// ---------------------------------------------------------------------------
// Saved views
// ---------------------------------------------------------------------------

/// The current version of the stored query document.
///
/// Bumped when the query shape changes; a row whose version this build does not
/// know is reported as needing repair rather than misread (spec §14.2).
pub const QUERY_DOCUMENT_VERSION: i64 = 2;

/// Which query language a saved view's document holds.
///
/// Version 1 documents carry a [`LibraryQuery`] and no `kind`; they are read as
/// [`SavedViewKind::Library`] and never rewritten, so an old view keeps working
/// and its bytes stay exactly as its author's build wrote them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedViewKind {
    /// A library query — `GET /library/items`'s parameters.
    #[default]
    Library,
    /// A catalogue query, stored as the canonical query string the caller sends
    /// to `GET /catalogue`.
    Catalogue,
}

impl SavedViewKind {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Catalogue => "catalogue",
        }
    }

    /// Parse the wire name; `None` for a word this build does not know.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "library" => Some(Self::Library),
            "catalogue" | "catalog" => Some(Self::Catalogue),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredQuery {
    version: i64,
    /// Absent in version 1 documents, which are library queries by definition.
    #[serde(default)]
    kind: SavedViewKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<LibraryQuery>,
    /// A catalogue query, as the canonical query string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    params: Option<String>,
}

/// A stored library query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedView {
    /// Identifier.
    pub id: String,
    /// The owning account.
    pub account_id: String,
    /// Display name.
    pub name: String,
    /// The query, when this build can read it.
    pub query: Option<LibraryQuery>,
    /// Which query language this view's document holds.
    pub kind: SavedViewKind,
    /// A catalogue query's canonical query string, when this is one.
    pub params: Option<String>,
    /// Whether the stored query needs repair before it can be used.
    ///
    /// True when the document's version is not one this build knows, or when it
    /// will not parse. The view is still listed — deleting or renaming it must
    /// not require understanding it.
    pub needs_repair: bool,
    /// The version of the stored document.
    pub query_version: i64,
    /// Ordering.
    pub sort: LibrarySort,
    /// Who the view is for.
    pub scope: ViewScope,
    /// Whether it is pinned to navigation.
    pub pinned: bool,
    /// Whether it may be shared.
    pub is_public: bool,
    /// When it was created, RFC 3339.
    pub created_at: String,
    /// When it last changed, RFC 3339.
    pub updated_at: String,
    /// Optimistic-concurrency version.
    pub version: i64,
}

#[derive(FromRow)]
struct SavedViewRow {
    id: String,
    account_id: String,
    name: String,
    query_json: String,
    query_version: i64,
    sort: String,
    scope: String,
    pinned: i64,
    is_public: i64,
    created_at: String,
    updated_at: String,
    version: i64,
}

const SAVED_VIEW_COLUMNS: &str = "id, account_id, name, query_json, query_version, sort, scope, \
     pinned, is_public, created_at, updated_at, version";

const SAVED_VIEW_COLUMNS_PG: &str = "id::text AS id, account_id::text AS account_id, name, \
     query_json, query_version, sort, scope, pinned::int::bigint AS pinned, \
     is_public::int::bigint AS is_public, created_at, updated_at, version";

fn decode_saved_view(row: SavedViewRow) -> SavedView {
    let stored: Option<StoredQuery> = serde_json::from_str(&row.query_json).ok();
    let mut kind = SavedViewKind::Library;
    let mut query = None;
    let mut params = None;
    let mut needs_repair = true;
    if let Some(stored) = stored {
        // Version 1 documents are library queries with no `kind` written; they are
        // read as such rather than migrated, so an old view keeps working and its
        // bytes stay as its author's build wrote them.
        if stored.version == 1 || stored.version == QUERY_DOCUMENT_VERSION {
            kind = if stored.version == 1 {
                SavedViewKind::Library
            } else {
                stored.kind
            };
            match kind {
                SavedViewKind::Library => {
                    query = stored.query;
                    needs_repair = query.is_none();
                }
                SavedViewKind::Catalogue => {
                    params = stored.params.filter(|p| !p.trim().is_empty());
                    needs_repair = params.is_none();
                }
            }
        }
        // Any other version: a build that thought about the query differently.
        // Both states mean "do not pretend to understand this", which the
        // interface can show.
    }
    SavedView {
        id: row.id,
        account_id: row.account_id,
        name: row.name,
        query,
        kind,
        params,
        needs_repair,
        query_version: row.query_version,
        sort: LibrarySort::parse(&row.sort),
        scope: ViewScope::parse(&row.scope).unwrap_or_default(),
        pinned: row.pinned != 0,
        is_public: row.is_public != 0,
        created_at: row.created_at,
        updated_at: row.updated_at,
        version: row.version,
    }
}

/// Store a query as a named view.
///
/// The query is validated here, not by the caller, and the scope decides the
/// rule: [`darkhold_domain::library::validate_query`] refuses a public view
/// that filters by a shelf, a private tag or a reading status. Putting the check
/// inside the writer rather than at the route is deliberate — a rule that lives
/// at the call site is a rule a second call site will not have.
///
/// # Errors
///
/// Returns an error when the query is refused, or if the statement fails.
pub async fn create_saved_view(
    db: &Database,
    account_id: &str,
    name: &str,
    query: &LibraryQuery,
    scope: ViewScope,
    pinned: bool,
    sort: LibrarySort,
) -> Result<SavedView> {
    darkhold_domain::library::validate_query(query, scope).map_err(|e| anyhow::anyhow!("{e}"))?;
    let document = serde_json::to_string(&StoredQuery {
        version: QUERY_DOCUMENT_VERSION,
        kind: SavedViewKind::Library,
        query: Some(query.clone()),
        params: None,
    })?;
    insert_view(db, account_id, name, &document, scope, pinned, sort).await
}

/// Store a catalogue query as a named view.
///
/// `params` is the canonical query string the caller already sends to
/// `GET /catalogue` — stored verbatim, because a second conversion of the same
/// parameters would be a second set of rules for windows and fandoms, and the two
/// would disagree exactly where a window is anchored.
///
/// One rule is enforced here, in the writer, for the same reason
/// [`create_saved_view`] validates here: `exclude_read` reads the caller's own
/// history, so a view whose params carry it is refused unless it is scoped to the
/// library. A public view that answered differently for every reader while
/// claiming to be shareable is the failure this refuses.
///
/// # Errors
///
/// Returns an error when the params are refused, or if the statement fails.
pub async fn create_catalogue_view(
    db: &Database,
    account_id: &str,
    name: &str,
    params: &str,
    scope: ViewScope,
    pinned: bool,
) -> Result<SavedView> {
    let params = params.trim();
    if params.is_empty() {
        anyhow::bail!(
            "a catalogue view needs a query — `dh rec --save <name>` stores the one you ran"
        );
    }
    let reads_history = params.split('&').any(|pair| {
        pair.split('=')
            .next()
            .is_some_and(|key| key == "exclude_read")
    });
    if reads_history && scope == ViewScope::Public {
        anyhow::bail!(
            "exclude_read: this query depends on one account's reading history, so the view must be \
             scoped to the library rather than shared"
        );
    }
    let document = serde_json::to_string(&StoredQuery {
        version: QUERY_DOCUMENT_VERSION,
        kind: SavedViewKind::Catalogue,
        query: None,
        params: Some(params.to_owned()),
    })?;
    insert_view(
        db,
        account_id,
        name,
        &document,
        scope,
        pinned,
        LibrarySort::Recent,
    )
    .await
}

/// Insert a stored document and read it back.
///
/// The read-back is deliberate: the returned view is decoded from the bytes that
/// were written, so a writer cannot return a struct that disagrees with its row —
/// the failure that makes a document look right in a unit test and refuse to
/// decode in production.
async fn insert_view(
    db: &Database,
    account_id: &str,
    name: &str,
    document: &str,
    scope: ViewScope,
    pinned: bool,
    sort: LibrarySort,
) -> Result<SavedView> {
    if name.trim().is_empty() {
        anyhow::bail!("a view needs a name");
    }

    let now = now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let pinned_int: i64 = i64::from(pinned);
    // A view is public only if it was both scoped public and marked public; a
    // public *scope* with a private flag is a view its owner has not shared.
    let is_public_int: i64 = i64::from(scope == ViewScope::Public);
    let sql = db.sql(
        "INSERT INTO saved_views (id, account_id, name, query_json, query_version, sort, scope, \
         pinned, is_public, created_at, updated_at, version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        "INSERT INTO saved_views (id, account_id, name, query_json, query_version, sort, scope, \
         pinned, is_public, created_at, updated_at, version) \
         VALUES (?::uuid, ?::uuid, ?, ?, ?, ?, ?, ?::int::boolean, ?::int::boolean, ?, ?, 1)",
    );
    run!(db, sql, |q| q
        .bind(&id)
        .bind(account_id)
        .bind(name)
        .bind(document)
        .bind(QUERY_DOCUMENT_VERSION)
        .bind(sort.as_str())
        .bind(scope.as_str())
        .bind(pinned_int)
        .bind(is_public_int)
        .bind(&now)
        .bind(&now))
    .await?;

    find_saved_view(db, account_id, &id).await?.ok_or_else(|| {
        anyhow::anyhow!("the saved view could not be read back after it was written")
    })
}

/// How many ids a run remembers. A saved query's page is at most a few hundred
/// rows today; the cap is here so a caller streaming a whole catalogue into one
/// run cannot grow this row without bound.
pub const SEEN_CAP: usize = 5_000;

/// One recorded run of a saved query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRun {
    /// Ids in this run that were not in the previous one, in page order.
    pub new_ids: Vec<String>,
    /// How many ids the previous run recorded; 0 on the first run.
    pub previous: i64,
    /// When this run was recorded, RFC 3339.
    pub ran_at: String,
}

/// Record a run of a saved query and answer what is new since the last one.
///
/// `Ok(None)` means the view does not exist or is not this account's — the caller
/// answers `404`, not an empty diff, because "nothing new" and "no such view" are
/// different facts and a scheduler that acted on the first would never notice the
/// second.
///
/// # Errors
///
/// Returns the store's error when a statement fails.
pub async fn record_run(
    db: &Database,
    account_id: &str,
    view_id: &str,
    ids: &[String],
) -> Result<Option<QueryRun>> {
    if find_saved_view(db, account_id, view_id).await?.is_none() {
        return Ok(None);
    }

    let previous = previous_run(db, view_id).await?;
    let new_ids = darkhold_domain::library::new_ids_since(&previous, ids);

    let ran_at = now_rfc3339();
    let capped: Vec<&String> = ids.iter().take(SEEN_CAP).collect();
    let seen = serde_json::to_string(&capped)?;
    let seen_count = i64::try_from(capped.len()).unwrap_or(i64::MAX);
    let sql = db.sql(
        "INSERT INTO saved_query_runs (saved_view_id, ran_at, seen_ids_json, seen_count) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT (saved_view_id) DO UPDATE SET ran_at = excluded.ran_at, \
         seen_ids_json = excluded.seen_ids_json, seen_count = excluded.seen_count",
        "INSERT INTO saved_query_runs (saved_view_id, ran_at, seen_ids_json, seen_count) \
         VALUES (?::uuid, ?, ?, ?) \
         ON CONFLICT (saved_view_id) DO UPDATE SET ran_at = excluded.ran_at, \
         seen_ids_json = excluded.seen_ids_json, seen_count = excluded.seen_count",
    );
    run!(db, sql, |q| q
        .bind(view_id)
        .bind(&ran_at)
        .bind(&seen)
        .bind(seen_count))
    .await?;

    Ok(Some(QueryRun {
        new_ids,
        previous: i64::try_from(previous.len()).unwrap_or(i64::MAX),
        ran_at,
    }))
}

/// The ids the previous run recorded, or nothing on a first run.
async fn previous_run(db: &Database, view_id: &str) -> Result<Vec<String>> {
    let sql = db.sql(
        "SELECT seen_ids_json FROM saved_query_runs WHERE saved_view_id = ?",
        "SELECT seen_ids_json FROM saved_query_runs WHERE saved_view_id = ?::uuid",
    );
    let stored: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(view_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(view_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(stored
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default())
}

/// Every view the account owns, pinned first.
pub async fn saved_views_for(db: &Database, account_id: &str) -> Result<Vec<SavedView>> {
    let sql = sql_owned(
        db,
        format!(
            "SELECT {SAVED_VIEW_COLUMNS} FROM saved_views WHERE account_id = ? \
             ORDER BY pinned DESC, name ASC"
        ),
        format!(
            "SELECT {SAVED_VIEW_COLUMNS_PG} FROM saved_views WHERE account_id::text = ? \
             ORDER BY pinned DESC, name ASC"
        ),
    );
    let rows: Vec<SavedViewRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(decode_saved_view).collect())
}

/// One view, if this account owns it.
pub async fn find_saved_view(
    db: &Database,
    account_id: &str,
    id: &str,
) -> Result<Option<SavedView>> {
    let sql = sql_owned(
        db,
        format!("SELECT {SAVED_VIEW_COLUMNS} FROM saved_views WHERE id = ? AND account_id = ?"),
        format!(
            "SELECT {SAVED_VIEW_COLUMNS_PG} FROM saved_views \
             WHERE id::text = ? AND account_id::text = ?"
        ),
    );
    let row: Option<SavedViewRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(id)
                .bind(account_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(id)
                .bind(account_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(decode_saved_view))
}

/// Rename, pin or re-scope a view, honouring `expected_version`.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn update_saved_view(
    db: &Database,
    account_id: &str,
    id: &str,
    name: Option<&str>,
    pinned: Option<bool>,
    expected_version: i64,
) -> Result<bool> {
    let now = now_rfc3339();
    let pinned_int = pinned.map(i64::from);
    let sql = db.sql(
        "UPDATE saved_views SET name = COALESCE(?, name), pinned = COALESCE(?, pinned), \
         updated_at = ?, version = version + 1 \
         WHERE id = ? AND account_id = ? AND version = ?",
        "UPDATE saved_views SET name = COALESCE(?, name), \
         pinned = COALESCE(?::int::boolean, pinned), updated_at = ?, version = version + 1 \
         WHERE id::text = ? AND account_id::text = ? AND version = ?",
    );
    let affected = run!(db, sql, |q| q
        .bind(name)
        .bind(pinned_int)
        .bind(&now)
        .bind(id)
        .bind(account_id)
        .bind(expected_version))
    .await?;
    Ok(affected > 0)
}

/// Whether `slug` is a legal name for a saved view.
///
/// Lowercase letters, digits and single hyphens, 1–64 characters, not starting or ending with a hyphen. Refused rather than
/// normalised: a reader who types `To Read` and gets `to-read` has a view whose address they did not choose, and the next person
/// asking for `to-read` finds somebody else's guess. The rule matches handles and route segments elsewhere in this tree.
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Name a saved view, or clear its name with `None`.
///
/// Returns `Ok(false)` when the view is not this account's, and `Err` when the name is taken by another of the account's views —
/// the unique index is the authority, and its violation is turned into a message naming the slug rather than a database error,
/// because the reader can fix it by choosing another one.
pub async fn set_saved_view_slug(
    db: &Database,
    account_id: &str,
    id: &str,
    slug: Option<&str>,
) -> Result<bool> {
    if let Some(slug) = slug {
        if !valid_slug(slug) {
            anyhow::bail!("a saved view's name is lowercase letters, digits and single hyphens, up to 64 characters");
        }
    }
    let now = now_rfc3339();
    let sql = db.sql(
        "UPDATE saved_views SET slug = ?, updated_at = ?, version = version + 1 \
         WHERE id = ? AND account_id = ?",
        "UPDATE saved_views SET slug = ?, updated_at = ?, version = version + 1 \
         WHERE id::text = ? AND account_id::text = ?",
    );
    let taken = db.sql(
        "SELECT COUNT(*) FROM saved_views WHERE account_id = ? AND slug = ? AND id <> ?",
        "SELECT COUNT(*) FROM saved_views WHERE account_id::text = ? AND slug = ? AND id::text <> ?",
    );
    if let Some(slug) = slug {
        let count: i64 = match db.backend() {
            Backend::Sqlite => {
                sqlx::query_scalar(&taken)
                    .bind(account_id)
                    .bind(slug)
                    .bind(id)
                    .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                    .await?
            }
            Backend::Postgres => {
                sqlx::query_scalar(&taken)
                    .bind(account_id)
                    .bind(slug)
                    .bind(id)
                    .fetch_one(db.postgres_pool().expect("postgres handle"))
                    .await?
            }
        };
        if count > 0 {
            anyhow::bail!("you already have a saved view named `{slug}`");
        }
    }
    let affected = run!(db, sql, |q| q
        .bind(slug)
        .bind(&now)
        .bind(id)
        .bind(account_id))
    .await?;
    Ok(affected > 0)
}

/// Resolve a slug to the id of this account's saved view.
///
/// Account-scoped on purpose: slugs are unique per account, not globally, so a global lookup would let one reader address
/// another reader's view — and a view carries the filter someone built, which is close to a description of what they read.
pub async fn saved_view_id_for_slug(
    db: &Database,
    account_id: &str,
    slug: &str,
) -> Result<Option<String>> {
    let sql = db.sql(
        "SELECT CAST(id AS TEXT) FROM saved_views WHERE account_id = ? AND slug = ?",
        "SELECT id::text FROM saved_views WHERE account_id::text = ? AND slug = ?",
    );
    let row: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(slug)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(slug)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row)
}

/// Delete a view. Returns whether one was removed.
pub async fn delete_saved_view(db: &Database, account_id: &str, id: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM saved_views WHERE id = ? AND account_id = ?",
        "DELETE FROM saved_views WHERE id::text = ? AND account_id::text = ?",
    );
    let affected = run!(db, sql, |q| q.bind(id).bind(account_id)).await?;
    Ok(affected > 0)
}

// ---------------------------------------------------------------------------
// The library listing
// ---------------------------------------------------------------------------

/// One page of a library listing and the total the filter matched.
#[derive(Debug, Clone)]
pub struct LibraryPage {
    /// The items on this page.
    pub items: Vec<LibraryItem>,
    /// How many the filter matched in total, so the interface can page.
    pub total: i64,
}

/// `?` repeated `n` times, optionally cast.
pub fn placeholders(n: usize, cast: bool) -> String {
    let one = if cast { "?::uuid" } else { "?" };
    vec![one; n].join(", ")
}

/// One facet of the filter, as SQL and the values to bind, in bind order.
struct Facet {
    sqlite: String,
    postgres: String,
    values: Vec<String>,
}

/// Build the shared `WHERE` body for a library query.
///
/// The two dialects differ only in casts, so the clause structure is written
/// once and the casts applied to a copy. The account is bound once and the
/// facets compare against `library_items.account_id` rather than binding it
/// again, which keeps the bind order identical in both arms and impossible to
/// get wrong by counting.
///
/// Values within a facet are ORed; facets are ANDed. An empty facet contributes
/// no clause at all, because `IN ()` is a syntax error in both dialects.
fn library_filter(query: &LibraryQuery, shelf_ids: &[String]) -> (String, String, Vec<String>) {
    let mut facets: Vec<Facet> = Vec::new();

    if !shelf_ids.is_empty() {
        facets.push(Facet {
            sqlite: format!(
                "EXISTS (SELECT 1 FROM shelf_items si WHERE si.library_item_id = library_items.id \
                 AND si.shelf_id IN ({}))",
                placeholders(shelf_ids.len(), false)
            ),
            postgres: format!(
                "EXISTS (SELECT 1 FROM shelf_items si WHERE si.library_item_id = library_items.id \
                 AND si.shelf_id IN ({}))",
                placeholders(shelf_ids.len(), false)
            ),
            values: shelf_ids.to_vec(),
        });
    }

    if !query.tags.is_empty() {
        // The tag must belong to the account that owns the item, which is also
        // what stops one reader's tag filtering another reader's listing.
        facets.push(Facet {
            sqlite: format!(
                "EXISTS (SELECT 1 FROM private_tags pt \
                 WHERE pt.account_id = library_items.account_id \
                   AND pt.subject_type = 'library_item' \
                   AND pt.subject_id = library_items.id AND pt.tag IN ({}))",
                vec!["?"; query.tags.len()].join(", ")
            ),
            postgres: format!(
                "EXISTS (SELECT 1 FROM private_tags pt \
                 WHERE pt.account_id = library_items.account_id \
                   AND pt.subject_type = 'library_item' \
                   AND pt.subject_id = library_items.id AND pt.tag IN ({}))",
                vec!["?"; query.tags.len()].join(", ")
            ),
            values: query.tags.clone(),
        });
    }

    if !query.statuses.is_empty() {
        let names: Vec<String> = query
            .statuses
            .iter()
            .map(|s| s.as_str().to_string())
            .collect();
        facets.push(Facet {
            sqlite: format!(
                "EXISTS (SELECT 1 FROM reading_status rs \
                 WHERE rs.account_id = library_items.account_id \
                   AND rs.subject_type = 'library_item' \
                   AND rs.subject_id = library_items.id AND rs.status IN ({}))",
                vec!["?"; names.len()].join(", ")
            ),
            postgres: format!(
                "EXISTS (SELECT 1 FROM reading_status rs \
                 WHERE rs.account_id = library_items.account_id \
                   AND rs.subject_type = 'library_item' \
                   AND rs.subject_id = library_items.id AND rs.status IN ({}))",
                vec!["?"; names.len()].join(", ")
            ),
            values: names,
        });
    }

    if let Some(source) = &query.source {
        facets.push(Facet {
            sqlite: "library_items.source_key = ?".to_string(),
            postgres: "library_items.source_key = ?".to_string(),
            values: vec![source.clone()],
        });
    }

    if let Some(since) = &query.updated_since {
        // A null `source_updated_at` means the source never said, which is not
        // the same as "older than the bound" — such an item is excluded rather
        // than silently included.
        let clause = "library_items.source_updated_at IS NOT NULL \
                      AND library_items.source_updated_at >= ?";
        facets.push(Facet {
            sqlite: clause.to_string(),
            postgres: clause.to_string(),
            values: vec![since.clone()],
        });
    }

    let mut sqlite = String::new();
    let mut postgres = String::new();
    let mut values = Vec::new();
    for facet in facets {
        if !sqlite.is_empty() {
            sqlite.push_str(" AND ");
            postgres.push_str(" AND ");
        }
        sqlite.push_str(&facet.sqlite);
        postgres.push_str(&facet.postgres);
        values.extend(facet.values);
    }

    (sqlite, postgres, values)
}

/// The ordering clause for a sort.
///
/// `COALESCE` and an `IS NULL` group are used rather than `NULLS LAST` so the
/// two dialects order identically without depending on the SQLite build's
/// version.
fn order_by(sort: LibrarySort) -> &'static str {
    match sort {
        LibrarySort::Recent => "library_items.created_at DESC, library_items.id DESC",
        LibrarySort::Title => "library_items.title ASC, library_items.id DESC",
        LibrarySort::Updated => {
            "(library_items.source_updated_at IS NULL) ASC, \
             library_items.source_updated_at DESC, library_items.id DESC"
        }
        LibrarySort::Words => "COALESCE(library_items.word_count, -1) DESC, library_items.id DESC",
        LibrarySort::Position => {
            "(SELECT COALESCE(MIN(si2.position), 0) FROM shelf_items si2 \
             WHERE si2.library_item_id = library_items.id) ASC, \
             library_items.created_at DESC"
        }
    }
}

/// The reader's own shelf ids for the shelf names a query names.
///
/// Resolved before the listing so the listing can bind shelf *ids*: a name is
/// what the reader typed, and only an id proves the shelf is theirs.
pub async fn shelf_ids_for_names(
    db: &Database,
    account_id: &str,
    names: &[String],
) -> Result<Vec<String>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let shelves = shelves_for(db, account_id).await?;
    Ok(shelves
        .into_iter()
        .filter(|s| names.iter().any(|n| n == &s.name))
        .map(|s| s.id)
        .collect())
}

/// Run a library query: one page of items, and the total that matched.
///
/// # Errors
///
/// Returns an error if either statement fails.
pub async fn query_library(
    db: &Database,
    account_id: &str,
    query: &LibraryQuery,
    limit: i64,
    offset: i64,
) -> Result<LibraryPage> {
    let shelf_ids = shelf_ids_for_names(db, account_id, &query.shelves).await?;
    let (facet_sqlite, facet_postgres, values) = library_filter(query, &shelf_ids);
    let connector = if facet_sqlite.is_empty() { "" } else { " AND " };
    let limit = limit.clamp(1, 200);
    let offset = offset.max(0);

    let where_sqlite =
        format!("FROM library_items WHERE library_items.account_id = ?{connector}{facet_sqlite}");
    let where_postgres = format!(
        "FROM library_items WHERE library_items.account_id::text = ?{connector}{facet_postgres}"
    );

    let count_sql = sql_owned(
        db,
        format!("SELECT COUNT(*) {where_sqlite}"),
        format!("SELECT COUNT(*) {where_postgres}"),
    );
    let page_sql = sql_owned(
        db,
        format!(
            "SELECT {LIBRARY_COLUMNS}, {LIBRARY_UNIT_COUNT} {where_sqlite} \
             ORDER BY {} LIMIT ? OFFSET ?",
            order_by(query.sort)
        ),
        format!(
            "SELECT {LIBRARY_COLUMNS_PG}, {LIBRARY_UNIT_COUNT} {where_postgres} \
             ORDER BY {} LIMIT ? OFFSET ?",
            order_by(query.sort)
        ),
    );

    let (total, rows) = match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query_scalar::<_, i64>(&count_sql).bind(account_id);
            for value in &values {
                q = q.bind(value);
            }
            let total: i64 = q
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?;

            let mut q = sqlx::query_as::<_, LibraryItemRow>(&page_sql).bind(account_id);
            for value in &values {
                q = q.bind(value);
            }
            let rows: Vec<LibraryItemRow> = q
                .bind(limit)
                .bind(offset)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?;
            (total, rows)
        }
        Backend::Postgres => {
            let mut q = sqlx::query_scalar::<_, i64>(&count_sql).bind(account_id);
            for value in &values {
                q = q.bind(value);
            }
            let total: i64 = q
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?;

            let mut q = sqlx::query_as::<_, LibraryItemRow>(&page_sql).bind(account_id);
            for value in &values {
                q = q.bind(value);
            }
            let rows: Vec<LibraryItemRow> = q
                .bind(limit)
                .bind(offset)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?;
            (total, rows)
        }
    };

    let items = rows
        .into_iter()
        .map(decode_library_item)
        .collect::<Result<Vec<_>>>()?;
    Ok(LibraryPage { items, total })
}

/// Every library item the account owns, oldest first, up to `limit`.
///
/// The update check walks the whole library rather than a page, so it needs a
/// read that is not the reader's paged listing. Ordered by creation so a job
/// that stops early has checked the same items the next one starts with, which
/// is what makes a partially completed check resumable.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn all_library_items(
    db: &Database,
    account_id: &str,
    limit: i64,
) -> Result<Vec<LibraryItem>> {
    let sql = sql_owned(
        db,
        format!(
            "SELECT {LIBRARY_COLUMNS}, {LIBRARY_UNIT_COUNT} FROM library_items \
             WHERE library_items.account_id = ? \
             ORDER BY library_items.created_at ASC, library_items.id ASC LIMIT ?"
        ),
        format!(
            "SELECT {LIBRARY_COLUMNS_PG}, {LIBRARY_UNIT_COUNT} FROM library_items \
             WHERE library_items.account_id::text = ? \
             ORDER BY library_items.created_at ASC, library_items.id ASC LIMIT ?"
        ),
    );
    let rows: Vec<LibraryItemRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    rows.into_iter().map(decode_library_item).collect()
}

// ---------------------------------------------------------------------------
// A page's worth of library facts
// ---------------------------------------------------------------------------

/// What a reader has done with one library item: the shelves it is on, their own
/// tags on it, and their reading status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemFacts {
    /// Shelf names, in the reader's own sidebar order.
    pub shelves: Vec<String>,
    /// The reader's private tags, sorted.
    pub tags: Vec<String>,
    /// The reader's reading status, when they have set one.
    pub status: Option<ReadingStatus>,
}

/// The library facts for a page of items, keyed by item id.
///
/// Three queries for the whole page rather than three per item: a page of fifty
/// items would otherwise be a hundred and fifty round trips to render one
/// screen, which is the difference between a page and a stall. The returned map
/// has an entry for every id asked about, so a caller never has to branch on a
/// missing key to tell "no tags" from "not looked up".
///
/// # Errors
///
/// Returns an error if a statement fails.
pub async fn facts_for_items(
    db: &Database,
    account_id: &str,
    ids: &[String],
) -> Result<std::collections::HashMap<String, ItemFacts>> {
    let mut facts: std::collections::HashMap<String, ItemFacts> = ids
        .iter()
        .map(|id| (id.clone(), ItemFacts::default()))
        .collect();
    if ids.is_empty() {
        return Ok(facts);
    }

    // PostgreSQL has no implicit `uuid = text`, and sqlx cannot decode a `uuid`
    // column into a `String`, so both the comparison and the projection have to be
    // told what they are. SQLite has neither problem and needs neither cast.
    let postgres = db.backend() == Backend::Postgres;
    let account_cast = if postgres { "::uuid" } else { "" };
    let id_cast = if postgres { "::text" } else { "" };
    let cast = |n: usize| placeholders(n, postgres);

    // Shelves, with each item's position so the sidebar's order is the one the
    // reader arranged rather than an alphabetical accident.
    let shelves_query = format!(
        "SELECT si.library_item_id{id_cast}, s.name FROM shelf_items si \
         JOIN shelves s ON s.id = si.shelf_id \
         WHERE s.account_id = ?{account_cast} AND si.library_item_id IN ({}) \
         ORDER BY s.position ASC, s.name ASC",
        cast(ids.len())
    );
    let rows: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query_as(&shelves_query).bind(account_id);
            for id in ids {
                q = q.bind(id);
            }
            q.fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let sql = rewrite_placeholders(&shelves_query);
            let mut q = sqlx::query_as(&sql).bind(account_id);
            for id in ids {
                q = q.bind(id);
            }
            q.fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    for (item_id, name) in rows {
        if let Some(entry) = facts.get_mut(&item_id) {
            entry.shelves.push(name);
        }
    }

    // The reader's own tags. Scoped by account in its own right, not by the
    // shelf join above or by a caller's hopeful join.
    let tags_query = format!(
        "SELECT subject_id{id_cast}, tag FROM private_tags \
         WHERE account_id = ?{account_cast} AND subject_type = 'library_item' \
           AND subject_id IN ({}) \
         ORDER BY tag ASC",
        cast(ids.len())
    );
    let rows: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query_as(&tags_query).bind(account_id);
            for id in ids {
                q = q.bind(id);
            }
            q.fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let sql = rewrite_placeholders(&tags_query);
            let mut q = sqlx::query_as(&sql).bind(account_id);
            for id in ids {
                q = q.bind(id);
            }
            q.fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    for (item_id, tag) in rows {
        if let Some(entry) = facts.get_mut(&item_id) {
            entry.tags.push(tag);
        }
    }

    // Reading statuses.
    let status_query = format!(
        "SELECT subject_id{id_cast}, status FROM reading_status \
         WHERE account_id = ?{account_cast} AND subject_type = 'library_item' \
           AND subject_id IN ({})",
        cast(ids.len())
    );
    let rows: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query_as(&status_query).bind(account_id);
            for id in ids {
                q = q.bind(id);
            }
            q.fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let sql = rewrite_placeholders(&status_query);
            let mut q = sqlx::query_as(&sql).bind(account_id);
            for id in ids {
                q = q.bind(id);
            }
            q.fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    for (item_id, status) in rows {
        if let Some(entry) = facts.get_mut(&item_id) {
            entry.status = ReadingStatus::parse(&status);
        }
    }

    Ok(facts)
}

// ---------------------------------------------------------------------------
// Storage usage
// ---------------------------------------------------------------------------

/// What the reader's library occupies, in bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageUsage {
    /// Bytes of imported copies, counted once per blob.
    pub imported_bytes: i64,
    /// Bytes of export artifacts.
    pub export_bytes: i64,
    /// The two added together.
    pub total_bytes: i64,
    /// How many library items the account has.
    pub item_count: i64,
    /// How many distinct stored blobs the imports reference.
    pub blob_count: i64,
}

/// Measure the account's storage.
///
/// **Physical, and deduplicated.** A blob referenced by two of the reader's
/// items is counted once, because that is what the instance actually stores —
/// so this can be less than the sum of the sizes the reader thinks each item is.
/// Reporting the deduplicated figure is the honest one for a "free up space"
/// action, which is what spec §14.1 asks the number for.
///
/// # Errors
///
/// Returns an error if either statement fails.
pub async fn storage_usage(db: &Database, account_id: &str) -> Result<StorageUsage> {
    let imported_sql = db.sql(
        "SELECT COALESCE(CAST(SUM(b.byte_size) AS BIGINT), 0), COUNT(*) FROM content_blobs b \
         WHERE b.checksum IN (SELECT cr.checksum FROM content_references cr \
             JOIN library_items li ON li.id = cr.owner_id \
             WHERE cr.owner_type = 'library_item' AND li.account_id = ?)",
        // `::bigint` because PostgreSQL's SUM over a bigint is NUMERIC, which
        // sqlx will not decode into an i64. SQLite sums integers to an integer.
        "SELECT COALESCE(SUM(b.byte_size), 0)::bigint, COUNT(*) FROM content_blobs b \
         WHERE b.checksum IN (SELECT cr.checksum FROM content_references cr \
             JOIN library_items li ON li.id::text = cr.owner_id \
             WHERE cr.owner_type = 'library_item' AND li.account_id::text = ?)",
    );
    let export_sql = db.sql(
        "SELECT COALESCE(CAST(SUM(output_bytes) AS BIGINT), 0) FROM export_jobs \
         WHERE account_id = ? AND output_bytes IS NOT NULL",
        "SELECT COALESCE(SUM(output_bytes), 0)::bigint FROM export_jobs \
         WHERE account_id::text = ? AND output_bytes IS NOT NULL",
    );
    let items_sql = db.sql(
        "SELECT COUNT(*) FROM library_items WHERE account_id = ?",
        "SELECT COUNT(*) FROM library_items WHERE account_id::text = ?",
    );

    let (imported_bytes, blob_count) = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, (i64, i64)>(&imported_sql)
                .bind(account_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, (i64, i64)>(&imported_sql)
                .bind(account_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let export_bytes: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&export_sql)
                .bind(account_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&export_sql)
                .bind(account_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let item_count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&items_sql)
                .bind(account_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&items_sql)
                .bind(account_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    Ok(StorageUsage {
        imported_bytes,
        export_bytes,
        total_bytes: imported_bytes + export_bytes,
        item_count,
        blob_count,
    })
}

/// The stored size of each named blob, in bytes.
///
/// Read *before* a deletion, because the row carrying the size is the row the
/// deletion removes. A checksum the table does not hold is simply absent from
/// the answer, which the caller treats as zero.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn blob_sizes(db: &Database, checksums: &[String]) -> Result<Vec<(String, i64)>> {
    if checksums.is_empty() {
        return Ok(Vec::new());
    }
    let query = format!(
        "SELECT checksum, byte_size FROM content_blobs WHERE checksum IN ({})",
        vec!["?"; checksums.len()].join(", ")
    );
    let sql = db.sql(&query, &query);
    let rows: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query_as(&sql);
            for checksum in checksums {
                q = q.bind(checksum);
            }
            q.fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let mut q = sqlx::query_as(&sql);
            for checksum in checksums {
                q = q.bind(checksum);
            }
            q.fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Update checks
// ---------------------------------------------------------------------------

/// One recorded check of a library item against its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheck {
    /// Identifier.
    pub id: String,
    /// The owning account.
    pub account_id: String,
    /// The item that was checked.
    pub library_item_id: String,
    /// When, RFC 3339.
    pub checked_at: String,
    /// How many differences the check found.
    pub found_changes: i64,
    /// The detail behind that number.
    pub report_json: String,
}

#[derive(FromRow)]
struct UpdateCheckRow {
    id: String,
    account_id: String,
    library_item_id: String,
    checked_at: String,
    found_changes: i64,
    report_json: String,
}

const UPDATE_CHECK_COLUMNS: &str = "id, account_id, library_item_id, checked_at, \
     found_changes, report_json";

// The UUID columns are cast to text for the two `String` fields; `checked_at` is already canonical TEXT and
// `found_changes` is BIGINT, so neither needs anything — casting a timestamp that is *stored* as text with a time-zone
// expression is not a portability fix but a bug, and the PostgreSQL run is what says which columns are which.
const UPDATE_CHECK_COLUMNS_PG: &str = "id::text AS id, account_id::text AS account_id, \
     library_item_id::text AS library_item_id, checked_at, found_changes, report_json";

fn decode_update_check(row: UpdateCheckRow) -> UpdateCheck {
    UpdateCheck {
        id: row.id,
        account_id: row.account_id,
        library_item_id: row.library_item_id,
        checked_at: row.checked_at,
        found_changes: row.found_changes,
        report_json: row.report_json,
    }
}

/// Record what a check found.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn record_update_check(
    db: &Database,
    account_id: &str,
    library_item_id: &str,
    found_changes: i64,
    report_json: &str,
) -> Result<UpdateCheck> {
    let now = now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let sql = db.sql(
        "INSERT INTO update_checks (id, account_id, library_item_id, checked_at, found_changes, \
         report_json) VALUES (?, ?, ?, ?, ?, ?)",
        "INSERT INTO update_checks (id, account_id, library_item_id, checked_at, found_changes, \
         report_json) VALUES (?::uuid, ?::uuid, ?::uuid, ?, ?, ?)",
    );
    run!(db, sql, |q| q
        .bind(&id)
        .bind(account_id)
        .bind(library_item_id)
        .bind(&now)
        .bind(found_changes)
        .bind(report_json))
    .await?;

    Ok(UpdateCheck {
        id,
        account_id: account_id.to_string(),
        library_item_id: library_item_id.to_string(),
        checked_at: now,
        found_changes,
        report_json: report_json.to_string(),
    })
}

/// The most recent check of one item.
pub async fn latest_update_check(
    db: &Database,
    account_id: &str,
    library_item_id: &str,
) -> Result<Option<UpdateCheck>> {
    let sql = sql_owned(
        db,
        format!(
            "SELECT {UPDATE_CHECK_COLUMNS} FROM update_checks \
             WHERE account_id = ? AND library_item_id = ? \
             ORDER BY checked_at DESC, id DESC LIMIT 1"
        ),
        format!(
            "SELECT {UPDATE_CHECK_COLUMNS_PG} FROM update_checks \
             WHERE account_id::text = ? AND library_item_id::text = ? \
             ORDER BY checked_at DESC, id DESC LIMIT 1"
        ),
    );
    let row: Option<UpdateCheckRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(library_item_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(library_item_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(row.map(decode_update_check))
}

/// Checks recorded at or after a time, newest first.
pub async fn update_checks_since(
    db: &Database,
    account_id: &str,
    since: &str,
) -> Result<Vec<UpdateCheck>> {
    let sql = sql_owned(
        db,
        format!(
            "SELECT {UPDATE_CHECK_COLUMNS} FROM update_checks \
             WHERE account_id = ? AND checked_at >= ? ORDER BY checked_at DESC, id DESC"
        ),
        format!(
            "SELECT {UPDATE_CHECK_COLUMNS_PG} FROM update_checks \
             WHERE account_id::text = ? AND checked_at >= ? ORDER BY checked_at DESC, id DESC"
        ),
    );
    let rows: Vec<UpdateCheckRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(since)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .bind(since)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(decode_update_check).collect())
}

/// Delete checks older than the retention cutoff.
///
/// Returns how many rows went. A sweep, not a trigger, so an instance that stops
/// sweeping accumulates rows rather than losing them.
///
/// # Errors
///
/// Returns an error if the statement fails.
pub async fn sweep_update_checks(db: &Database, cutoff: &str) -> Result<u64> {
    let sql = db.sql(
        "DELETE FROM update_checks WHERE checked_at < ?",
        "DELETE FROM update_checks WHERE checked_at < ?",
    );
    let affected = run!(db, sql, |q| q.bind(cutoff)).await?;
    Ok(affected)
}

// ---------------------------------------------------------------------------
// Removing library items
// ---------------------------------------------------------------------------

/// What a batch removal did, and which stored blobs it orphaned.
#[derive(Debug, Clone)]
pub struct RemovalOutcome {
    /// Per-item result, in the shape spec §14.3 fixes.
    pub outcome: BatchOutcome,
    /// Blobs that no longer have any reference, for the caller to delete.
    ///
    /// Empty when the caller asked to keep the copies. This module never deletes
    /// a file itself.
    pub orphaned_checksums: Vec<String>,
}

/// Remove library items, optionally with the imported copies they hold.
///
/// # The two operations are different, and the caller says which
///
/// Removing an item from a library is not the same as deleting what was imported
/// for it (spec §14's acceptance list, "Removing an item distinguishes deleting a
/// reference from deleting a private copy"). The item goes, and the references it
/// held go with it, always — an item that no longer exists cannot own a stored
/// unit, and a reference left pointing at a deleted row is one no sweep will
/// ever match.
///
/// What `delete_copy` decides is **when the bytes follow**:
///
/// * `true` — now. Every blob that lost its last reference is reported in
///   [`RemovalOutcome::orphaned_checksums`] so the caller can delete the file
///   immediately, and report the space freed.
/// * `false` — later, by the maintenance sweep that collects unreferenced
///   blobs. Nothing is reported, because nothing was freed *now*; the data is
///   already unreachable and the collector will find it on its own terms.
///
/// # Errors
///
/// Returns an error if a statement fails.
pub async fn remove_library_items(
    db: &Database,
    account_id: &str,
    ids: &[String],
    delete_copy: bool,
) -> Result<RemovalOutcome> {
    if ids.is_empty() {
        return Ok(RemovalOutcome {
            outcome: BatchOutcome::default(),
            orphaned_checksums: Vec::new(),
        });
    }

    // Which of the requested ids this account actually owns. One query, so the
    // answer cannot change between checking and deleting.
    let owned_sqlite = format!(
        "SELECT id FROM library_items WHERE account_id = ? AND id IN ({})",
        placeholders(ids.len(), false)
    );
    let owned_postgres = format!(
        "SELECT id::text FROM library_items WHERE account_id::text = ? AND id::text IN ({})",
        placeholders(ids.len(), false)
    );
    let owned_sql = db.sql(&owned_sqlite, &owned_postgres);
    let owned: Vec<String> = match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query_scalar(&owned_sql).bind(account_id);
            for id in ids {
                q = q.bind(id);
            }
            q.fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let mut q = sqlx::query_scalar(&owned_sql).bind(account_id);
            for id in ids {
                q = q.bind(id);
            }
            q.fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    // A requested id the account does not own is reported as gone rather than
    // as forbidden: one code for both, because distinguishing them would confirm
    // which identifiers exist.
    let failures: Vec<BatchFailure> = ids
        .iter()
        .filter(|id| !owned.iter().any(|o| o == *id))
        .map(BatchFailure::gone)
        .collect();
    if owned.is_empty() {
        return Ok(RemovalOutcome {
            outcome: BatchOutcome::from_parts(Vec::new(), failures),
            orphaned_checksums: Vec::new(),
        });
    }

    // The item's references go with it, always.
    //
    // An item that no longer exists cannot own a stored unit, and a reference
    // left pointing at a deleted row is one no sweep will ever match — it holds
    // the blob alive for ever while nothing can reach it. So the references are
    // dropped either way, and `delete_copy` decides *when* the bytes follow:
    // now, or later, by the maintenance sweep that collects unreferenced blobs.
    // The checksums the items held, read before the references go.
    let refs_query = format!(
        "SELECT checksum FROM content_references \
         WHERE owner_type = 'library_item' AND owner_id IN ({})",
        vec!["?"; owned.len()].join(", ")
    );
    let refs_sql = db.sql(&refs_query, &refs_query);
    let mut touched_checksums: Vec<String> = match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query_scalar(&refs_sql);
            for id in &owned {
                q = q.bind(id);
            }
            q.fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let mut q = sqlx::query_scalar(&refs_sql);
            for id in &owned {
                q = q.bind(id);
            }
            q.fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    // And the references themselves, always.
    let drop_refs_query = format!(
        "DELETE FROM content_references \
         WHERE owner_type = 'library_item' AND owner_id IN ({})",
        vec!["?"; owned.len()].join(", ")
    );
    let drop_refs_sql = db.sql(&drop_refs_query, &drop_refs_query);
    match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(&drop_refs_sql);
            for id in &owned {
                q = q.bind(id);
            }
            q.execute(db.sqlite_pool().expect("sqlite handle")).await?;
        }
        Backend::Postgres => {
            let mut q = sqlx::query(&drop_refs_sql);
            for id in &owned {
                q = q.bind(id);
            }
            q.execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }

    let delete_sqlite = format!(
        "DELETE FROM library_items WHERE account_id = ? AND id IN ({})",
        placeholders(owned.len(), false)
    );
    let delete_postgres = format!(
        "DELETE FROM library_items WHERE account_id::text = ? AND id::text IN ({})",
        placeholders(owned.len(), false)
    );
    let delete_sql = db.sql(&delete_sqlite, &delete_postgres);
    match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(&delete_sql).bind(account_id);
            for id in &owned {
                q = q.bind(id);
            }
            q.execute(db.sqlite_pool().expect("sqlite handle")).await?;
        }
        Backend::Postgres => {
            let mut q = sqlx::query(&delete_sql).bind(account_id);
            for id in &owned {
                q = q.bind(id);
            }
            q.execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }

    // Which of the checksums lost their last reference. Asked *after* the
    // deletion so the answer is about the state that now exists. Only the
    // immediate mode reports them: with `delete_copy` false the bytes are left
    // exactly as they are, for the collector to find on its own terms.
    let mut orphaned: Vec<String> = Vec::new();
    if delete_copy && !touched_checksums.is_empty() {
        touched_checksums.sort();
        touched_checksums.dedup();
        let remaining_sql = db.sql(
            "SELECT COUNT(*) FROM content_references WHERE checksum = ?",
            "SELECT COUNT(*) FROM content_references WHERE checksum = ?",
        );
        for checksum in &touched_checksums {
            let remaining: i64 = match db.backend() {
                Backend::Sqlite => {
                    sqlx::query_scalar(&remaining_sql)
                        .bind(checksum)
                        .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                        .await?
                }
                Backend::Postgres => {
                    sqlx::query_scalar(&remaining_sql)
                        .bind(checksum)
                        .fetch_one(db.postgres_pool().expect("postgres handle"))
                        .await?
                }
            };
            if remaining == 0 {
                orphaned.push(checksum.clone());
            }
        }
    }

    Ok(RemovalOutcome {
        outcome: BatchOutcome::from_parts(owned, failures),
        orphaned_checksums: orphaned,
    })
}

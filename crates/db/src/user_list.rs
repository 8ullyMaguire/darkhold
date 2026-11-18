//! Row 413 — the user-list service, adapted from gravity's §13.2
//! (`legacy/backend/crates/db/src/services/user_list.rs`).
//!
//! The shape is gravity's: one list, its items, its collaborators, and a
//! visibility ladder. Darkhold's noun is a reading list, so an *item* is a work
//! rather than a user; ownership stays the account's, exactly as gravity's
//! `owner_id` does. There is no `reading_lists` table and no second ownership
//! model: the tables are gravity's `user_lists`, `user_list_members` and
//! `user_list_collaborators`.
//!
//! Every gate lives here, not in a route: [`can_read`] decides visibility from
//! a collaborator row the store reads, and [`require_mutation`] decides
//! mutations from the same row through the moderation ladder. Routes only
//! authenticate the caller and render the result.

use darkhold_domain::user_list::{
    AddUserListCollaboratorRequest, AddUserListItemRequest, CreateUserListRequest, ListModeration,
    ListVisibility, UpdateUserListRequest, UserList, UserListCollaborator, UserListItem,
    UserListView,
};
use sqlx::Row;

use crate::{Backend, Database};

/// The service's refusals. A private list that the caller may not read is
/// `NotFound`, never `Forbidden`: the status code must not disclose existence.
#[derive(Debug, thiserror::Error)]
pub enum UserListError {
    #[error("list not found")]
    NotFound,
    #[error("access denied")]
    Forbidden,
    #[error("{0}")]
    Validation(String),
    #[error("database error: {0}")]
    Fault(#[from] sqlx::Error),
}

pub type Result<T> = std::result::Result<T, UserListError>;

type ListRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
);

fn decode_list(row: ListRow) -> UserList {
    let (id, owner_id, name, description, visibility, moderation, criteria, created_at, updated_at) =
        row;
    UserList {
        id,
        owner_id,
        name,
        description,
        visibility,
        moderation,
        criteria,
        created_at,
        updated_at,
    }
}

fn clamp_limit(limit: i64) -> i64 {
    limit.clamp(1, 200)
}

fn now() -> String {
    crate::identity::now_rfc3339()
}

/// `Some(role)` when the account collaborates on the list. One query, shared
/// by the read rule and the mutation rule so they cannot disagree.
async fn collaborator_role(db: &Database, list_id: &str, user_id: &str) -> Result<Option<String>> {
    let statement = db.sql(
        "SELECT role FROM user_list_collaborators WHERE list_id = ? AND user_id = ?",
        "SELECT role FROM user_list_collaborators WHERE list_id = $1::uuid AND user_id = $2::uuid",
    );
    Ok(match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&statement)
                .bind(list_id)
                .bind(user_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&statement)
                .bind(list_id)
                .bind(user_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    })
}

/// The visibility rule, from the stored row and one service-owned role lookup.
async fn can_read(db: &Database, list: &UserList, viewer: Option<&str>) -> Result<bool> {
    let is_collaborator = match viewer {
        Some(v) => collaborator_role(db, &list.id, v).await?.is_some(),
        None => false,
    };
    Ok(list
        .visibility_kind()
        .can_read(viewer, &list.owner_id, is_collaborator))
}

/// The mutation rule. `chosen_mods` consults the same collaborator row;
/// `public_quorum` refuses everybody, including the owner.
async fn require_mutation(db: &Database, list: &UserList, actor_id: &str) -> Result<()> {
    let is_moderator = matches!(
        collaborator_role(db, &list.id, actor_id).await?.as_deref(),
        Some("moderator")
    );
    if list
        .moderation_kind()
        .mutation_allowed(actor_id, &list.owner_id, is_moderator)
    {
        Ok(())
    } else {
        Err(UserListError::Forbidden)
    }
}

async fn load(db: &Database, id: &str) -> Result<UserList> {
    let statement = db.sql(
        "SELECT id, owner_id, name, description, visibility, moderation, criteria, created_at, updated_at \
         FROM user_lists WHERE id = ?",
        "SELECT id::text, owner_id::text, name, description, visibility, moderation, criteria, created_at, updated_at \
         FROM user_lists WHERE id = $1::uuid",
    );
    let row: Option<ListRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    row.map(decode_list).ok_or(UserListError::NotFound)
}

async fn work_exists(db: &Database, id: &str) -> Result<bool> {
    let statement = db.sql(
        "SELECT count(*) FROM works WHERE id = ?",
        "SELECT count(*) FROM works WHERE id = $1::uuid",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&statement)
                .bind(id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&statement)
                .bind(id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(count > 0)
}

async fn account_exists(db: &Database, id: &str) -> Result<bool> {
    let statement = db.sql(
        "SELECT count(*) FROM accounts WHERE id = ?",
        "SELECT count(*) FROM accounts WHERE id = $1::uuid",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&statement)
                .bind(id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&statement)
                .bind(id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(count > 0)
}

fn validate_name(name: &str) -> Result<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 120 {
        return Err(UserListError::Validation(
            "list name must be 1..=120 characters after trimming".to_owned(),
        ));
    }
    Ok(trimmed)
}

fn validate_visibility(value: Option<&str>) -> Result<ListVisibility> {
    match value {
        None => Ok(ListVisibility::Private),
        Some(v) => ListVisibility::parse(v)
            .ok_or_else(|| UserListError::Validation(format!("unknown visibility '{v}'"))),
    }
}

fn validate_moderation(value: Option<&str>) -> Result<ListModeration> {
    match value {
        None => Ok(ListModeration::SingleUser),
        Some(v) => ListModeration::parse(v)
            .ok_or_else(|| UserListError::Validation(format!("unknown moderation '{v}'"))),
    }
}

fn is_duplicate_name(error: &sqlx::Error) -> bool {
    let text = error.to_string();
    text.contains("UNIQUE constraint failed") || text.contains("user_lists_owner_id_name_key")
}

/// Create a list owned by `owner_id`. Name is trimmed and unique per owner.
pub async fn create(
    db: &Database,
    owner_id: &str,
    req: &CreateUserListRequest,
) -> Result<UserList> {
    let name = validate_name(&req.name)?;
    let visibility = validate_visibility(req.visibility.as_deref())?;
    let moderation = validate_moderation(req.moderation.as_deref())?;
    let criteria = req.criteria.as_ref().map(serde_json::Value::to_string);
    let id = uuid::Uuid::new_v4().to_string();
    let stamp = now();
    let statement = db.sql(
        "INSERT INTO user_lists (id, owner_id, name, description, visibility, moderation, criteria, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        "INSERT INTO user_lists (id, owner_id, name, description, visibility, moderation, criteria, created_at, updated_at) \
         VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6, $7, $8, $9)",
    );
    let written: std::result::Result<u64, sqlx::Error> = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(&id)
            .bind(owner_id)
            .bind(name)
            .bind(&req.description)
            .bind(visibility.as_str())
            .bind(moderation.as_str())
            .bind(&criteria)
            .bind(&stamp)
            .bind(&stamp)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await
            .map(|result| result.rows_affected()),
        Backend::Postgres => sqlx::query(&statement)
            .bind(&id)
            .bind(owner_id)
            .bind(name)
            .bind(&req.description)
            .bind(visibility.as_str())
            .bind(moderation.as_str())
            .bind(&criteria)
            .bind(&stamp)
            .bind(&stamp)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await
            .map(|result| result.rows_affected()),
    };
    match written {
        Ok(_) => load(db, &id).await,
        Err(error) if is_duplicate_name(&error) => Err(UserListError::Validation(
            "you already have a list with that name".to_owned(),
        )),
        Err(error) => Err(UserListError::Fault(error)),
    }
}

/// `None` for a missing OR unreadable list; a private list's existence is not
/// disclosed through this door.
pub async fn get_visible(
    db: &Database,
    id: &str,
    viewer: Option<&str>,
) -> Result<Option<UserListView>> {
    let list = match load(db, id).await {
        Ok(list) => list,
        Err(UserListError::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    if !can_read(db, &list, viewer).await? {
        return Ok(None);
    }
    let statement = db.sql(
        "SELECT count(*) FROM user_list_members WHERE list_id = ?",
        "SELECT count(*) FROM user_list_members WHERE list_id = $1::uuid",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&statement)
                .bind(id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&statement)
                .bind(id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let is_owner = viewer == Some(list.owner_id.as_str());
    Ok(Some(UserListView {
        list,
        item_count: count,
        is_owner,
    }))
}

/// Lists owned by `owner_id` that `viewer` may read. The collaborator set is
/// read through the same rule as a single list.
pub async fn list_by_owner(
    db: &Database,
    owner_id: &str,
    viewer: Option<&str>,
    limit: i64,
) -> Result<Vec<UserListView>> {
    let statement = db.sql(
        "SELECT id, owner_id, name, description, visibility, moderation, criteria, created_at, updated_at \
         FROM user_lists WHERE owner_id = ? ORDER BY created_at DESC LIMIT ?",
        "SELECT id::text, owner_id::text, name, description, visibility, moderation, criteria, created_at, updated_at \
         FROM user_lists WHERE owner_id = $1::uuid ORDER BY created_at DESC LIMIT $2",
    );
    let rows: Vec<ListRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(owner_id)
                .bind(clamp_limit(limit))
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(owner_id)
                .bind(clamp_limit(limit))
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let mut out = Vec::new();
    for row in rows {
        let list = decode_list(row);
        if can_read(db, &list, viewer).await? {
            out.push(UserListView {
                list,
                item_count: 0,
                is_owner: viewer == Some(owner_id),
            });
        }
    }
    Ok(out)
}

/// Update the list. Every field is optional; the permission comes from the
/// service's moderation rule, not from the route.
pub async fn update(
    db: &Database,
    id: &str,
    actor_id: &str,
    req: &UpdateUserListRequest,
) -> Result<UserList> {
    let list = load(db, id).await?;
    require_mutation(db, &list, actor_id).await?;
    let name = match req.name.as_deref() {
        Some(v) => validate_name(v)?,
        None => &list.name,
    };
    let description = req.description.as_deref().unwrap_or(&list.description);
    let visibility = match req.visibility.as_deref() {
        Some(v) => validate_visibility(Some(v))?,
        None => list.visibility_kind(),
    };
    let moderation = match req.moderation.as_deref() {
        Some(v) => validate_moderation(Some(v))?,
        None => list.moderation_kind(),
    };
    let statement = db.sql(
        "UPDATE user_lists SET name = ?, description = ?, visibility = ?, moderation = ?, updated_at = ? WHERE id = ?",
        "UPDATE user_lists SET name = $2, description = $3, visibility = $4, moderation = $5, updated_at = $6 WHERE id = $1::uuid",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&statement)
                .bind(name)
                .bind(description)
                .bind(visibility.as_str())
                .bind(moderation.as_str())
                .bind(now())
                .bind(id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&statement)
                .bind(id)
                .bind(name)
                .bind(description)
                .bind(visibility.as_str())
                .bind(moderation.as_str())
                .bind(now())
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    load(db, id).await
}

/// Delete the list. The three child tables carry `ON DELETE CASCADE`, so one
/// DELETE here removes every item, collaborator and their history.
pub async fn delete(db: &Database, id: &str, actor_id: &str) -> Result<()> {
    let list = load(db, id).await?;
    require_mutation(db, &list, actor_id).await?;
    let statement = db.sql(
        "DELETE FROM user_lists WHERE id = ?",
        "DELETE FROM user_lists WHERE id = $1::uuid",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&statement)
                .bind(id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&statement)
                .bind(id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

async fn load_item(db: &Database, list_id: &str, work_id: &str) -> Result<UserListItem> {
    let statement = db.sql(
        "SELECT list_id, work_id, added_by, note, created_at FROM user_list_members \
         WHERE list_id = ? AND work_id = ?",
        "SELECT list_id::text, work_id::text, added_by::text, note, created_at FROM user_list_members \
         WHERE list_id = $1::uuid AND work_id = $2::uuid",
    );
    let row: Option<(String, String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(list_id)
                .bind(work_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(list_id)
                .bind(work_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    row.map(
        |(list_id, work_id, added_by, note, created_at)| UserListItem {
            list_id,
            work_id,
            added_by,
            note,
            created_at,
        },
    )
    .ok_or(UserListError::NotFound)
}

/// Add a work to the list, idempotently: a second add refreshes the note but
/// is not a second item.
pub async fn add_item(
    db: &Database,
    id: &str,
    actor_id: &str,
    req: &AddUserListItemRequest,
) -> Result<UserListItem> {
    let list = load(db, id).await?;
    require_mutation(db, &list, actor_id).await?;
    if !work_exists(db, &req.work_id).await? {
        return Err(UserListError::Validation("work not found".to_owned()));
    }
    let statement = db.sql(
        "INSERT INTO user_list_members (list_id, work_id, added_by, note, created_at) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT (list_id, work_id) DO UPDATE SET added_by = EXCLUDED.added_by, note = EXCLUDED.note",
        "INSERT INTO user_list_members (list_id, work_id, added_by, note, created_at) \
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4, $5) \
         ON CONFLICT (list_id, work_id) DO UPDATE SET added_by = EXCLUDED.added_by, note = EXCLUDED.note",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&statement)
                .bind(id)
                .bind(&req.work_id)
                .bind(actor_id)
                .bind(&req.note)
                .bind(now())
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&statement)
                .bind(id)
                .bind(&req.work_id)
                .bind(actor_id)
                .bind(&req.note)
                .bind(now())
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    load_item(db, id, &req.work_id).await
}

pub async fn remove_item(db: &Database, id: &str, actor_id: &str, work_id: &str) -> Result<()> {
    let list = load(db, id).await?;
    require_mutation(db, &list, actor_id).await?;
    let statement = db.sql(
        "DELETE FROM user_list_members WHERE list_id = ? AND work_id = ?",
        "DELETE FROM user_list_members WHERE list_id = $1::uuid AND work_id = $2::uuid",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(id)
            .bind(work_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(id)
            .bind(work_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    if affected == 0 {
        return Err(UserListError::NotFound);
    }
    Ok(())
}

/// The works on the list, gated by the same visibility rule as the list read.
pub async fn items(
    db: &Database,
    id: &str,
    viewer: Option<&str>,
    limit: i64,
) -> Result<Vec<UserListItem>> {
    let list = load(db, id).await?;
    if !can_read(db, &list, viewer).await? {
        return Err(UserListError::NotFound);
    }
    let statement = db.sql(
        "SELECT list_id, work_id, added_by, note, created_at FROM user_list_members \
         WHERE list_id = ? ORDER BY created_at ASC LIMIT ?",
        "SELECT list_id::text, work_id::text, added_by::text, note, created_at FROM user_list_members \
         WHERE list_id = $1::uuid ORDER BY created_at ASC LIMIT $2",
    );
    let rows: Vec<(String, String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(id)
                .bind(clamp_limit(limit))
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(id)
                .bind(clamp_limit(limit))
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(list_id, work_id, added_by, note, created_at)| UserListItem {
                list_id,
                work_id,
                added_by,
                note,
                created_at,
            },
        )
        .collect())
}

async fn load_collaborator(
    db: &Database,
    list_id: &str,
    user_id: &str,
) -> Result<UserListCollaborator> {
    let statement = db.sql(
        "SELECT list_id, user_id, role, created_at FROM user_list_collaborators \
         WHERE list_id = ? AND user_id = ?",
        "SELECT list_id::text, user_id::text, role, created_at FROM user_list_collaborators \
         WHERE list_id = $1::uuid AND user_id = $2::uuid",
    );
    let row: Option<(String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(list_id)
                .bind(user_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(list_id)
                .bind(user_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    row.map(
        |(list_id, user_id, role, created_at)| UserListCollaborator {
            list_id,
            user_id,
            role,
            created_at,
        },
    )
    .ok_or(UserListError::NotFound)
}

/// Grant curation rights on the list. Only somebody who may mutate the list
/// can grant them, so a curator cannot promote themselves to moderator even
/// under `chosen_mods`.
pub async fn add_collaborator(
    db: &Database,
    id: &str,
    actor_id: &str,
    req: &AddUserListCollaboratorRequest,
) -> Result<UserListCollaborator> {
    let list = load(db, id).await?;
    require_mutation(db, &list, actor_id).await?;
    if req.role != "curator" && req.role != "moderator" {
        return Err(UserListError::Validation(format!(
            "unknown collaborator role '{}'",
            req.role
        )));
    }
    if !account_exists(db, &req.user_id).await? {
        return Err(UserListError::Validation("account not found".to_owned()));
    }
    let statement = db.sql(
        "INSERT INTO user_list_collaborators (list_id, user_id, role, created_at) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT (list_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        "INSERT INTO user_list_collaborators (list_id, user_id, role, created_at) \
         VALUES ($1::uuid, $2::uuid, $3, $4) \
         ON CONFLICT (list_id, user_id) DO UPDATE SET role = EXCLUDED.role",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&statement)
                .bind(id)
                .bind(&req.user_id)
                .bind(&req.role)
                .bind(now())
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&statement)
                .bind(id)
                .bind(&req.user_id)
                .bind(&req.role)
                .bind(now())
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    load_collaborator(db, id, &req.user_id).await
}

/// The collaborators, readable under the same rule as the list itself.
pub async fn collaborators(
    db: &Database,
    id: &str,
    viewer: Option<&str>,
    limit: i64,
) -> Result<Vec<UserListCollaborator>> {
    let list = load(db, id).await?;
    if !can_read(db, &list, viewer).await? {
        return Err(UserListError::NotFound);
    }
    let statement = db.sql(
        "SELECT list_id, user_id, role, created_at FROM user_list_collaborators \
         WHERE list_id = ? ORDER BY created_at ASC LIMIT ?",
        "SELECT list_id::text, user_id::text, role, created_at FROM user_list_collaborators \
         WHERE list_id = $1::uuid ORDER BY created_at ASC LIMIT $2",
    );
    let rows: Vec<(String, String, String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&statement)
                .bind(id)
                .bind(clamp_limit(limit))
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&statement)
                .bind(id)
                .bind(clamp_limit(limit))
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(list_id, user_id, role, created_at)| UserListCollaborator {
                list_id,
                user_id,
                role,
                created_at,
            },
        )
        .collect())
}

pub async fn remove_collaborator(
    db: &Database,
    id: &str,
    actor_id: &str,
    user_id: &str,
) -> Result<()> {
    let list = load(db, id).await?;
    require_mutation(db, &list, actor_id).await?;
    let statement = db.sql(
        "DELETE FROM user_list_collaborators WHERE list_id = ? AND user_id = ?",
        "DELETE FROM user_list_collaborators WHERE list_id = $1::uuid AND user_id = $2::uuid",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&statement)
            .bind(id)
            .bind(user_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&statement)
            .bind(id)
            .bind(user_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    if affected == 0 {
        return Err(UserListError::NotFound);
    }
    Ok(())
}

/// Count a list's children. Used by the cascade gate's test, and by no route:
/// private so the orphan-query ratchet stays quiet.
#[allow(dead_code)]
async fn child_counts(db: &Database, id: &str) -> Result<(i64, i64)> {
    let statement = db.sql(
        "SELECT (SELECT count(*) FROM user_list_members WHERE list_id = ?), \
                (SELECT count(*) FROM user_list_collaborators WHERE list_id = ?)",
        "SELECT (SELECT count(*) FROM user_list_members WHERE list_id = $1::uuid), \
                (SELECT count(*) FROM user_list_collaborators WHERE list_id = $1::uuid)",
    );
    Ok(match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&statement)
                .bind(id)
                .bind(id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?;
            (row.get::<i64, _>(0), row.get::<i64, _>(1))
        }
        Backend::Postgres => {
            let row = sqlx::query(&statement)
                .bind(id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?;
            (row.get::<i64, _>(0), row.get::<i64, _>(1))
        }
    })
}

/// The public lists that contain one work — the `story.lists_containing` candidate pipeline.
///
/// Only `visibility = 'public'` rows are returned: a private list is not a discovery signal for a reader who cannot see it, and
/// returning its name (or even a count) would disclose that it exists. The pair of tables already carries the relationship, so
/// this is a projection, not a new model.
pub async fn lists_containing(db: &Database, work_id: &str, limit: i64) -> Result<Vec<UserList>> {
    let limit = clamp_limit(limit);
    let sqlite =
        "SELECT ul.id, ul.owner_id, ul.name, ul.description, ul.visibility, ul.moderation, \
                         ul.criteria, ul.created_at, ul.updated_at \
                    FROM user_list_members m JOIN user_lists ul ON ul.id = m.list_id \
                   WHERE m.work_id = ? AND ul.visibility = 'public' \
                   ORDER BY ul.created_at DESC, ul.name ASC LIMIT ?";
    let postgres =
        "SELECT ul.id::text, ul.owner_id::text, ul.name, ul.description, ul.visibility, \
                           ul.moderation, ul.criteria, ul.created_at::text, ul.updated_at::text \
                      FROM user_list_members m JOIN user_lists ul ON ul.id = m.list_id \
                     WHERE m.work_id = $1::uuid AND ul.visibility = 'public' \
                     ORDER BY ul.created_at DESC, ul.name ASC LIMIT $2";
    let rows: Vec<ListRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<sqlx::Sqlite, _>(sqlite)
                .bind(work_id)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<sqlx::Postgres, _>(postgres)
                .bind(work_id)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(decode_list).collect())
}

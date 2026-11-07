//! A tag script: one kept set of assertions, applied while browsing (M69-20).
//!
//! Tagging a work one form at a time is a chore, and chores get done in batches. A reader working through forty works of one
//! fandom knows twenty of them share three tags; typing those three tags forty times is why the tag is still missing.
//!
//! It is a **set of assertions**, not a saved filter, and that is the design decision: applying a script calls
//! `tag_assertions::assert_tag` with `Claimant::Human`, so the row records a person as its claimant and everything M69-17
//! established holds — a script cannot write an inference, and a script cannot overwrite a row somebody else stands behind.
//!
//! Ownership decides `curator` versus `author`: applying a script to your own work writes `author`, to somebody else's writes
//! `curator`. That is read from the work's owner rather than passed in, because the caller who forgot would write the wrong claim.

use crate::{
    identity::now_rfc3339,
    tag_assertions::{assert_tag, Claimant},
    Backend, Database, Result,
};
use serde::Serialize;

/// One script, with its nodes.
#[derive(Debug, Clone, Serialize)]
pub struct TagScript {
    /// Identifier.
    pub id: String,
    /// Display name, unique per account.
    pub name: String,
    /// Taxonomy nodes, in the order the reader put them.
    pub node_ids: Vec<String>,
    /// When it was last changed, RFC 3339.
    pub updated_at: String,
}

/// What applying a script did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ApplyReport {
    /// How many tags the work now carries from this script.
    pub applied: i64,
    /// How many nodes were silently already there — counted, so "nothing happened" is distinguishable from "the script is empty".
    pub already_present: i64,
}

/// Make a script. An empty script is allowed: a reader who names a set before filling it has not made a mistake.
pub async fn create_script(db: &Database, account: &str, name: &str) -> Result<TagScript> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("a tag script needs a name");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let sql = db.sql(
        "INSERT INTO tag_scripts (id, account_id, name, created_at, updated_at, version)
         VALUES (?1, ?2, ?3, ?4, ?4, 1)",
        "INSERT INTO tag_scripts (id, account_id, name, created_at, updated_at, version)
         VALUES ($1::uuid, $2::uuid, $3, $4, $4, 1)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(account)
                .bind(name)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(account)
                .bind(name)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(TagScript {
        id,
        name: name.to_owned(),
        node_ids: Vec::new(),
        updated_at: now,
    })
}

/// Add a node to a script. Idempotent: adding what is already there changes nothing and is not an error.
pub async fn add_node(
    db: &Database,
    account: &str,
    script_id: &str,
    node_id: &str,
) -> Result<bool> {
    if !owns(db, account, script_id).await? {
        return Ok(false);
    }
    let sql = db.sql(
        "INSERT INTO tag_script_nodes (script_id, node_id, position)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(position), -1) + 1 FROM tag_script_nodes WHERE script_id = ?1))
         ON CONFLICT (script_id, node_id) DO NOTHING",
        "INSERT INTO tag_script_nodes (script_id, node_id, position)
         VALUES ($1::uuid, $2, (SELECT COALESCE(MAX(position), -1) + 1 FROM tag_script_nodes WHERE script_id = $1::uuid))
         ON CONFLICT (script_id, node_id) DO NOTHING",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(script_id)
            .bind(node_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(script_id)
            .bind(node_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    touch(db, script_id).await?;
    Ok(affected > 0)
}

/// Remove a node from a script. Returns whether there was one.
pub async fn remove_node(
    db: &Database,
    account: &str,
    script_id: &str,
    node_id: &str,
) -> Result<bool> {
    if !owns(db, account, script_id).await? {
        return Ok(false);
    }
    let sql = db.sql(
        "DELETE FROM tag_script_nodes WHERE script_id = ?1 AND node_id = ?2",
        "DELETE FROM tag_script_nodes WHERE script_id = $1::uuid AND node_id = $2",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(script_id)
            .bind(node_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(script_id)
            .bind(node_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    touch(db, script_id).await?;
    Ok(affected > 0)
}

/// The account's scripts, with their nodes.
pub async fn scripts_for(db: &Database, account: &str) -> Result<Vec<TagScript>> {
    let sql = db.sql(
        "SELECT CAST(s.id AS TEXT), s.name, s.updated_at,
                (SELECT GROUP_CONCAT(n.node_id, ',') FROM (
                    SELECT node_id FROM tag_script_nodes WHERE script_id = s.id ORDER BY position, node_id
                ) n)
           FROM tag_scripts s WHERE s.account_id = ?1 ORDER BY s.name",
        "SELECT s.id::text, s.name, s.updated_at,
                (SELECT string_agg(n.node_id, ',' ORDER BY n.position, n.node_id)
                   FROM tag_script_nodes n WHERE n.script_id = s.id)
           FROM tag_scripts s WHERE s.account_id = $1::uuid ORDER BY s.name",
    );
    let rows: Vec<(String, String, String, Option<String>)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(id, name, updated_at, joined)| TagScript {
            id,
            name,
            node_ids: joined
                .filter(|value| !value.is_empty())
                .map(|value| value.split(',').map(str::to_owned).collect())
                .unwrap_or_default(),
            updated_at,
        })
        .collect())
}

/// Apply a script to a work: every node in it becomes an assertion by the caller.
///
/// Returns `None` when the script is not the caller's — a script belongs to the reader who made it, and applying somebody else's
/// set would attribute their judgement to nobody.
pub async fn apply_to_work(
    db: &Database,
    account: &str,
    script_id: &str,
    work_id: &str,
) -> Result<Option<ApplyReport>> {
    if !owns(db, account, script_id).await? {
        return Ok(None);
    }
    let nodes = nodes_of(db, script_id).await?;
    if nodes.is_empty() {
        return Ok(Some(ApplyReport {
            applied: 0,
            already_present: 0,
        }));
    }
    // Whose work it is decides the claim: `author` on your own, `curator` on somebody else's. Read here rather than trusted from
    // the caller, because a caller who guesses writes the wrong claim and the row outlives the request.
    let owned = owned_by(db, work_id, account).await?;
    let mut applied = 0;
    let mut already_present = 0;
    for node in &nodes {
        let existed = has_tag(db, work_id, node).await?;
        assert_tag(db, work_id, node, Claimant::Human, owned, 1.0).await?;
        if existed {
            already_present += 1;
        } else {
            applied += 1;
        }
    }
    Ok(Some(ApplyReport {
        applied,
        already_present,
    }))
}

async fn nodes_of(db: &Database, script_id: &str) -> Result<Vec<String>> {
    let sql = db.sql(
        "SELECT node_id FROM tag_script_nodes WHERE script_id = ?1 ORDER BY position, node_id",
        "SELECT node_id FROM tag_script_nodes WHERE script_id = $1::uuid ORDER BY position, node_id",
    );
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(script_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(script_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(|(node,)| node).collect())
}

async fn owns(db: &Database, account: &str, script_id: &str) -> Result<bool> {
    let sql = db.sql(
        "SELECT COUNT(*) FROM tag_scripts WHERE id = ?1 AND account_id = ?2",
        "SELECT COUNT(*) FROM tag_scripts WHERE id = $1::uuid AND account_id = $2::uuid",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(script_id)
                .bind(account)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(script_id)
                .bind(account)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(count > 0)
}

async fn owned_by(db: &Database, work_id: &str, account: &str) -> Result<bool> {
    let sql = db.sql(
        "SELECT COUNT(*) FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id
          WHERE w.id = ?1 AND p.account_id = ?2",
        "SELECT COUNT(*) FROM works w JOIN pseuds p ON p.id = w.owner_pseud_id
          WHERE w.id = $1::uuid AND p.account_id = $2::uuid",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(account)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(account)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(count > 0)
}

async fn has_tag(db: &Database, work_id: &str, node_id: &str) -> Result<bool> {
    let sql = db.sql(
        "SELECT COUNT(*) FROM work_tags WHERE work_id = ?1 AND node_id = ?2",
        "SELECT COUNT(*) FROM work_tags WHERE work_id = $1::uuid AND node_id = $2",
    );
    let count: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(node_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(node_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(count > 0)
}

async fn touch(db: &Database, script_id: &str) -> Result<()> {
    let now = now_rfc3339();
    let sql = db.sql(
        "UPDATE tag_scripts SET updated_at = ?, version = version + 1 WHERE id = ?",
        "UPDATE tag_scripts SET updated_at = $1, version = version + 1 WHERE id = $2::uuid",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&now)
                .bind(script_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&now)
                .bind(script_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

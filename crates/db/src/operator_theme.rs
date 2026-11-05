//! M45-69 — the operator's shelf, stored as a visible claim rather than blended into the theme.
//!
//! One row per instance: re-posting a shelf replaces it, because the statement is "this is what the
//! instance is for" and two of those cannot both be true. Deleting the row returns the instance to what
//! its readers' bookmarks say, which is why deletion is a supported verb and not a hidden reset.

use crate::{Backend, Database};
use anyhow::Result;
use serde_json::Value as JsonValue;

/// What the operator posted, already reduced: the vector, the headline tags, and every line with its
/// outcome.
#[derive(Debug, Clone)]
pub struct ThemeClaim<'a> {
    /// `{tag: weight}` derived from the seeds' tags.
    pub vector: &'a JsonValue,
    /// The ten heaviest tags with their weights, as published.
    pub top_tags: &'a JsonValue,
    /// Every line as posted, with `status` and a reason where it was not used.
    pub seeds: &'a JsonValue,
    /// How many lines were posted.
    pub seed_count: i64,
    /// How many of them contributed at least one tag.
    pub resolved_count: i64,
}

/// One instance's stated theme.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OperatorTheme {
    pub instance_id: String,
    pub theme_vector: JsonValue,
    pub top_tags: JsonValue,
    pub seeds: JsonValue,
    pub seed_count: i64,
    pub resolved_count: i64,
    pub sourced_at: String,
    pub updated_at: String,
}

type ThemeRow = (String, String, String, String, i64, i64, String, String);

impl From<ThemeRow> for OperatorTheme {
    fn from(
        (instance_id, vector, top_tags, seeds, seed_count, resolved_count, sourced_at, updated_at): ThemeRow,
    ) -> Self {
        let parse = |text: &str| serde_json::from_str(text).unwrap_or(JsonValue::Null);
        Self {
            instance_id,
            theme_vector: parse(&vector),
            top_tags: parse(&top_tags),
            seeds: parse(&seeds),
            seed_count,
            resolved_count,
            sourced_at,
            updated_at,
        }
    }
}

/// Replace the instance's stated theme.
pub async fn upsert(
    db: &Database,
    instance_id: &str,
    claim: &ThemeClaim<'_>,
    now: &str,
) -> Result<()> {
    let vector = serde_json::to_string(claim.vector)?;
    let top_tags = serde_json::to_string(claim.top_tags)?;
    let seeds = serde_json::to_string(claim.seeds)?;
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO operator_themes
                 (instance_id, theme_vector, top_tags, seeds, seed_count, resolved_count, sourced_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(instance_id) DO UPDATE SET
                   theme_vector = excluded.theme_vector,
                   top_tags = excluded.top_tags,
                   seeds = excluded.seeds,
                   seed_count = excluded.seed_count,
                   resolved_count = excluded.resolved_count,
                   sourced_at = excluded.sourced_at,
                   updated_at = excluded.updated_at",
            )
            .bind(instance_id)
            .bind(&vector)
            .bind(&top_tags)
            .bind(&seeds)
            .bind(claim.seed_count)
            .bind(claim.resolved_count)
            .bind(now)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO operator_themes
                 (instance_id, theme_vector, top_tags, seeds, seed_count, resolved_count, sourced_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT(instance_id) DO UPDATE SET
                   theme_vector = EXCLUDED.theme_vector,
                   top_tags = EXCLUDED.top_tags,
                   seeds = EXCLUDED.seeds,
                   seed_count = EXCLUDED.seed_count,
                   resolved_count = EXCLUDED.resolved_count,
                   sourced_at = EXCLUDED.sourced_at,
                   updated_at = EXCLUDED.updated_at",
            )
            .bind(instance_id)
            .bind(&vector)
            .bind(&top_tags)
            .bind(&seeds)
            .bind(claim.seed_count)
            .bind(claim.resolved_count)
            .bind(now)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?;
        }
    }
    Ok(())
}

/// The instance's stated theme, if it has one.
pub async fn get(db: &Database, instance_id: &str) -> Result<Option<OperatorTheme>> {
    const COLUMNS: &str = "instance_id, theme_vector, top_tags, seeds, seed_count, resolved_count, sourced_at, updated_at";
    let row: Option<ThemeRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&format!(
                "SELECT {COLUMNS} FROM operator_themes WHERE instance_id = ?"
            ))
            .bind(instance_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
            .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&format!(
                "SELECT {COLUMNS} FROM operator_themes WHERE instance_id = $1"
            ))
            .bind(instance_id)
            .fetch_optional(db.postgres_pool().expect("postgres handle"))
            .await?
        }
    };
    Ok(row.map(OperatorTheme::from))
}

/// Forget the stated theme, returning the instance to its bookmarks. Returns whether a row was removed,
/// so a caller can tell "forgotten" from "there was nothing to forget".
pub async fn delete(db: &Database, instance_id: &str) -> Result<bool> {
    let sql = match db.backend() {
        Backend::Sqlite => "DELETE FROM operator_themes WHERE instance_id = ?",
        Backend::Postgres => "DELETE FROM operator_themes WHERE instance_id = $1",
    };
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(sql)
            .bind(instance_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(sql)
            .bind(instance_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// The canonical tags a catalogue row carries, through its work.
///
/// Empty for a row with no `work_id`: a shelf entry imported but never linked to a work has no tags to
/// state, and inventing them from the title would be a claim about a work this instance has not read.
pub async fn tags_for_item(db: &Database, item_id: &str) -> Result<Vec<String>> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT tn.canonical
             FROM library_items li
             JOIN work_tags wt ON wt.work_id = li.work_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE li.id = ?
             ORDER BY tn.canonical"
        }
        Backend::Postgres => {
            "SELECT tn.canonical
             FROM library_items li
             JOIN work_tags wt ON wt.work_id = li.work_id
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE li.id = $1
             ORDER BY tn.canonical"
        }
    };
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sql)
                .bind(item_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(sql)
                .bind(item_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows.into_iter().map(|(tag,)| tag).collect())
}

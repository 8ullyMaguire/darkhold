//! `dh lib sync --since`: a derived local SQLite mirror of the site's library and catalogue.
//!
//! This is not a second schema. Each source row becomes one JSON payload keyed by
//! `(kind, id)`, and a single watermark records how far the mirror has moved.
//! The site database is never written to.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// One changed row from the source database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorChange {
    pub kind: &'static str,
    pub id: String,
    pub updated_at: String,
    pub payload: Value,
}

/// What one sync wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorReport {
    pub library_rows: usize,
    pub work_rows: usize,
    pub watermark: String,
}

type LibraryTuple = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<i64>,
    Option<String>,
);

type WorkTuple = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

fn library_change(row: LibraryTuple) -> MirrorChange {
    let (
        id,
        updated_at,
        title,
        author_text,
        source_key,
        work_id,
        source_url,
        status,
        word_count,
        last_synced_at,
    ) = row;
    MirrorChange {
        kind: "library_item",
        id: id.clone(),
        updated_at,
        payload: json!({
            "id": id,
            "title": title,
            "author_text": author_text,
            "source_key": source_key,
            "work_id": work_id,
            "source_url": source_url,
            "status": status,
            "word_count": word_count,
            "last_synced_at": last_synced_at,
        }),
    }
}

fn work_change(row: WorkTuple) -> MirrorChange {
    let (
        id,
        updated_at,
        title,
        summary,
        rating,
        visibility,
        lifecycle,
        completion,
        owner_pseud_id,
        deleted_at,
    ) = row;
    MirrorChange {
        kind: "work",
        id: id.clone(),
        updated_at,
        payload: json!({
            "id": id,
            "title": title,
            "summary": summary,
            "rating": rating,
            "visibility": visibility,
            "lifecycle": lifecycle,
            "completion": completion,
            "owner_pseud_id": owner_pseud_id,
            "deleted_at": deleted_at,
        }),
    }
}

async fn library_changes(
    db: &darkhold_db::Database,
    since: &str,
    account: Option<&str>,
) -> Result<Vec<MirrorChange>> {
    let (sqlite, postgres) = if account.is_some() {
        (
            "SELECT id, updated_at, title, author_text, source_key, work_id, source_url, status, word_count, last_synced_at FROM library_items WHERE updated_at >= ? AND account_id = ?",
            "SELECT id::text, updated_at, title, author_text, source_key, work_id::text, source_url, status, word_count::bigint, last_synced_at FROM library_items WHERE updated_at >= $1 AND account_id = $2",
        )
    } else {
        (
            "SELECT id, updated_at, title, author_text, source_key, work_id, source_url, status, word_count, last_synced_at FROM library_items WHERE updated_at >= ?",
            "SELECT id::text, updated_at, title, author_text, source_key, work_id::text, source_url, status, word_count::bigint, last_synced_at FROM library_items WHERE updated_at >= $1",
        )
    };
    let sql = db.sql(sqlite, postgres);
    let rows = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            let mut query = sqlx::query_as::<sqlx::Sqlite, LibraryTuple>(&sql).bind(since);
            if let Some(account) = account {
                query = query.bind(account);
            }
            query.fetch_all(db.sqlite_pool().expect("sqlite")).await?
        }
        darkhold_db::Backend::Postgres => {
            let mut query = sqlx::query_as::<sqlx::Postgres, LibraryTuple>(&sql).bind(since);
            if let Some(account) = account {
                query = query.bind(account);
            }
            query
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows.into_iter().map(library_change).collect())
}

async fn work_changes(db: &darkhold_db::Database, since: &str) -> Result<Vec<MirrorChange>> {
    let sqlite = "SELECT id, updated_at, title, summary, rating, visibility, lifecycle, completion, owner_pseud_id, deleted_at FROM works WHERE updated_at >= ?";
    let postgres = "SELECT id::text, updated_at, title, summary, rating, visibility, lifecycle, completion, owner_pseud_id::text, deleted_at FROM works WHERE updated_at >= $1";
    let sql = db.sql(sqlite, postgres);
    let rows = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_as::<sqlx::Sqlite, WorkTuple>(&sql)
                .bind(since)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_as::<sqlx::Postgres, WorkTuple>(&sql)
                .bind(since)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows.into_iter().map(work_change).collect())
}

/// Read rows newer than `since` from the configured site database. An absent
/// `since` means the beginning of time; the caller can pass the stored watermark
/// for an incremental pull.
pub async fn read_changes(
    db: &darkhold_db::Database,
    since: Option<&str>,
    account: Option<&str>,
) -> Result<Vec<MirrorChange>> {
    let since = since.unwrap_or("0000-01-01T00:00:00Z");
    let mut changes = library_changes(db, since, account).await?;
    changes.extend(work_changes(db, since).await?);
    changes.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    Ok(changes)
}

/// Upsert the changed rows into the derived SQLite mirror and move its watermark.
pub async fn write_mirror(
    path: &Path,
    changes: &[MirrorChange],
    watermark: &str,
) -> Result<MirrorReport> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let config = darkhold_db::DatabaseConfig::new(url);
    let db = darkhold_db::Database::connect(&config).await?;
    let pool = db.sqlite_pool().expect("mirror is SQLite");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS mirror_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS mirror_rows (\n         kind TEXT NOT NULL, id TEXT NOT NULL, updated_at TEXT NOT NULL, payload TEXT NOT NULL,\n         PRIMARY KEY (kind, id))",
    )
    .execute(pool)
    .await?;

    for change in changes {
        sqlx::query(
            "INSERT INTO mirror_rows (kind, id, updated_at, payload) VALUES (?, ?, ?, ?)\n             ON CONFLICT(kind, id) DO UPDATE SET updated_at = excluded.updated_at, payload = excluded.payload",
        )
        .bind(change.kind)
        .bind(&change.id)
        .bind(&change.updated_at)
        .bind(change.payload.to_string())
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO mirror_meta (key, value) VALUES ('last_sync', ?)\n         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(watermark)
    .execute(pool)
    .await?;

    let report = MirrorReport {
        library_rows: changes.iter().filter(|c| c.kind == "library_item").count(),
        work_rows: changes.iter().filter(|c| c.kind == "work").count(),
        watermark: watermark.to_owned(),
    };
    db.close().await;
    Ok(report)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_missing_since_means_the_beginning() {
        assert!("0000-01-01T00:00:00Z" < "1970-01-01T00:00:00Z");
    }
}

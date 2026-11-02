//! §13's account archive: everything this instance holds about one account, in one document
//! (`M45-39` phase 2 — the thing `succession_plan.export_url` is supposed to point at).
//!
//! # Why this is not the per-work exporter
//!
//! `crates/app/src/routes/exports.rs` renders *a work* into EPUB, PDF and friends, as a job, because a
//! 122-unit work is not something to do inside a request. This is the other question: what does the
//! instance hold about **me** — my pseuds, my works, my shelf, my progress, my ratings, my reviews, my
//! credits — in a shape a person can keep after the instance is gone. A rendered novel is a reading
//! copy; this is the recovery copy, and it is JSON because the reader who needs it needs the data, not
//! the typography.
//!
//! # Every section carries a checksum
//!
//! `manifest` holds a per-section SHA-256 and a row count. A dead instance cannot be asked whether
//! the file it handed out was complete; a hash computed by the instance that wrote it can at least
//! tell a reader whether the copy they kept is the copy they were given.
//!
//! # What is deliberately absent
//!
//! - **Work bodies.** This archive lists a reader's published works and their metadata; the words
//!   travel through the work exporter, which is where the licence and the CTA logic lives. Bundling
//!   them here would be a second, unaudited path to the same bytes.
//! - **Other people's data.** A bookmark's *note* is the reader's; the work it points at is
//!   identified by id and title only, never by its author's private fields.
//! - **Anything deleted.** Rows with a `deleted_at` are excluded: an archive of erased data would be
//!   an erasure that did not happen.

use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::Database;

/// The schema name this archive declares.
pub const SCHEMA: &str = "darkhold-archive/1";

/// One section of the archive.
#[derive(Debug, Clone, Serialize)]
pub struct Section {
    /// The section's name, which is also its key in the document.
    pub name: &'static str,
    /// The rows.
    pub rows: Vec<Value>,
    /// How many rows.
    pub count: usize,
    /// SHA-256 of the section's JSON, so a reader can tell whether their copy is complete.
    pub sha256: String,
}

impl Section {
    fn new(name: &'static str, rows: Vec<Value>) -> Self {
        let body = serde_json::to_string(&rows).unwrap_or_default();
        let digest = Sha256::digest(body.as_bytes());
        Self {
            name,
            count: rows.len(),
            sha256: hex::encode(digest),
            rows,
        }
    }
}

/// Build every section for one account.
///
/// Section order is fixed so two archives of an unchanged account are byte-identical apart from
/// `generated_at` — a reader comparing two downloads should see "nothing changed", not "everything
/// changed because a HashMap was iterated".
pub async fn build(db: &Database, account_id: &str) -> Result<Vec<Section>> {
    let mut sections = Vec::with_capacity(11);
    sections.push(Section::new("account", account(db, account_id).await?));
    sections.push(Section::new("pseuds", pseuds(db, account_id).await?));
    sections.push(Section::new("works", works(db, account_id).await?));
    sections.push(Section::new("bookmarks", bookmarks(db, account_id).await?));
    sections.push(Section::new("library", library(db, account_id).await?));
    sections.push(Section::new("reading_status", reading_status(db, account_id).await?));
    sections.push(Section::new("reading_progress", reading_progress(db, account_id).await?));
    sections.push(Section::new("reading_history", reading_history(db, account_id).await?));
    sections.push(Section::new("ratings", ratings(db, account_id).await?));
    sections.push(Section::new("reviews", reviews(db, account_id).await?));
    sections.push(Section::new("credits", credits(db, account_id).await?));
    Ok(sections)
}

/// Fetch typed rows from either engine.
///
/// The two pools have different `Executor` types, so the match is unavoidable; keeping it in one
/// macro keeps the eleven call sites from each spelling it out and drifting.
macro_rules! fetch {
    ($db:expr, $sqlite:expr, $postgres:expr, $account:expr, $row:ty) => {{
        // The PostgreSQL arm differs from the SQLite one by casts, not just by placeholders: the
        // identity columns are UUID there and text here, and `uuid = text` is not an operator.
        let sql = $db.sql($sqlite, $postgres);
        match $db.backend() {
            crate::Backend::Sqlite => {
                sqlx::query_as::<_, $row>(&sql)
                    .bind($account)
                    .fetch_all($db.sqlite_pool().expect("sqlite handle"))
                    .await?
            }
            crate::Backend::Postgres => {
                sqlx::query_as::<_, $row>(&sql)
                    .bind($account)
                    .fetch_all($db.postgres_pool().expect("postgres handle"))
                    .await?
            }
        }
    }};
}

async fn account(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let rows: Vec<(String, String, String, String)> = fetch!(
        db,
        "SELECT id, email, age_state, created_at FROM accounts WHERE id = ?",
        "SELECT id::text, email, age_state, created_at FROM accounts WHERE id::text = ?",
        account_id,
        (String, String, String, String)
    );
    Ok(rows
        .into_iter()
        .map(|(id, email, age_state, created_at)| {
            json!({ "id": id, "email": email, "age_state": age_state, "created_at": created_at })
        })
        .collect())
}

async fn pseuds(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let rows: Vec<(String, String, String, Option<String>, String, String)> = fetch!(
        db,
        "SELECT id, handle, display_name, bio, discoverability, created_at
           FROM pseuds WHERE account_id = ? AND deleted_at IS NULL
          ORDER BY created_at ASC, id ASC",
        "SELECT id::text, handle, display_name, bio, discoverability, created_at
           FROM pseuds WHERE account_id::text = ? AND deleted_at IS NULL
          ORDER BY created_at ASC, id ASC",
        account_id,
        (String, String, String, Option<String>, String, String)
    );
    Ok(rows
        .into_iter()
        .map(|(id, handle, display_name, bio, discoverability, created_at)| {
            json!({
                "id": id,
                "handle": handle,
                "display_name": display_name,
                "bio": bio,
                "discoverability": discoverability,
                "created_at": created_at,
            })
        })
        .collect())
}

async fn works(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let rows: Vec<(String, String, String, String, String, Option<String>, String)> = fetch!(
        db,
        "SELECT w.id, w.title, w.language, w.lifecycle, w.visibility, w.published_at, w.created_at
           FROM works w
           JOIN pseuds p ON p.id = w.owner_pseud_id
          WHERE p.account_id = ? AND p.deleted_at IS NULL AND w.deleted_at IS NULL
          ORDER BY w.created_at ASC, w.id ASC",
        "SELECT w.id::text, w.title, w.language, w.lifecycle, w.visibility, w.published_at, w.created_at
           FROM works w
           JOIN pseuds p ON p.id = w.owner_pseud_id
          WHERE p.account_id::text = ? AND p.deleted_at IS NULL AND w.deleted_at IS NULL
          ORDER BY w.created_at ASC, w.id ASC",
        account_id,
        (String, String, String, String, String, Option<String>, String)
    );
    Ok(rows
        .into_iter()
        .map(|(id, title, language, lifecycle, visibility, published_at, created_at)| {
            json!({
                "id": id,
                "title": title,
                "language": language,
                "lifecycle": lifecycle,
                "visibility": visibility,
                "published_at": published_at,
                "created_at": created_at,
            })
        })
        .collect())
}

async fn bookmarks(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    // `is_public` is INTEGER on SQLite and BOOLEAN on PostgreSQL (0009_library.sql), so the two
    // engines need different spellings of the same column. This is the class of fault ADR 0004 names,
    // one layer down: the type differs, not just the width.
    let sql = db.sql(
        "SELECT subject_type, subject_id, unit_id, note, is_public, created_at
           FROM bookmarks WHERE account_id = ?
          ORDER BY created_at ASC, id ASC",
        "SELECT subject_type, subject_id::text, unit_id, note, is_public::int, created_at
           FROM bookmarks WHERE account_id::text = ?
          ORDER BY created_at ASC, id ASC",
    );
    let rows: Vec<(String, String, Option<String>, String, i32, String)> = match db.backend() {
        crate::Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        crate::Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(subject_type, subject_id, unit_id, note, is_public, created_at)| {
            json!({
                "subject_type": subject_type,
                "subject_id": subject_id,
                "unit_id": unit_id,
                "note": note,
                "is_public": is_public != 0,
                "created_at": created_at,
            })
        })
        .collect())
}

async fn library(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let rows: Vec<(String, String, String, String, Option<String>, String)> = fetch!(
        db,
        "SELECT title, author_text, source_key, source_work_key, work_id, created_at
           FROM library_items WHERE account_id = ?
          ORDER BY created_at ASC, id ASC",
        "SELECT title, author_text, source_key, source_work_key, work_id::text, created_at
           FROM library_items WHERE account_id::text = ?
          ORDER BY created_at ASC, id ASC",
        account_id,
        (String, String, String, String, Option<String>, String)
    );
    Ok(rows
        .into_iter()
        .map(|(title, author_text, source_key, source_work_key, work_id, created_at)| {
            json!({
                "title": title,
                "author_text": author_text,
                "source_key": source_key,
                "source_work_key": source_work_key,
                "work_id": work_id,
                "created_at": created_at,
            })
        })
        .collect())
}

async fn reading_status(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let rows: Vec<(String, String, String, Option<String>, Option<String>, String)> = fetch!(
        db,
        "SELECT subject_type, subject_id, status, started_at, finished_at, updated_at
           FROM reading_status WHERE account_id = ?
          ORDER BY updated_at ASC, id ASC",
        "SELECT subject_type, subject_id::text, status, started_at, finished_at, updated_at
           FROM reading_status WHERE account_id::text = ?
          ORDER BY updated_at ASC, id ASC",
        account_id,
        (String, String, String, Option<String>, Option<String>, String)
    );
    Ok(rows
        .into_iter()
        .map(|(subject_type, subject_id, status, started_at, finished_at, updated_at)| {
            json!({
                "subject_type": subject_type,
                "subject_id": subject_id,
                "status": status,
                "started_at": started_at,
                "finished_at": finished_at,
                "updated_at": updated_at,
            })
        })
        .collect())
}

async fn reading_progress(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let rows: Vec<(String, String, Option<String>, i64, String)> = fetch!(
        db,
        "SELECT subject_type, subject_id, unit_id, position_permille, updated_at
           FROM reading_progress WHERE account_id = ?
          ORDER BY updated_at ASC, id ASC",
        "SELECT subject_type, subject_id::text, unit_id, position_permille, updated_at
           FROM reading_progress WHERE account_id::text = ?
          ORDER BY updated_at ASC, id ASC",
        account_id,
        (String, String, Option<String>, i64, String)
    );
    Ok(rows
        .into_iter()
        .map(|(subject_type, subject_id, unit_id, position_permille, updated_at)| {
            json!({
                "subject_type": subject_type,
                "subject_id": subject_id,
                "unit_id": unit_id,
                "position_permille": position_permille,
                "updated_at": updated_at,
            })
        })
        .collect())
}

async fn reading_history(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let rows: Vec<(String, String, String)> = fetch!(
        db,
        "SELECT subject_type, subject_id, last_read_at
           FROM reading_history_entry WHERE account_id = ?
          ORDER BY last_read_at ASC, id ASC",
        "SELECT subject_type, subject_id::text, last_read_at
           FROM reading_history_entry WHERE account_id::text = ?
          ORDER BY last_read_at ASC, id ASC",
        account_id,
        (String, String, String)
    );
    Ok(rows
        .into_iter()
        .map(|(subject_type, subject_id, last_read_at)| {
            json!({
                "subject_type": subject_type,
                "subject_id": subject_id,
                "last_read_at": last_read_at,
            })
        })
        .collect())
}

async fn ratings(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let sql = db.sql(
        "SELECT work_id, stars, is_public, created_at
           FROM rating WHERE account_id = ? AND deleted_at IS NULL
          ORDER BY created_at ASC, id ASC",
        "SELECT work_id::text, stars, is_public::int, created_at
           FROM rating WHERE account_id::text = ? AND deleted_at IS NULL
          ORDER BY created_at ASC, id ASC",
    );
    // `stars` is BIGINT on PostgreSQL (0004_reading.sql) and INTEGER on SQLite. `i64` decodes both;
    // `i32` decodes only SQLite, which is exactly the kind of difference that compiles and fails on
    // the second engine.
    let rows: Vec<(String, i64, i32, String)> = match db.backend() {
        crate::Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        crate::Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(work_id, stars, is_public, created_at)| {
            json!({
                "work_id": work_id,
                "stars": stars,
                "is_public": is_public != 0,
                "created_at": created_at,
            })
        })
        .collect())
}

async fn reviews(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let sql = db.sql(
        "SELECT work_id, body, contains_spoilers, is_public, created_at
           FROM review WHERE account_id = ? AND deleted_at IS NULL
          ORDER BY created_at ASC, id ASC",
        "SELECT work_id::text, body, contains_spoilers::int, is_public::int, created_at
           FROM review WHERE account_id::text = ? AND deleted_at IS NULL
          ORDER BY created_at ASC, id ASC",
    );
    let rows: Vec<(String, String, i32, i32, String)> = match db.backend() {
        crate::Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        crate::Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(work_id, body, contains_spoilers, is_public, created_at)| {
            json!({
                "work_id": work_id,
                "body": body,
                "contains_spoilers": contains_spoilers != 0,
                "is_public": is_public != 0,
                "created_at": created_at,
            })
        })
        .collect())
}

/// The credit ledger, summed per bucket.
///
/// Summed rather than itemised: §9.7.1 forbids a lifetime personal total as a *surface*, but an
/// archive is not a surface, and a reader recovering their account needs to know what they held. The
/// individual transactions are the ledger's, and they are not this account's to publish.
async fn credits(db: &Database, account_id: &str) -> Result<Vec<Value>> {
    let sql = db.sql(
        "SELECT bucket, CAST(SUM(amount_bp) AS INTEGER) AS total
           FROM credit_entries WHERE account = ? GROUP BY bucket ORDER BY bucket ASC",
        "SELECT bucket, SUM(amount_bp)::bigint AS total
           FROM credit_entries WHERE account = ? GROUP BY bucket ORDER BY bucket ASC",
    );
    let rows: Vec<(String, i64)> = match db.backend() {
        crate::Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        crate::Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(bucket, total)| json!({ "bucket": bucket, "total": total }))
        .collect())
}

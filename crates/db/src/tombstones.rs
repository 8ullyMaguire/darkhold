//! M69-04 — the tombstone store: file it, find it, refuse with it, and let it be withdrawn.
//!
//! # The refusal is recorded, not just evaluated
//!
//! `refusals` is incremented on every refused import attempt, with `last_refused_at`. A tombstone that has never matched
//! anything is a guess about a work's identity — often a title spelled differently from the source's — and an operator
//! cannot tell a working tombstone from a mistyped one without that number. It is also the answer to "did the takedown
//! actually stop the re-import?" without reading the job log.
//!
//! # Withdrawal is a column, not a delete
//!
//! An over-broad tombstone must be undoable, and the row is the evidence that it refused what it refused. Deleting it would
//! answer the author's "why did my work vanish?" with silence. So `withdraw` writes `withdrawn_at` and the refusal check
//! filters on it — a withdrawn tombstone stops refusing and still explains itself.

use anyhow::Result;
use sqlx::Row as _;

use darkhold_domain::work_identity;

use crate::{Backend, Database};

/// A filed tombstone.
#[derive(Debug, Clone, PartialEq)]
pub struct Tombstone {
    /// Its id.
    pub id: String,
    /// The identity key it refuses.
    pub identity: String,
    /// The title as filed.
    pub title: String,
    /// The author as filed.
    pub author: String,
    /// The language as filed.
    pub language: Option<String>,
    /// Why it was filed.
    pub reason: String,
    /// Where the work was first seen. Not part of the key.
    pub origin_url: Option<String>,
    /// The DMCA paperwork, when there is any.
    pub dmca_id: Option<String>,
    /// Who filed it.
    pub filed_by: String,
    /// When.
    pub filed_at: String,
    /// A free-form note.
    pub note: String,
    /// How many imports it has stopped.
    pub refusals: i64,
    /// When it last stopped one.
    pub last_refused_at: Option<String>,
    /// When it was withdrawn, if it was.
    pub withdrawn_at: Option<String>,
    /// Why it was withdrawn.
    pub withdraw_reason: Option<String>,
}

/// What a refusal cites.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    /// The tombstone's id.
    pub tombstone: String,
    /// Why the work was taken down — the sentence an operator puts in the import report.
    pub reason: String,
    /// When it was filed.
    pub filed_at: String,
}

/// A filing: what a tombstone records about the work and why.
///
/// A struct rather than nine positional arguments, because six of them are `&str` and a caller that swaps `reason` and
/// `note` would compile, store a sentence in the wrong column, and only a human reading the refusal report would notice.
#[derive(Debug, Clone)]
pub struct Filing<'a> {
    /// The work's title, as an importer sees it.
    pub title: &'a str,
    /// The author as displayed. Empty is a real value: it is not a wildcard.
    pub author: &'a str,
    /// The language, when it is known.
    pub language: Option<&'a str>,
    /// Why: `author request`, `dmca`, `court order`, ...
    pub reason: &'a str,
    /// Where the work was seen. Not part of the key.
    pub origin_url: Option<&'a str>,
    /// The DMCA paperwork's id, when there is any.
    pub dmca_id: Option<&'a str>,
    /// Who filed it.
    pub filed_by: &'a str,
    /// A free-form note.
    pub note: &'a str,
    /// When it was filed.
    pub now: &'a str,
}

/// File a tombstone for a work identity.
///
/// Filing the same identity twice updates the existing row rather than failing: a second complaint about a work already
/// tombstoned is a stronger version of the same fact, and refusing it would leave the operator with an error and no way to
/// add the paperwork reference.
pub async fn file(db: &Database, filing: &Filing<'_>) -> Result<Tombstone> {
    let Filing {
        title,
        author,
        language,
        reason,
        origin_url,
        dmca_id,
        filed_by,
        note,
        now,
    } = *filing;
    let identity = work_identity::key(title, author, language).ok_or_else(|| {
        anyhow::anyhow!("a work with no title has no identity, so there is nothing to tombstone")
    })?;
    let id = uuid::Uuid::new_v4().to_string();

    // A re-file of a *withdrawn* tombstone re-arms it: the author withdrew the complaint and then made it again, which is a
    // different claim from "the tombstone was wrong the first time".
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO work_tombstones
                    (id, identity, title, author, language, reason, origin_url, dmca_id, filed_by, filed_at, note)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(identity) DO UPDATE SET
                    title = excluded.title,
                    author = excluded.author,
                    language = excluded.language,
                    reason = excluded.reason,
                    origin_url = COALESCE(excluded.origin_url, work_tombstones.origin_url),
                    dmca_id = COALESCE(excluded.dmca_id, work_tombstones.dmca_id),
                    filed_by = excluded.filed_by,
                    filed_at = excluded.filed_at,
                    note = excluded.note,
                    withdrawn_at = NULL,
                    withdrawn_by = NULL,
                    withdraw_reason = NULL",
            )
            .bind(&id)
            .bind(&identity)
            .bind(title)
            .bind(author)
            .bind(language)
            .bind(reason)
            .bind(origin_url)
            .bind(dmca_id)
            .bind(filed_by)
            .bind(now)
            .bind(note)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO work_tombstones
                    (id, identity, title, author, language, reason, origin_url, dmca_id, filed_by, filed_at, note)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 ON CONFLICT(identity) DO UPDATE SET
                    title = EXCLUDED.title,
                    author = EXCLUDED.author,
                    language = EXCLUDED.language,
                    reason = EXCLUDED.reason,
                    origin_url = COALESCE(EXCLUDED.origin_url, work_tombstones.origin_url),
                    dmca_id = COALESCE(EXCLUDED.dmca_id, work_tombstones.dmca_id),
                    filed_by = EXCLUDED.filed_by,
                    filed_at = EXCLUDED.filed_at,
                    note = EXCLUDED.note,
                    withdrawn_at = NULL,
                    withdrawn_by = NULL,
                    withdraw_reason = NULL",
            )
            .bind(&id)
            .bind(&identity)
            .bind(title)
            .bind(author)
            .bind(language)
            .bind(reason)
            .bind(origin_url)
            .bind(dmca_id)
            .bind(filed_by)
            .bind(now)
            .bind(note)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }

    find_by_identity(db, &identity)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the tombstone was written and cannot be read back"))
}

fn project(tombstone: &Tombstone) -> &Tombstone {
    tombstone
}

/// Read one tombstone by identity, withdrawn or not.
async fn find_by_identity(db: &Database, identity: &str) -> Result<Option<Tombstone>> {
    let sqlite = "SELECT id, identity, title, author, language, reason, origin_url, dmca_id, filed_by, filed_at, \
                  note, refusals, last_refused_at, withdrawn_at, withdraw_reason \
                  FROM work_tombstones WHERE identity = ?";
    let postgres = "SELECT id, identity, title, author, language, reason, origin_url, dmca_id, filed_by, filed_at, \
                    note, CAST(refusals AS BIGINT) AS refusals, last_refused_at, withdrawn_at, withdraw_reason \
                    FROM work_tombstones WHERE identity = $1";
    let found = match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(sqlite)
                .bind(identity)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?;
            row.map(|row| Tombstone {
                id: row.get("id"),
                identity: row.get("identity"),
                title: row.get("title"),
                author: row.get("author"),
                language: row.get("language"),
                reason: row.get("reason"),
                origin_url: row.get("origin_url"),
                dmca_id: row.get("dmca_id"),
                filed_by: row.get("filed_by"),
                filed_at: row.get("filed_at"),
                note: row.get("note"),
                refusals: row.get("refusals"),
                last_refused_at: row.get("last_refused_at"),
                withdrawn_at: row.get("withdrawn_at"),
                withdraw_reason: row.get("withdraw_reason"),
            })
        }
        Backend::Postgres => {
            let row = sqlx::query(postgres)
                .bind(identity)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?;
            row.map(|row| Tombstone {
                id: row.get("id"),
                identity: row.get("identity"),
                title: row.get("title"),
                author: row.get("author"),
                language: row.get("language"),
                reason: row.get("reason"),
                origin_url: row.get("origin_url"),
                dmca_id: row.get("dmca_id"),
                filed_by: row.get("filed_by"),
                filed_at: row.get("filed_at"),
                note: row.get("note"),
                refusals: row.get("refusals"),
                last_refused_at: row.get("last_refused_at"),
                withdrawn_at: row.get("withdrawn_at"),
                withdraw_reason: row.get("withdraw_reason"),
            })
        }
    };
    Ok(found.map(|tombstone| project(&tombstone).clone()))
}

/// Check a work about to be imported, and record the refusal when there is one.
///
/// Returns `None` when the work may be imported. A withdrawn tombstone is not a refusal, and an untitled work is not
/// checked at all — it cannot be keyed, and inventing a key for it would match something.
pub async fn refuse(
    db: &Database,
    title: &str,
    author: &str,
    language: Option<&str>,
    now: &str,
) -> Result<Option<Refusal>> {
    let Some(identity) = work_identity::key(title, author, language) else {
        return Ok(None);
    };
    let Some(tombstone) = find_by_identity(db, &identity).await? else {
        return Ok(None);
    };
    if tombstone.withdrawn_at.is_some() {
        return Ok(None);
    }

    // The count is the operator's evidence that the tombstone works, so it is written before the caller is told. If this
    // update fails the import is refused anyway: a counter is not worth re-serving a taken-down work over.
    let counted = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "UPDATE work_tombstones SET refusals = refusals + 1, last_refused_at = ? WHERE identity = ?",
            )
            .bind(now)
            .bind(&identity)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .map(|_| ())
        }
        Backend::Postgres => {
            sqlx::query(
                "UPDATE work_tombstones SET refusals = refusals + 1, last_refused_at = $1 WHERE identity = $2",
            )
            .bind(now)
            .bind(&identity)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .map(|_| ())
        }
    };
    if let Err(error) = counted {
        tracing::warn!(%error, "a tombstone refusal was not counted");
    }

    Ok(Some(Refusal {
        tombstone: tombstone.id,
        reason: tombstone.reason,
        filed_at: tombstone.filed_at,
    }))
}

/// Every tombstone, newest first, with withdrawn ones included.
pub async fn list(db: &Database, limit: i64) -> Result<Vec<Tombstone>> {
    let limit = limit.clamp(1, 500);
    let sqlite = "SELECT id, identity, title, author, language, reason, origin_url, dmca_id, filed_by, filed_at, \
                  note, refusals, last_refused_at, withdrawn_at, withdraw_reason \
                  FROM work_tombstones ORDER BY filed_at DESC, id DESC LIMIT ?";
    let postgres = "SELECT id, identity, title, author, language, reason, origin_url, dmca_id, filed_by, filed_at, \
                    note, CAST(refusals AS BIGINT) AS refusals, last_refused_at, withdrawn_at, withdraw_reason \
                    FROM work_tombstones ORDER BY filed_at DESC, id DESC LIMIT $1";
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(sqlite)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows
                .iter()
                .map(|row| Tombstone {
                    id: row.get("id"),
                    identity: row.get("identity"),
                    title: row.get("title"),
                    author: row.get("author"),
                    language: row.get("language"),
                    reason: row.get("reason"),
                    origin_url: row.get("origin_url"),
                    dmca_id: row.get("dmca_id"),
                    filed_by: row.get("filed_by"),
                    filed_at: row.get("filed_at"),
                    note: row.get("note"),
                    refusals: row.get("refusals"),
                    last_refused_at: row.get("last_refused_at"),
                    withdrawn_at: row.get("withdrawn_at"),
                    withdraw_reason: row.get("withdraw_reason"),
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(postgres)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows
                .iter()
                .map(|row| Tombstone {
                    id: row.get("id"),
                    identity: row.get("identity"),
                    title: row.get("title"),
                    author: row.get("author"),
                    language: row.get("language"),
                    reason: row.get("reason"),
                    origin_url: row.get("origin_url"),
                    dmca_id: row.get("dmca_id"),
                    filed_by: row.get("filed_by"),
                    filed_at: row.get("filed_at"),
                    note: row.get("note"),
                    refusals: row.get("refusals"),
                    last_refused_at: row.get("last_refused_at"),
                    withdrawn_at: row.get("withdrawn_at"),
                    withdraw_reason: row.get("withdraw_reason"),
                })
                .collect())
        }
    }
}

/// Withdraw a tombstone, so it stops refusing and still explains itself.
pub async fn withdraw(db: &Database, id: &str, by: &str, reason: &str, now: &str) -> Result<bool> {
    if reason.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "a withdrawal needs a reason: the row is the answer to 'why did my work vanish?'"
        ));
    }
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "UPDATE work_tombstones SET withdrawn_at = ?, withdrawn_by = ?, withdraw_reason = ?
                  WHERE id = ? AND withdrawn_at IS NULL",
        )
        .bind(now)
        .bind(by)
        .bind(reason)
        .bind(id)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await?
        .rows_affected(),
        Backend::Postgres => sqlx::query(
            "UPDATE work_tombstones SET withdrawn_at = $1, withdrawn_by = $2, withdraw_reason = $3
                  WHERE id = $4 AND withdrawn_at IS NULL",
        )
        .bind(now)
        .bind(by)
        .bind(reason)
        .bind(id)
        .execute(db.postgres_pool().expect("postgres"))
        .await?
        .rows_affected(),
    };
    Ok(affected > 0)
}

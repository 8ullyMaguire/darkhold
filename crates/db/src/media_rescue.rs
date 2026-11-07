//! M69-06 — the rescue ring's store: survivor copies in, hunts out.
//!
//! The decision itself is `darkhold_domain::media_rescue::rescue`'s; this module supplies it with the copies and records
//! what it decided. Keeping the two apart is what lets the ring be tested without a database and the store be tested without
//! two hundred lines of hash fixtures.
//!
//! # A hunt is a row even when it finds nothing
//!
//! `NotFound` is stored. The alternative — store only successes — makes "we looked and nobody had it" invisible, and that is
//! the answer an operator needs most when deciding whether to give up on a piece of media or write to the author.

use anyhow::Result;
use sqlx::Row as _;

use darkhold_domain::media_rescue::{rescue, Copy, Outcome, Wanted};

use crate::{Backend, Database};

/// One survivor's claim about a copy it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct SurvivorCopy {
    /// Its id.
    pub id: String,
    /// The survivor's canonical base URL.
    pub instance: String,
    /// The exact hash of the bytes.
    pub content_hash: String,
    /// Its perceptual hash, when it computed one.
    pub perceptual_hash: Option<String>,
    /// Width in pixels.
    pub width: Option<i64>,
    /// Height in pixels.
    pub height: Option<i64>,
    /// Recorded when the survivor said so.
    pub recorded_at: String,
}

/// One hunt and what it decided.
#[derive(Debug, Clone, PartialEq)]
pub struct RescueRequest {
    /// Its id.
    pub id: String,
    /// The peer whose copy is gone.
    pub dead_peer: String,
    /// The media reference this instance still has a row for.
    pub media_reference_id: String,
    /// The bytes it used to have.
    pub content_hash: String,
    /// The perceptual hash it used to have.
    pub perceptual_hash: Option<String>,
    /// The Hamming bound the search used.
    pub threshold: i64,
    /// `exact`, `proposal`, `ambiguous` or `not_found`.
    pub outcome: String,
    /// The survivor to credit, when exactly one was named. `None` on an ambiguity — deliberately.
    pub matched_instance: Option<String>,
    /// The Hamming distance, when there was a measurement.
    pub distance: Option<i64>,
    /// The confidence derived from it.
    pub confidence: Option<f64>,
    /// The tied or near names, so a human can choose later.
    pub candidates_json: String,
    /// When the hunt ran.
    pub created_at: String,
    /// When a human settled it.
    pub resolved_at: Option<String>,
    /// Who settled it.
    pub resolved_by: Option<String>,
    /// `accepted`, `rejected` or `abandoned`.
    pub resolution: Option<String>,
}

/// What a survivor says it holds, as one value.
///
/// A struct rather than eight arguments: five of them are `&str`/`Option<&str>` and a caller that swapped `perceptual_hash`
/// for `media_kind` would compile and store nonsense in a column nothing validates.
#[derive(Debug, Clone)]
pub struct CopyClaim<'a> {
    /// The survivor's canonical base URL.
    pub instance: &'a str,
    /// The exact hash of the bytes it holds.
    pub content_hash: &'a str,
    /// Its perceptual hash, when it computed one.
    pub perceptual_hash: Option<&'a str>,
    /// Width in pixels.
    pub width: Option<i64>,
    /// Height in pixels.
    pub height: Option<i64>,
    /// `image`, `audio`, `video` — the same vocabulary `media_references` uses.
    pub media_kind: &'a str,
    /// When it said so.
    pub now: &'a str,
}

/// Record what a survivor says it holds.
///
/// Keyed by `(instance, content_hash)`: a survivor re-announcing the same bytes updates the row rather than adding a second,
/// and `last_seen_at` moves. The perceptual hash is replaced too — a survivor that recomputed it with a better algorithm is
/// telling the truth twice, and the last word is its own.
pub async fn record_copy(db: &Database, claim: &CopyClaim<'_>) -> Result<SurvivorCopy> {
    let CopyClaim {
        instance,
        content_hash,
        perceptual_hash,
        width,
        height,
        media_kind,
        now,
    } = *claim;
    if instance.trim().is_empty() || content_hash.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "a copy needs an instance and a content hash; without them it cannot be matched to anything"
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO media_rescue_copies
                    (id, instance, content_hash, perceptual_hash, width, height, media_kind, recorded_at, last_seen_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(instance, content_hash) DO UPDATE SET
                    perceptual_hash = COALESCE(excluded.perceptual_hash, media_rescue_copies.perceptual_hash),
                    width = COALESCE(excluded.width, media_rescue_copies.width),
                    height = COALESCE(excluded.height, media_rescue_copies.height),
                    media_kind = excluded.media_kind,
                    last_seen_at = excluded.last_seen_at",
            )
            .bind(&id)
            .bind(instance)
            .bind(content_hash)
            .bind(perceptual_hash)
            .bind(width)
            .bind(height)
            .bind(media_kind)
            .bind(now)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO media_rescue_copies
                    (id, instance, content_hash, perceptual_hash, width, height, media_kind, recorded_at, last_seen_at)
                 VALUES ($1, $2, $3, $4, $5::bigint, $6::bigint, $7, $8, $9)
                 ON CONFLICT(instance, content_hash) DO UPDATE SET
                    perceptual_hash = COALESCE(EXCLUDED.perceptual_hash, media_rescue_copies.perceptual_hash),
                    width = COALESCE(EXCLUDED.width, media_rescue_copies.width),
                    height = COALESCE(EXCLUDED.height, media_rescue_copies.height),
                    media_kind = EXCLUDED.media_kind,
                    last_seen_at = EXCLUDED.last_seen_at",
            )
            .bind(&id)
            .bind(instance)
            .bind(content_hash)
            .bind(perceptual_hash)
            .bind(width)
            .bind(height)
            .bind(media_kind)
            .bind(now)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    find_copy(db, instance, content_hash)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the copy was written and cannot be read back"))
}

fn copy_from_sqlite(row: &sqlx::sqlite::SqliteRow) -> SurvivorCopy {
    SurvivorCopy {
        id: row.get("id"),
        instance: row.get("instance"),
        content_hash: row.get("content_hash"),
        perceptual_hash: row.get("perceptual_hash"),
        width: row.get("width"),
        height: row.get("height"),
        recorded_at: row.get("recorded_at"),
    }
}

fn copy_from_postgres(row: &sqlx::postgres::PgRow) -> SurvivorCopy {
    SurvivorCopy {
        id: row.get("id"),
        instance: row.get("instance"),
        content_hash: row.get("content_hash"),
        perceptual_hash: row.get("perceptual_hash"),
        width: row.get("width"),
        height: row.get("height"),
        recorded_at: row.get("recorded_at"),
    }
}

const COPY_COLUMNS: &str =
    "id, instance, content_hash, perceptual_hash, CAST(width AS BIGINT) AS width, \
     CAST(height AS BIGINT) AS height, recorded_at";
const COPY_COLUMNS_SQLITE: &str =
    "id, instance, content_hash, perceptual_hash, width, height, recorded_at";

async fn find_copy(
    db: &Database,
    instance: &str,
    content_hash: &str,
) -> Result<Option<SurvivorCopy>> {
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&format!(
                "SELECT {COPY_COLUMNS_SQLITE} FROM media_rescue_copies WHERE instance = ? AND content_hash = ?"
            ))
            .bind(instance)
            .bind(content_hash)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.as_ref().map(copy_from_sqlite))
        }
        Backend::Postgres => {
            let row = sqlx::query(&format!(
                "SELECT {COPY_COLUMNS} FROM media_rescue_copies WHERE instance = $1 AND content_hash = $2"
            ))
            .bind(instance)
            .bind(content_hash)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.as_ref().map(copy_from_postgres))
        }
    }
}

/// Every survivor copy on record, newest claim first.
pub async fn list_copies(db: &Database, limit: i64) -> Result<Vec<SurvivorCopy>> {
    let limit = limit.clamp(1, 500);
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&format!(
                "SELECT {COPY_COLUMNS_SQLITE} FROM media_rescue_copies ORDER BY recorded_at DESC, id DESC LIMIT ?"
            ))
            .bind(limit)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows.iter().map(copy_from_sqlite).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(&format!(
                "SELECT {COPY_COLUMNS} FROM media_rescue_copies ORDER BY recorded_at DESC, id DESC LIMIT $1"
            ))
            .bind(limit)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows.iter().map(copy_from_postgres).collect())
        }
    }
}

fn request_from_sqlite(row: &sqlx::sqlite::SqliteRow) -> RescueRequest {
    RescueRequest {
        id: row.get("id"),
        dead_peer: row.get("dead_peer"),
        media_reference_id: row.get("media_reference_id"),
        content_hash: row.get("content_hash"),
        perceptual_hash: row.get("perceptual_hash"),
        threshold: row.get("threshold"),
        outcome: row.get("outcome"),
        matched_instance: row.get("matched_instance"),
        distance: row.get("distance"),
        confidence: row.get("confidence"),
        candidates_json: row.get("candidates_json"),
        created_at: row.get("created_at"),
        resolved_at: row.get("resolved_at"),
        resolved_by: row.get("resolved_by"),
        resolution: row.get("resolution"),
    }
}

fn request_from_postgres(row: &sqlx::postgres::PgRow) -> RescueRequest {
    RescueRequest {
        id: row.get("id"),
        dead_peer: row.get("dead_peer"),
        media_reference_id: row.get("media_reference_id"),
        content_hash: row.get("content_hash"),
        perceptual_hash: row.get("perceptual_hash"),
        threshold: row.get("threshold"),
        outcome: row.get("outcome"),
        matched_instance: row.get("matched_instance"),
        distance: row.get("distance"),
        confidence: row.get("confidence"),
        candidates_json: row.get("candidates_json"),
        created_at: row.get("created_at"),
        resolved_at: row.get("resolved_at"),
        resolved_by: row.get("resolved_by"),
        resolution: row.get("resolution"),
    }
}

const REQUEST_COLUMNS: &str = "id, dead_peer, media_reference_id, content_hash, perceptual_hash, \
     CAST(threshold AS BIGINT) AS threshold, outcome, matched_instance, \
     CAST(distance AS BIGINT) AS distance, confidence, candidates_json, created_at, \
     resolved_at, resolved_by, resolution";
const REQUEST_COLUMNS_SQLITE: &str =
    "id, dead_peer, media_reference_id, content_hash, perceptual_hash, threshold, outcome, matched_instance, \
     distance, confidence, candidates_json, created_at, resolved_at, resolved_by, resolution";

/// Run one hunt and record its outcome.
///
/// The `Wanted` is supplied rather than read here because the caller already has the media reference in hand, and a store
/// that fetched it again would be a second reader of the same row — the drift this row's sibling (M69-05) avoids by passing
/// the similarity it decided on.
pub async fn request(
    db: &Database,
    dead_peer: &str,
    media_reference_id: &str,
    wanted: &Wanted,
    threshold: u32,
    now: &str,
) -> Result<RescueRequest> {
    let copies = list_copies(db, 500).await?;
    let candidates: Vec<Copy> = copies
        .iter()
        .map(|copy| Copy {
            instance: copy.instance.clone(),
            content_hash: copy.content_hash.clone(),
            perceptual_hash: copy.perceptual_hash.clone(),
            width: copy.width.and_then(|value| u32::try_from(value).ok()),
            height: copy.height.and_then(|value| u32::try_from(value).ok()),
        })
        .collect();
    let outcome = rescue(wanted, &candidates, threshold);

    let (kind, matched, distance, confidence, candidates_json) = match &outcome {
        Outcome::Exact { instance } => (
            "exact",
            Some(instance.clone()),
            None,
            None,
            serde_json::json!([instance]).to_string(),
        ),
        Outcome::Proposal {
            instance,
            distance,
            confidence,
        } => (
            "proposal",
            Some(instance.clone()),
            Some(i64::from(*distance)),
            Some(*confidence),
            serde_json::json!([instance]).to_string(),
        ),
        Outcome::Ambiguous { instances, .. } => (
            "ambiguous",
            // Deliberately no name: choosing one of the tied survivors is the operator's call.
            None,
            None,
            None,
            serde_json::json!(instances).to_string(),
        ),
        Outcome::NotFound => ("not_found", None, None, None, "[]".to_owned()),
    };

    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO media_rescue_requests
                    (id, dead_peer, media_reference_id, content_hash, perceptual_hash, threshold,
                     outcome, matched_instance, distance, confidence, candidates_json, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(dead_peer)
            .bind(media_reference_id)
            .bind(&wanted.content_hash)
            .bind(wanted.perceptual_hash.as_deref())
            .bind(i64::from(threshold))
            .bind(kind)
            .bind(matched.as_deref())
            .bind(distance)
            .bind(confidence)
            .bind(&candidates_json)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO media_rescue_requests
                    (id, dead_peer, media_reference_id, content_hash, perceptual_hash, threshold,
                     outcome, matched_instance, distance, confidence, candidates_json, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6::bigint, $7, $8, $9::bigint, $10, $11, $12)",
            )
            .bind(&id)
            .bind(dead_peer)
            .bind(media_reference_id)
            .bind(&wanted.content_hash)
            .bind(wanted.perceptual_hash.as_deref())
            .bind(i64::from(threshold))
            .bind(kind)
            .bind(matched.as_deref())
            .bind(distance)
            .bind(confidence)
            .bind(&candidates_json)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }

    find(db, &id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the hunt was written and cannot be read back"))
}

/// One hunt, by id.
pub async fn find(db: &Database, id: &str) -> Result<Option<RescueRequest>> {
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(&format!(
                "SELECT {REQUEST_COLUMNS_SQLITE} FROM media_rescue_requests WHERE id = ?"
            ))
            .bind(id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.as_ref().map(request_from_sqlite))
        }
        Backend::Postgres => {
            let row = sqlx::query(&format!(
                "SELECT {REQUEST_COLUMNS} FROM media_rescue_requests WHERE id = $1"
            ))
            .bind(id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.as_ref().map(request_from_postgres))
        }
    }
}

/// Every hunt, newest first, settled ones included.
pub async fn list(db: &Database, limit: i64) -> Result<Vec<RescueRequest>> {
    let limit = limit.clamp(1, 500);
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&format!(
                "SELECT {REQUEST_COLUMNS_SQLITE} FROM media_rescue_requests ORDER BY created_at DESC, id DESC LIMIT ?"
            ))
            .bind(limit)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows.iter().map(request_from_sqlite).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(&format!(
                "SELECT {REQUEST_COLUMNS} FROM media_rescue_requests ORDER BY created_at DESC, id DESC LIMIT $1"
            ))
            .bind(limit)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows.iter().map(request_from_postgres).collect())
        }
    }
}

/// Settle a hunt: accepted, rejected or abandoned.
///
/// `matched_instance` is optional and only meaningful for an accepted hunt whose outcome was `ambiguous` — that is the
/// operator naming which survivor they chose, which is the whole point of leaving the ambiguity to them.
pub async fn resolve(
    db: &Database,
    id: &str,
    resolution: &str,
    by: &str,
    note_instance: Option<&str>,
    now: &str,
) -> Result<bool> {
    if !matches!(resolution, "accepted" | "rejected" | "abandoned") {
        return Err(anyhow::anyhow!(
            "a hunt settles as 'accepted', 'rejected' or 'abandoned', not '{resolution}'"
        ));
    }
    if matches!(resolution, "accepted") && note_instance.is_none() {
        // Accepting without a name is only honest when the ring already named one; `find` is the caller's job, so this
        // function takes the name it should store and refuses to store a blank one.
        return Err(anyhow::anyhow!(
            "accepting a rescue needs the survivor being credited; the ring refuses to invent one"
        ));
    }
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "UPDATE media_rescue_requests
                    SET resolved_at = ?, resolved_by = ?, resolution = ?,
                        matched_instance = COALESCE(?, matched_instance)
                  WHERE id = ? AND resolved_at IS NULL",
        )
        .bind(now)
        .bind(by)
        .bind(resolution)
        .bind(note_instance)
        .bind(id)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await?
        .rows_affected(),
        Backend::Postgres => sqlx::query(
            "UPDATE media_rescue_requests
                    SET resolved_at = $1, resolved_by = $2, resolution = $3,
                        matched_instance = COALESCE($4, matched_instance)
                  WHERE id = $5 AND resolved_at IS NULL",
        )
        .bind(now)
        .bind(by)
        .bind(resolution)
        .bind(note_instance)
        .bind(id)
        .execute(db.postgres_pool().expect("postgres"))
        .await?
        .rows_affected(),
    };
    Ok(affected > 0)
}

//! M45-37 — the store for stable identifiers.
//!
//! Two behaviours carry the row:
//!
//! * **`attach` reports who already holds an identifier instead of failing.** The primary key is
//!   `(scheme, value)`, so a second claim is a conflict by construction; the store reads the holder back and
//!   returns it, which lets the route answer "already claimed by this work" rather than "duplicate key". A
//!   caller learns the identity the row exists to establish.
//! * **Re-attaching the same identifier to the same target is idempotent.** An import that runs twice must not
//!   turn into a uniqueness error on its second pass; `ON CONFLICT DO UPDATE SET source = ...` makes the second
//!   call a provenance update, not a failure.

use commons_identify::{canonical, Identified, Scheme};

use crate::{sql_owned, Backend, Database, Result};

/// Where an identifier points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Work(String),
    Edition(String),
}

impl Target {
    fn work_id(&self) -> Option<&str> {
        match self {
            Self::Work(id) => Some(id),
            Self::Edition(_) => None,
        }
    }

    fn edition_id(&self) -> Option<&str> {
        match self {
            Self::Work(_) => None,
            Self::Edition(id) => Some(id),
        }
    }
}

/// One identifier row.
#[derive(Debug, Clone, PartialEq)]
pub struct Identifier {
    pub scheme: Scheme,
    /// Canonical spelling, as `commons_identify` produced it.
    pub value: String,
    pub work_id: Option<String>,
    pub edition_id: Option<String>,
    /// `imported`, `curator`, `quorum` or `author`.
    pub source: String,
    pub created_at: String,
}

/// What `attach` did.
#[derive(Debug, Clone, PartialEq)]
pub enum Attached {
    /// Written, or re-written onto the same target.
    Stored(Identifier),
    /// Another target already claims it; the holder is returned so the caller can say so.
    Taken(Identifier),
    /// The input is not a valid identifier of that scheme, with the sentence explaining why.
    Refused(String),
}

/// Attach `raw` (as `scheme`) to `target`.
///
/// # Errors
/// As the store.
pub async fn attach(
    db: &Database,
    scheme: Scheme,
    raw: &str,
    target: &Target,
    source: &str,
) -> Result<Attached> {
    let canonical_value = match canonical(scheme, raw) {
        Identified::Canonical(value) => value,
        Identified::Refused(reason) => return Ok(Attached::Refused(reason)),
    };

    if let Some(existing) = lookup(db, scheme, &canonical_value).await? {
        // Held by a different target: the answer is who holds it, not a write.
        let same_target = existing.work_id.as_deref() == target.work_id()
            && existing.edition_id.as_deref() == target.edition_id();
        if !same_target {
            return Ok(Attached::Taken(existing));
        }
        // Same identity, same target: a re-import. It falls through to the upsert so that a second pass
        // with better provenance (`imported` → `author`) updates the row instead of being discarded.
    }

    let now = crate::identity::now_rfc3339();
    let sql = sql_owned(
        db,
        "INSERT INTO entity_identifiers (scheme, value, work_id, edition_id, source, created_at) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT (scheme, value) DO UPDATE SET source = excluded.source"
            .to_owned(),
        "INSERT INTO entity_identifiers (scheme, value, work_id, edition_id, source, created_at) \
         VALUES ($1, $2, $3::uuid, $4::uuid, $5, $6) \
         ON CONFLICT (scheme, value) DO UPDATE SET source = excluded.source"
            .to_owned(),
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(scheme.as_str())
                .bind(&canonical_value)
                .bind(target.work_id())
                .bind(target.edition_id())
                .bind(source)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(scheme.as_str())
                .bind(&canonical_value)
                .bind(target.work_id())
                .bind(target.edition_id())
                .bind(source)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    match lookup(db, scheme, &canonical_value).await? {
        Some(stored) => Ok(Attached::Stored(stored)),
        None => Ok(Attached::Refused(
            "the identifier vanished between insert and read".to_owned(),
        )),
    }
}

/// The row for `(scheme, canonical value)`, if any.
///
/// # Errors
/// As the store.
pub async fn lookup(db: &Database, scheme: Scheme, raw: &str) -> Result<Option<Identifier>> {
    let Some(value) = canonical(scheme, raw).value().map(str::to_owned) else {
        return Ok(None);
    };
    let sql = sql_owned(
        db,
        "SELECT scheme, value, work_id, edition_id, source, created_at \
           FROM entity_identifiers WHERE scheme = ? AND value = ?".to_owned(),
        "SELECT scheme, value, work_id::text AS work_id, edition_id::text AS edition_id, source, created_at \
           FROM entity_identifiers WHERE scheme = $1 AND value = $2"
            .to_owned(),
    );
    type Row6 = (
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    );
    let row: Option<Row6> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(scheme.as_str())
                .bind(&value)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(scheme.as_str())
                .bind(&value)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let Some((scheme_word, value, work_id, edition_id, source, created_at)) = row else {
        return Ok(None);
    };
    let Some(parsed) = Scheme::parse(&scheme_word) else {
        return Ok(None);
    };
    Ok(Some(Identifier {
        scheme: parsed,
        value,
        work_id,
        edition_id,
        source,
        created_at,
    }))
}

/// Everything a work claims, in scheme order then value order, so a page renders deterministically.
///
/// # Errors
/// As the store.
pub async fn for_work(db: &Database, work_id: &str) -> Result<Vec<Identifier>> {
    let sql = sql_owned(
        db,
        "SELECT scheme, value, work_id, edition_id, source, created_at \
           FROM entity_identifiers WHERE work_id = ? ORDER BY scheme, value".to_owned(),
        "SELECT scheme, value, work_id::text AS work_id, edition_id::text AS edition_id, source, created_at \
           FROM entity_identifiers WHERE work_id = $1::uuid ORDER BY scheme, value"
            .to_owned(),
    );
    type Row6 = (
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    );
    let rows: Vec<Row6> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .filter_map(|(scheme, value, work_id, edition_id, source, created_at)| {
            Some(Identifier {
                scheme: Scheme::parse(&scheme)?,
                value,
                work_id,
                edition_id,
                source,
                created_at,
            })
        })
        .collect())
}

/// Detach one identifier from a work, when the work holds it.
///
/// Returns `false` when the work does not hold it — including when another work does, which must not be
/// detachable through this path.
///
/// # Errors
/// As the store.
pub async fn detach(db: &Database, scheme: Scheme, raw: &str, target: &Target) -> Result<bool> {
    let Some(value) = canonical(scheme, raw).value().map(str::to_owned) else {
        return Ok(false);
    };
    let sql = sql_owned(
        db,
        "DELETE FROM entity_identifiers WHERE scheme = ? AND value = ? \
           AND ((? IS NULL AND work_id IS NULL) OR work_id = ?) \
           AND ((? IS NULL AND edition_id IS NULL) OR edition_id = ?)"
            .to_owned(),
        "DELETE FROM entity_identifiers WHERE scheme = $1 AND value = $2 \
           AND (($3::uuid IS NULL AND work_id IS NULL) OR work_id = $3::uuid) \
           AND (($4::uuid IS NULL AND edition_id IS NULL) OR edition_id = $4::uuid)"
            .to_owned(),
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(scheme.as_str())
            .bind(&value)
            .bind(target.work_id())
            .bind(target.work_id())
            .bind(target.edition_id())
            .bind(target.edition_id())
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(scheme.as_str())
            .bind(&value)
            .bind(target.work_id())
            .bind(target.edition_id())
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

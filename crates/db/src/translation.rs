//! M17 — Translation repository: jobs, units, memory, glossaries, reviews, publications.

use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{Backend, Database};
use darkhold_domain::translation::{self, ReviewGate, TranslationJobState, TranslationUnitState};

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

pub async fn create_job(
    db: &Database,
    source_work: &str,
    source_lang: &str,
    target_lang: &str,
    provider: &str,
    quote_transaction: Option<&str>,
    created_by: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO translation_jobs (id, source_work, source_lang, target_lang, provider, quote_transaction, state, created_by, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, 'quoted', ?, ?, ?)"
            )
            .bind(&id).bind(source_work).bind(source_lang).bind(target_lang).bind(provider).bind(quote_transaction).bind(created_by).bind(&now).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO translation_jobs (id, source_work, source_lang, target_lang, provider, quote_transaction, state, created_by, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, 'quoted', $7, $8, $9)"
            )
            .bind(&id).bind(source_work).bind(source_lang).bind(target_lang).bind(provider).bind(quote_transaction).bind(created_by).bind(&now).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(id)
}

async fn get_job_sqlite(
    pool: &sqlx::SqlitePool,
    job_id: &str,
) -> Result<Option<Value>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM translation_jobs WHERE id = ?")
        .bind(job_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "source_work": r.get::<String, _>("source_work"),
            "source_lang": r.get::<String, _>("source_lang"),
            "target_lang": r.get::<String, _>("target_lang"),
            "provider": r.get::<String, _>("provider"),
            "quote_transaction": r.get::<Option<String>, _>("quote_transaction"),
            "state": r.get::<String, _>("state"),
            "created_by": r.get::<String, _>("created_by"),
            "created_at": r.get::<String, _>("created_at"),
            "updated_at": r.get::<String, _>("updated_at"),
        })
    }))
}

async fn get_job_postgres(
    pool: &sqlx::postgres::PgPool,
    job_id: &str,
) -> Result<Option<Value>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM translation_jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "source_work": r.get::<String, _>("source_work"),
            "source_lang": r.get::<String, _>("source_lang"),
            "target_lang": r.get::<String, _>("target_lang"),
            "provider": r.get::<String, _>("provider"),
            "quote_transaction": r.get::<Option<String>, _>("quote_transaction"),
            "state": r.get::<String, _>("state"),
            "created_by": r.get::<String, _>("created_by"),
            "created_at": r.get::<String, _>("created_at"),
            "updated_at": r.get::<String, _>("updated_at"),
        })
    }))
}

pub async fn get_job(db: &Database, job_id: &str) -> Result<Option<Value>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => get_job_sqlite(db.sqlite_pool().expect("sqlite"), job_id).await,
        Backend::Postgres => get_job_postgres(db.postgres_pool().expect("postgres"), job_id).await,
    }
}

pub async fn transition_job(
    db: &Database,
    job_id: &str,
    from: &TranslationJobState,
    to: &TranslationJobState,
) -> Result<(), sqlx::Error> {
    if !translation::valid_job_transition(from, to) {
        return Err(sqlx::Error::Protocol(format!(
            "invalid transition: {:?} -> {:?}",
            from, to
        )));
    }

    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "UPDATE translation_jobs SET state = ?, updated_at = ? WHERE id = ? AND state = ?",
            )
            .bind(to.as_str())
            .bind(&now)
            .bind(job_id)
            .bind(from.as_str())
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query("UPDATE translation_jobs SET state = $1, updated_at = $2 WHERE id = $3 AND state = $4")
                .bind(to.as_str()).bind(&now).bind(job_id).bind(from.as_str())
                .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn upsert_unit(
    db: &Database,
    job_id: &str,
    unit_id: &str,
    paragraph_index: i64,
    source_text: &str,
    target_text: Option<&str>,
    state: &TranslationUnitState,
    memory_hit: Option<&str>,
) -> Result<(), sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO translation_units (id, job_id, unit_id, paragraph_index, source_text, target_text, state, memory_hit, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(job_id, unit_id, paragraph_index) DO UPDATE SET
                    target_text = excluded.target_text, state = excluded.state, memory_hit = excluded.memory_hit, updated_at = excluded.updated_at"
            )
            .bind(&id).bind(job_id).bind(unit_id).bind(paragraph_index).bind(source_text).bind(target_text).bind(state.as_str()).bind(memory_hit).bind(&now).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO translation_units (id, job_id, unit_id, paragraph_index, source_text, target_text, state, memory_hit, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT(job_id, unit_id, paragraph_index) DO UPDATE SET
                    target_text = EXCLUDED.target_text, state = EXCLUDED.state, memory_hit = EXCLUDED.memory_hit, updated_at = EXCLUDED.updated_at"
            )
            .bind(&id).bind(job_id).bind(unit_id).bind(paragraph_index).bind(source_text).bind(target_text).bind(state.as_str()).bind(memory_hit).bind(&now).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(())
}

async fn units_for_job_sqlite(
    pool: &sqlx::SqlitePool,
    job_id: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT * FROM translation_units WHERE job_id = ? ORDER BY unit_id, paragraph_index",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "job_id": r.get::<String, _>("job_id"),
                "unit_id": r.get::<String, _>("unit_id"),
                "paragraph_index": r.get::<i32, _>("paragraph_index"),
                "source_text": r.get::<String, _>("source_text"),
                "target_text": r.get::<Option<String>, _>("target_text"),
                "state": r.get::<String, _>("state"),
                "memory_hit": r.get::<Option<String>, _>("memory_hit"),
            })
        })
        .collect())
}

async fn units_for_job_postgres(
    pool: &sqlx::postgres::PgPool,
    job_id: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT * FROM translation_units WHERE job_id = $1 ORDER BY unit_id, paragraph_index",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "job_id": r.get::<String, _>("job_id"),
                "unit_id": r.get::<String, _>("unit_id"),
                "paragraph_index": r.get::<i32, _>("paragraph_index"),
                "source_text": r.get::<String, _>("source_text"),
                "target_text": r.get::<Option<String>, _>("target_text"),
                "state": r.get::<String, _>("state"),
                "memory_hit": r.get::<Option<String>, _>("memory_hit"),
            })
        })
        .collect())
}

pub async fn units_for_job(db: &Database, job_id: &str) -> Result<Vec<Value>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => units_for_job_sqlite(db.sqlite_pool().expect("sqlite"), job_id).await,
        Backend::Postgres => {
            units_for_job_postgres(db.postgres_pool().expect("postgres"), job_id).await
        }
    }
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn add_memory(
    db: &Database,
    owner: &str,
    source_lang: &str,
    target_lang: &str,
    source_hash: &str,
    source_text: &str,
    target_text: &str,
    quality_bp: i64,
    shared: bool,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO translation_memory (id, owner, source_lang, target_lang, source_hash, source_text, target_text, quality_bp, shared, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&id).bind(owner).bind(source_lang).bind(target_lang).bind(source_hash).bind(source_text).bind(target_text).bind(quality_bp).bind(if shared { 1i32 } else { 0i32 }).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO translation_memory (id, owner, source_lang, target_lang, source_hash, source_text, target_text, quality_bp, shared, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
            )
            .bind(&id).bind(owner).bind(source_lang).bind(target_lang).bind(source_hash).bind(source_text).bind(target_text).bind(quality_bp).bind(if shared { 1i32 } else { 0i32 }).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(id)
}

async fn lookup_memory_sqlite(
    pool: &sqlx::SqlitePool,
    owner: &str,
    source_lang: &str,
    target_lang: &str,
    source_hash: &str,
) -> Result<Option<(String, i64, bool)>, sqlx::Error> {
    let row = sqlx::query("SELECT target_text, quality_bp, shared FROM translation_memory WHERE owner = ? AND source_lang = ? AND target_lang = ? AND source_hash = ?")
        .bind(owner).bind(source_lang).bind(target_lang).bind(source_hash)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| {
        (
            r.get::<String, _>("target_text"),
            r.get::<i32, _>("quality_bp") as i64,
            r.get::<i32, _>("shared") != 0,
        )
    }))
}

async fn lookup_memory_postgres(
    pool: &sqlx::postgres::PgPool,
    owner: &str,
    source_lang: &str,
    target_lang: &str,
    source_hash: &str,
) -> Result<Option<(String, i64, bool)>, sqlx::Error> {
    let row = sqlx::query("SELECT target_text, CAST(quality_bp AS BIGINT), CAST(shared AS BIGINT) FROM translation_memory WHERE owner = $1 AND source_lang = $2 AND target_lang = $3 AND source_hash = $4")
        .bind(owner).bind(source_lang).bind(target_lang).bind(source_hash)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| {
        (
            r.get::<String, _>("target_text"),
            // The SELECT casts both to BIGINT, so the reader is `i64`. Reading
            // them as `i32` is `INT4 is not compatible with INT8` -- a decode
            // error, not a truncation. The SQLite arm selects the columns
            // uncast and correctly reads `i32`.
            r.get::<i64, _>("quality_bp"),
            r.get::<i64, _>("shared") != 0,
        )
    }))
}

pub async fn lookup_memory(
    db: &Database,
    owner: &str,
    source_lang: &str,
    target_lang: &str,
    source_hash: &str,
) -> Result<Option<(String, i64, bool)>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            lookup_memory_sqlite(
                db.sqlite_pool().expect("sqlite"),
                owner,
                source_lang,
                target_lang,
                source_hash,
            )
            .await
        }
        Backend::Postgres => {
            lookup_memory_postgres(
                db.postgres_pool().expect("postgres"),
                owner,
                source_lang,
                target_lang,
                source_hash,
            )
            .await
        }
    }
}

// ---------------------------------------------------------------------------
// Memory listing
// ---------------------------------------------------------------------------

pub async fn list_memory(db: &Database, owner: &str) -> Result<Vec<Value>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query("SELECT id, owner, source_lang, target_lang, source_hash, source_text, target_text, quality_bp, shared, created_at FROM translation_memory WHERE owner = ? ORDER BY created_at DESC")
                .bind(owner)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "owner": r.get::<String, _>("owner"),
                        "source_lang": r.get::<String, _>("source_lang"),
                        "target_lang": r.get::<String, _>("target_lang"),
                        "source_hash": r.get::<String, _>("source_hash"),
                        "source_text": r.get::<String, _>("source_text"),
                        "target_text": r.get::<String, _>("target_text"),
                        "quality_bp": r.get::<i64, _>("quality_bp"),
                        "shared": r.get::<i64, _>("shared") == 1,
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query("SELECT id, owner, source_lang, target_lang, source_hash, source_text, target_text, CAST(quality_bp AS BIGINT), CAST(shared AS BIGINT), created_at FROM translation_memory WHERE owner = $1 ORDER BY created_at DESC")
                .bind(owner)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "owner": r.get::<String, _>("owner"),
                        "source_lang": r.get::<String, _>("source_lang"),
                        "target_lang": r.get::<String, _>("target_lang"),
                        "source_hash": r.get::<String, _>("source_hash"),
                        "source_text": r.get::<String, _>("source_text"),
                        "target_text": r.get::<String, _>("target_text"),
                        "quality_bp": r.get::<i64, _>("quality_bp"),
                        "shared": r.get::<i64, _>("shared") == 1,
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Glossaries
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn add_glossary_term(
    db: &Database,
    owner: &str,
    work_id: Option<&str>,
    source_lang: &str,
    target_lang: &str,
    term: &str,
    translation: &str,
    case_sensitive: bool,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO translation_glossaries (id, owner, work_id, source_lang, target_lang, term, translation, case_sensitive, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&id).bind(owner).bind(work_id).bind(source_lang).bind(target_lang).bind(term).bind(translation).bind(if case_sensitive { 1i32 } else { 0i32 }).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO translation_glossaries (id, owner, work_id, source_lang, target_lang, term, translation, case_sensitive, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
            )
            .bind(&id).bind(owner).bind(work_id).bind(source_lang).bind(target_lang).bind(term).bind(translation).bind(if case_sensitive { 1i32 } else { 0i32 }).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Reviews
// ---------------------------------------------------------------------------

pub async fn open_review_gate(
    db: &Database,
    job_id: &str,
    reviewer: &str,
    gate: &ReviewGate,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO translation_reviews (id, job_id, reviewer, gate, state, created_at)
                 VALUES (?, ?, ?, ?, 'pending', ?)",
            )
            .bind(&id)
            .bind(job_id)
            .bind(reviewer)
            .bind(gate.as_str())
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO translation_reviews (id, job_id, reviewer, gate, state, created_at)
                 VALUES ($1, $2, $3, $4, 'pending', $5)",
            )
            .bind(&id)
            .bind(job_id)
            .bind(reviewer)
            .bind(gate.as_str())
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(id)
}

pub async fn decide_review_gate(
    db: &Database,
    review_id: &str,
    state: &str,
    notes: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "UPDATE translation_reviews SET state = ?, notes = ?, decided_at = ? WHERE id = ?",
            )
            .bind(state)
            .bind(notes)
            .bind(&now)
            .bind(review_id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query("UPDATE translation_reviews SET state = $1, notes = $2, decided_at = $3 WHERE id = $4")
                .bind(state).bind(notes).bind(&now).bind(review_id)
                .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Publications
// ---------------------------------------------------------------------------

pub async fn record_publication(
    db: &Database,
    job_id: &str,
    work_id: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO translation_publications (id, job_id, work_id, published_at)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(job_id)
            .bind(work_id)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO translation_publications (id, job_id, work_id, published_at)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(&id)
            .bind(job_id)
            .bind(work_id)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// §61 — cross-language supply (M45-30)
// ---------------------------------------------------------------------------

/// How much of the catalogue is in each language, counted from `works.language`.
///
/// The whole catalogue, not a sample: a supply report that samples is a report about the sampler.
/// Works whose tag is unusable land in `unknown` rather than being dropped, so `total` always equals
/// the number of works.
pub async fn language_supply(
    db: &Database,
) -> Result<darkhold_domain::translation::Supply, sqlx::Error> {
    let sql = db.sql(
        "SELECT language, COUNT(*) AS n FROM works GROUP BY language",
        "SELECT language, COUNT(*) AS n FROM works GROUP BY language",
    );
    let rows: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?
            .iter()
            .map(|r| (r.get::<String, _>("language"), r.get::<i64, _>("n")))
            .collect(),
        Backend::Postgres => sqlx::query(&sql)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?
            .iter()
            .map(|r| (r.get::<String, _>("language"), r.get::<i64, _>("n")))
            .collect(),
    };
    let mut supply = darkhold_domain::translation::Supply::default();
    for (language, count) in rows {
        supply.add(Some(&language), count);
    }
    Ok(supply)
}

/// What happened when a patron tried to fund a translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FundOutcome {
    Funded {
        bounty_id: String,
    },
    /// `translation: ask` — the author must answer first.
    MustAsk,
    /// `translation: no` or `unstated`, with the statement named for the refusal.
    Forbidden {
        statement: &'static str,
    },
    /// The account cannot cover the amount it is promising.
    InsufficientBalance {
        available: i64,
        required: i64,
    },
}

/// Fund a translation of `work_id` into `target_lang`, on the existing `bounties` table.
///
/// Two gates, in this order, and the order matters: **permission before money**. A refusal that
/// mentioned a balance first would tell a patron to top up for something the author has forbidden.
///
/// The bounty is written with `job_kind = 'translation'` and terms naming the work and both
/// languages; `create_bounty` is the §20.3 path and this function does not invent a second one. The
/// balance check exists because a bounty from an account that cannot pay is a promise the archive
/// would have to break.
pub async fn fund_translation(
    db: &Database,
    work_id: &str,
    target_lang: &str,
    account: &str,
    amount: i64,
) -> Result<FundOutcome, sqlx::Error> {
    let permission = crate::permission::get_work_permission_statement(db, work_id)
        .await
        .map_err(|e| sqlx::Error::Protocol(format!("{e}")))?;
    match darkhold_domain::translation::funding_gate(permission.translation) {
        darkhold_domain::translation::Funding::Fundable => {}
        darkhold_domain::translation::Funding::MustAsk => return Ok(FundOutcome::MustAsk),
        darkhold_domain::translation::Funding::Forbidden { statement } => {
            return Ok(FundOutcome::Forbidden { statement })
        }
    }

    let available: i64 = crate::economy::balances(db, account)
        .await?
        .into_iter()
        .map(|(_, amount)| amount)
        .sum();
    if available < amount {
        return Ok(FundOutcome::InsufficientBalance {
            available,
            required: amount,
        });
    }

    // The source language is the work's own, so a translator is never told to guess it.
    let source_lang: String = match db.backend() {
        Backend::Sqlite => sqlx::query("SELECT language FROM works WHERE id = ?")
            .bind(work_id)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await?
            .get(0),
        Backend::Postgres => sqlx::query("SELECT language FROM works WHERE id::text = $1")
            .bind(work_id)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await?
            .get(0),
    };

    let terms = serde_json::json!({
        "work_id": work_id,
        "source_language": darkhold_domain::translation::normalise_language(&source_lang)
            .unwrap_or(source_lang),
        "target_language": target_lang,
    })
    .to_string();

    // `create_bounty` takes the id and the timestamp from its caller, and returns nothing.
    let bounty_id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();
    crate::economy::create_bounty(db, &bounty_id, account, "translation", &terms, amount, &now)
        .await?;
    Ok(FundOutcome::Funded { bounty_id })
}

/// Open translation bounties, newest first.
pub async fn translation_bounties(db: &Database, limit: i64) -> Result<Vec<Value>, sqlx::Error> {
    let sql = db.sql(
        // `amount` is INT4 on PostgreSQL and INT8 on SQLite; the cast is what lets one Rust type
        // read both (the same trap `likes` set).
        "SELECT id, terms, state, created_by, created_at, account, CAST(amount AS BIGINT) AS amount
           FROM bounties WHERE job_kind = 'translation' AND state = 'open'
          ORDER BY created_at DESC, id LIMIT ?",
        "SELECT id, terms, state, created_by, created_at, account, CAST(amount AS BIGINT) AS amount
           FROM bounties WHERE job_kind = 'translation' AND state = 'open'
          ORDER BY created_at DESC, id LIMIT $1",
    );
    // Mapping happens inside each arm: the two drivers have different row types, and a generic
    // helper cannot satisfy `Row::get` for both.
    let out: Vec<Value> = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(limit)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.get::<String, _>("id"),
                    "terms": r.get::<String, _>("terms"),
                    "state": r.get::<String, _>("state"),
                    "created_by": r.get::<String, _>("created_by"),
                    "created_at": r.get::<String, _>("created_at"),
                    "amount": r.get::<i64, _>("amount"),
                })
            })
            .collect(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(limit)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.get::<String, _>("id"),
                    "terms": r.get::<String, _>("terms"),
                    "state": r.get::<String, _>("state"),
                    "created_by": r.get::<String, _>("created_by"),
                    "created_at": r.get::<String, _>("created_at"),
                    "amount": r.get::<i64, _>("amount"),
                })
            })
            .collect(),
    };
    Ok(out)
}

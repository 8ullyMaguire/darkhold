//! The reader's own gravity dial (M64-09, spec §16.5).
//!
//! Three levels, from the preference ladder (M69-26): the platform fixes the vocabulary, the operator sets the instance's
//! pull, and the reader may **narrow** their own. Narrowing is the whole of the reader's power here — they can ask for less
//! gravity than the instance runs, never more. A reader who could turn gravity up would be choosing a stronger editorial hand
//! than the operator configured, which is the opposite of the dial's purpose.
//!
//! A stored value is clamped to the instance's strength **on read**, not on write: an operator who lowers the instance value
//! must not leave every reader pinned above the new number by rows that were legal when they were written.

use crate::{Backend, Database, Result};
use uuid::Uuid;

/// What a reader asked for, if they ever asked.
pub async fn read(db: &Database, pseud_id: Uuid) -> Result<Option<f64>> {
    let sql = match db.backend() {
        Backend::Sqlite => "SELECT strength FROM reader_gravity WHERE pseud_id = ?1",
        Backend::Postgres => "SELECT strength FROM reader_gravity WHERE pseud_id = $1",
    };
    Ok(match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(sql)
                .bind(pseud_id.to_string())
                .fetch_optional(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(sql)
                .bind(pseud_id)
                .fetch_optional(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    })
}

/// Store a reader's choice. `strength` is expected to be already narrowed by the caller's policy.
pub async fn set(db: &Database, pseud_id: Uuid, strength: f64, now: &str) -> Result<()> {
    let sql = match db.backend() {
        Backend::Sqlite => {
            "INSERT INTO reader_gravity (pseud_id, strength, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT (pseud_id) DO UPDATE SET strength = excluded.strength, updated_at = excluded.updated_at"
        }
        Backend::Postgres => {
            "INSERT INTO reader_gravity (pseud_id, strength, updated_at) VALUES ($1::uuid, $2, $3::timestamptz) \
             ON CONFLICT (pseud_id) DO UPDATE SET strength = EXCLUDED.strength, updated_at = EXCLUDED.updated_at"
        }
    };
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(sql)
                .bind(pseud_id.to_string())
                .bind(strength)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(sql)
                .bind(pseud_id)
                .bind(strength)
                .bind(now)
                .execute(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?;
        }
    }
    Ok(())
}

/// Forget a reader's choice: back to the instance's value. "No preference" and "0.0" are different answers, which is why this
/// is a delete and not `set(0.0)`.
pub async fn clear(db: &Database, pseud_id: Uuid) -> Result<bool> {
    let sql = match db.backend() {
        Backend::Sqlite => "DELETE FROM reader_gravity WHERE pseud_id = ?1",
        Backend::Postgres => "DELETE FROM reader_gravity WHERE pseud_id = $1",
    };
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(sql)
            .bind(pseud_id.to_string())
            .execute(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(sql)
            .bind(pseud_id)
            .execute(
                db.postgres_pool()
                    .expect("postgres pool for postgres backend"),
            )
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

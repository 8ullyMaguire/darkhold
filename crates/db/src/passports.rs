//! M69-03 — the store half of a taste passport: verify it, record it, and seed taste as *imported* evidence.
//!
//! The order is the design. A passport is verified **before** anything is written, so a forged document leaves no row to
//! clean up and no signal to un-import; then the receipt is written, then the taste is seeded, then the receipt learns how
//! much it seeded. A failure between those steps leaves a claim with `signals = 0`, which is a true statement ("we accepted
//! this and imported nothing") rather than a half-imported history nobody can explain.
//!
//! # Why the signals are `imported` and not `organic`
//!
//! `taste_signals` (0101) already distinguishes the two, and §49.7's invariant is that they stay distinguishable
//! *everywhere*. A passport is somebody else's measurement of this reader: it is better than a blank page and worse than a
//! bookmark this instance watched happen, and the column exists so the difference is not a matter of trust.
//!
//! # Why re-importing replaces
//!
//! A passport is a statement about now. Keeping the previous document's signals would let the same history count twice with
//! two different weights, and "the sum of every passport a reader ever presented" is not a taste — it is a drift toward
//! whoever they were.

use anyhow::Result;
use sqlx::Row as _;

use darkhold_domain::passport::{self, Passport, SignedPassport};

use crate::{Backend, Database};

/// What an import did.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    /// The document's id: the hash of its canonical bytes.
    pub id: String,
    /// The instance that issued it.
    pub origin: String,
    /// The reader's handle at the origin.
    pub subject: String,
    /// When the origin signed it.
    pub issued_at: String,
    /// How many taste signals this document seeded.
    pub signals: usize,
    /// The tag vector the receiving instance reads, in basis points.
    pub taste_bp: std::collections::BTreeMap<String, u32>,
}

/// A recorded passport, as the reader's own page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Recorded {
    /// The document's id.
    pub id: String,
    /// The instance that issued it.
    pub origin: String,
    /// The key it was signed with.
    pub key: String,
    /// When the origin signed it.
    pub issued_at: String,
    /// When this instance received it.
    pub received_at: String,
    /// How many signals it seeded.
    pub signals: i64,
}

/// Verify and record a signed passport, seeding its taste as imported evidence.
pub async fn import(
    db: &Database,
    account: &str,
    signed: &SignedPassport,
    now: &str,
) -> Result<Receipt> {
    let at = time::OffsetDateTime::parse(now, &time::format_description::well_known::Rfc3339)
        .map_err(|error| anyhow::anyhow!("the clock is not RFC 3339: {error}"))?;
    // Verification first: a forged document must not create a row, an event, or a signal.
    let passport: &Passport = passport::verify(signed, at).map_err(|error| anyhow::anyhow!("{error}"))?;

    let id = passport::document_id(passport);
    let document = serde_json::to_string(signed)?;
    let taste = passport::normalised(passport);

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO taste_passports
                    (id, account_id, origin, key, subject, document, issued_at, received_at, signals)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)
                 ON CONFLICT(account_id, origin) DO UPDATE SET
                    id = excluded.id,
                    key = excluded.key,
                    subject = excluded.subject,
                    document = excluded.document,
                    issued_at = excluded.issued_at,
                    received_at = excluded.received_at,
                    signals = 0",
            )
            .bind(&id)
            .bind(account)
            .bind(&passport.origin)
            .bind(&signed.key)
            .bind(&passport.subject)
            .bind(&document)
            .bind(&passport.issued_at)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO taste_passports
                    (id, account_id, origin, key, subject, document, issued_at, received_at, signals)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0)
                 ON CONFLICT(account_id, origin) DO UPDATE SET
                    id = EXCLUDED.id,
                    key = EXCLUDED.key,
                    subject = EXCLUDED.subject,
                    document = EXCLUDED.document,
                    issued_at = EXCLUDED.issued_at,
                    received_at = EXCLUDED.received_at,
                    signals = 0",
            )
            .bind(&id)
            .bind(account)
            .bind(&passport.origin)
            .bind(&signed.key)
            .bind(&passport.subject)
            .bind(&document)
            .bind(&passport.issued_at)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }

    // `taste_signals.account_id` is a UUID (it references `accounts`), while `taste_passports.account_id` is TEXT like every
    // other M69 table — so the PostgreSQL arm casts the parameter in the statements that touch `taste_signals`, and only
    // there. Getting this backwards fails with `operator does not exist: uuid = text`, which is what the first run said.
    // Replace this origin's previous signals before seeding: two documents from one origin are two versions of one prior.
    let source_key = format!("passport:{}", passport.origin);
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("DELETE FROM taste_signals WHERE account_id = ? AND source_key = ? AND origin = 'imported'")
                .bind(account)
                .bind(&source_key)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query("DELETE FROM taste_signals WHERE account_id = $1::uuid AND source_key = $2 AND origin = 'imported'")
                .bind(account)
                .bind(&source_key)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }

    let provenance = serde_json::json!({
        "passport": id,
        "origin": passport.origin,
        "key": signed.key,
        "issued_at": passport.issued_at,
    })
    .to_string();

    let mut seeded = 0usize;
    for (tag, weight_bp) in &taste {
        let dimension = format!("tag:{tag}");
        // The signal value is the basis-point share as a 0..1 number, the same units the rest of `taste_signals` uses: the
        // discount for imported evidence is applied once, at read time (§49.6), not baked in here.
        let value = f64::from(*weight_bp) / 10_000.0;
        let inserted = match db.backend() {
            Backend::Sqlite => {
                sqlx::query(
                    "INSERT INTO taste_signals
                        (id, account_id, origin, signal_kind, dimension_key, signal_value, occurred_at,
                         source_key, source_signal_key, provenance_json, created_at, updated_at)
                     VALUES (?, ?, 'imported', 'rating', ?, ?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(account_id, source_key, source_signal_key, dimension_key) DO NOTHING",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(account)
                .bind(&dimension)
                .bind(value)
                .bind(&passport.issued_at)
                .bind(&source_key)
                .bind(format!("{id}:{tag}"))
                .bind(&provenance)
                .bind(now)
                .bind(now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?
                .rows_affected()
            }
            Backend::Postgres => {
                sqlx::query(
                    "INSERT INTO taste_signals
                        (id, account_id, origin, signal_kind, dimension_key, signal_value, occurred_at,
                         source_key, source_signal_key, provenance_json, created_at, updated_at)
                     VALUES ($1::uuid, $2::uuid, 'imported', 'rating', $3, $4, $5, $6, $7, $8, $9, $9)
                     ON CONFLICT(account_id, source_key, source_signal_key, dimension_key) DO NOTHING",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(account)
                .bind(&dimension)
                .bind(value)
                .bind(&passport.issued_at)
                .bind(&source_key)
                .bind(format!("{id}:{tag}"))
                .bind(&provenance)
                .bind(now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?
                .rows_affected()
            }
        };
        seeded += usize::try_from(inserted).unwrap_or(0);
    }

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("UPDATE taste_passports SET signals = ? WHERE account_id = ? AND origin = ?")
                .bind(i64::try_from(seeded).unwrap_or(0))
                .bind(account)
                .bind(&passport.origin)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query("UPDATE taste_passports SET signals = $1 WHERE account_id = $2 AND origin = $3")
                .bind(i64::try_from(seeded).unwrap_or(0))
                .bind(account)
                .bind(&passport.origin)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }

    Ok(Receipt {
        id,
        origin: passport.origin.clone(),
        subject: passport.subject.clone(),
        issued_at: passport.issued_at.clone(),
        signals: seeded,
        taste_bp: taste,
    })
}

/// Every passport this account has presented, newest first.
pub async fn for_account(db: &Database, account: &str) -> Result<Vec<Recorded>> {
    let rows = match db.backend() {
        Backend::Sqlite => {
            let raw = sqlx::query(
                "SELECT id, origin, key, issued_at, received_at, signals
                   FROM taste_passports WHERE account_id = ? ORDER BY received_at DESC",
            )
            .bind(account)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            raw.iter()
                .map(|row| Recorded {
                    id: row.get("id"),
                    origin: row.get("origin"),
                    key: row.get("key"),
                    issued_at: row.get("issued_at"),
                    received_at: row.get("received_at"),
                    signals: row.get("signals"),
                })
                .collect::<Vec<_>>()
        }
        Backend::Postgres => {
            let raw = sqlx::query(
                "SELECT id, origin, key, issued_at, received_at, CAST(signals AS BIGINT) AS signals
                   FROM taste_passports WHERE account_id = $1 ORDER BY received_at DESC",
            )
            .bind(account)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            raw.iter()
                .map(|row| Recorded {
                    id: row.get("id"),
                    origin: row.get("origin"),
                    key: row.get("key"),
                    issued_at: row.get("issued_at"),
                    received_at: row.get("received_at"),
                    signals: row.get("signals"),
                })
                .collect::<Vec<_>>()
        }
    };
    Ok(rows)
}

/// Withdraw a passport, and the taste it seeded with it.
///
/// Revocation is a delete of both, because a receipt without its signals would claim an import that is not there and signals
/// without their receipt would be evidence with no provenance — and provenance is the whole reason they are marked imported.
pub async fn revoke(db: &Database, account: &str, origin: &str) -> Result<bool> {
    let source_key = format!("passport:{origin}");
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query("DELETE FROM taste_signals WHERE account_id = ? AND source_key = ? AND origin = 'imported'")
                .bind(account)
                .bind(&source_key)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
            let removed = sqlx::query("DELETE FROM taste_passports WHERE account_id = ? AND origin = ?")
                .bind(account)
                .bind(origin)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?
                .rows_affected();
            i64::try_from(removed).unwrap_or(0)
        }
        Backend::Postgres => {
            sqlx::query("DELETE FROM taste_signals WHERE account_id = $1::uuid AND source_key = $2 AND origin = 'imported'")
                .bind(account)
                .bind(&source_key)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
            let removed = sqlx::query("DELETE FROM taste_passports WHERE account_id = $1 AND origin = $2")
                .bind(account)
                .bind(origin)
                .execute(db.postgres_pool().expect("postgres"))
                .await?
                .rows_affected();
            i64::try_from(removed).unwrap_or(0)
        }
    };
    Ok(affected > 0)
}

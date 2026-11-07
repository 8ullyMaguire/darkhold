//! M69-02 — peer fingerprints as a peer published them: stored whole, read back as the same document.
//!
//! The document is kept as JSON rather than shredded into columns because it is a **claim** a peer made at a moment, in
//! the shape that peer's own build emitted. A reader that could reconstruct this instance's fingerprint from columns and
//! a peer's only from a document would be two readers, and the comparison between them would be between two
//! implementations rather than between two instances.
//!
//! Nothing here fetches. An operator (or a future scheduler) supplies the document; `record` keeps it, keyed by the
//! instance id the document itself declares — not by a host someone typed, because a document that names a different
//! instance than the one it was recorded under is a mis-filing waiting to happen.

use anyhow::{Context, Result};
use sqlx::Row as _;

use darkhold_domain::taste_fingerprint::Fingerprint;

use crate::{Backend, Database};

/// Store a peer's published fingerprint, replacing an older copy of the same instance.
///
/// Replacing rather than appending: a fingerprint is a statement about now, and two copies would leave the operator
/// comparing against a stale one without knowing which. The `generated_at` inside the document is the peer's own claim
/// about when the numbers were computed; `fetched_at` is when this instance saw it, and the two answer different
/// questions (a peer that has not rebuilt in a month is a peer whose taste may have moved).
pub async fn record(db: &Database, document: &str, fetched_at: &str) -> Result<Fingerprint> {
    let fingerprint: Fingerprint =
        serde_json::from_str(document).context("a peer fingerprint is a JSON document")?;
    if fingerprint.instance_id.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "a peer fingerprint must name its own instance; this one is anonymous"
        ));
    }

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO peer_fingerprints (instance_id, document, fetched_at) VALUES (?, ?, ?)
                 ON CONFLICT(instance_id) DO UPDATE SET
                    document = excluded.document,
                    fetched_at = excluded.fetched_at",
            )
            .bind(&fingerprint.instance_id)
            .bind(document)
            .bind(fetched_at)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO peer_fingerprints (instance_id, document, fetched_at) VALUES ($1, $2, $3)
                 ON CONFLICT(instance_id) DO UPDATE SET
                    document = EXCLUDED.document,
                    fetched_at = EXCLUDED.fetched_at",
            )
            .bind(&fingerprint.instance_id)
            .bind(document)
            .bind(fetched_at)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(fingerprint)
}

/// Every stored peer fingerprint, with when this instance saw it.
///
/// The projection is written per arm: `SqliteRow` and `PgRow` are different types, so a shared closure would need `Decode`
/// bounds the two databases do not unify (the same rule as `two-engine-store-tests`).
pub async fn list(db: &Database) -> Result<Vec<(Fingerprint, String)>> {
    /// A row that no longer parses is skipped rather than failing the whole list: one peer that changed its document shape
    /// must not take the operator's peering page down with it.
    fn parse(document: &str, fetched_at: String) -> Option<(Fingerprint, String)> {
        match serde_json::from_str::<Fingerprint>(document) {
            Ok(fingerprint) => Some((fingerprint, fetched_at)),
            Err(error) => {
                tracing::warn!(%error, "a stored peer fingerprint does not parse; skipped");
                None
            }
        }
    }

    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT document, fetched_at FROM peer_fingerprints ORDER BY instance_id",
            )
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows
                .iter()
                .filter_map(|row| {
                    parse(
                        &row.get::<String, _>("document"),
                        row.get::<String, _>("fetched_at"),
                    )
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT document, fetched_at FROM peer_fingerprints ORDER BY instance_id",
            )
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows
                .iter()
                .filter_map(|row| {
                    parse(
                        &row.get::<String, _>("document"),
                        row.get::<String, _>("fetched_at"),
                    )
                })
                .collect())
        }
    }
}

/// Remove a peer's fingerprint, so the operator can say "stop suggesting this one".
pub async fn forget(db: &Database, instance_id: &str) -> Result<bool> {
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query("DELETE FROM peer_fingerprints WHERE instance_id = ?")
            .bind(instance_id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query("DELETE FROM peer_fingerprints WHERE instance_id = $1")
            .bind(instance_id)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

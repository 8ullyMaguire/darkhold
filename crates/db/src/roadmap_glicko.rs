//! M69-10 — Glicko-2 positions for the roadmap arena.
//!
//! The arithmetic is `darkhold_domain::glicko`'s; this stores what it produced so "provisional" survives a restart. A card
//! with no row here has never been rated under Glicko-2 and is *adopted* at its Elo rating with maximum deviation — not at a
//! small one, because an Elo number is a position without an uncertainty, and inventing an uncertainty is how a two-ballot
//! card starts claiming to be settled.

use anyhow::Result;
use sqlx::Row as _;

use darkhold_domain::glicko::Rating;

use crate::{Backend, Database};

/// The stored numbers for one card.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// The rating deviation, in rating points.
    pub deviation: f64,
    /// The volatility.
    pub volatility: f64,
    /// Whether the last ballot that moved this card inverted a 300-point gap.
    pub diverged: bool,
}

/// Every stored position, keyed by card id.
pub async fn load_all(db: &Database) -> Result<std::collections::BTreeMap<String, Position>> {
    let mut out = std::collections::BTreeMap::new();
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT card_id, deviation, volatility, diverged FROM roadmap_card_glicko",
            )
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            for row in rows {
                out.insert(
                    row.get::<String, _>("card_id"),
                    Position {
                        deviation: row.get("deviation"),
                        volatility: row.get("volatility"),
                        diverged: row.get::<i64, _>("diverged") != 0,
                    },
                );
            }
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT card_id, deviation, volatility, diverged FROM roadmap_card_glicko",
            )
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            for row in rows {
                out.insert(
                    row.get::<String, _>("card_id"),
                    Position {
                        deviation: row.get("deviation"),
                        volatility: row.get("volatility"),
                        diverged: row.get::<bool, _>("diverged"),
                    },
                );
            }
        }
    }
    Ok(out)
}

/// One card's stored position, if it has one.
pub async fn load(db: &Database, card_id: &str) -> Result<Option<Position>> {
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(
                "SELECT deviation, volatility, diverged FROM roadmap_card_glicko WHERE card_id = ?",
            )
            .bind(card_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.map(|row| Position {
                deviation: row.get("deviation"),
                volatility: row.get("volatility"),
                diverged: row.get::<i64, _>("diverged") != 0,
            }))
        }
        Backend::Postgres => {
            let row = sqlx::query(
                "SELECT deviation, volatility, diverged FROM roadmap_card_glicko WHERE card_id = $1",
            )
            .bind(card_id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.map(|row| Position {
                deviation: row.get("deviation"),
                volatility: row.get("volatility"),
                diverged: row.get::<bool, _>("diverged"),
            }))
        }
    }
}

/// Store one card's position after a ballot.
///
/// `diverged` is written, not accumulated: the flag describes the last ballot that moved the card, so a card that diverged
/// once and then settled stops being flagged. A stored "ever diverged" would mark a card forever for one reader's opinion.
pub async fn save(
    db: &Database,
    card_id: &str,
    rating: Rating,
    diverged: bool,
    now: &str,
) -> Result<()> {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO roadmap_card_glicko (card_id, deviation, volatility, diverged, updated_at)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT (card_id) DO UPDATE SET
                     deviation = excluded.deviation,
                     volatility = excluded.volatility,
                     diverged = excluded.diverged,
                     updated_at = excluded.updated_at",
            )
            .bind(card_id)
            .bind(rating.deviation)
            .bind(rating.volatility)
            .bind(i64::from(diverged))
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO roadmap_card_glicko (card_id, deviation, volatility, diverged, updated_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (card_id) DO UPDATE SET
                     deviation = excluded.deviation,
                     volatility = excluded.volatility,
                     diverged = excluded.diverged,
                     updated_at = excluded.updated_at",
            )
            .bind(card_id)
            .bind(rating.deviation)
            .bind(rating.volatility)
            .bind(diverged)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(())
}

/// The rating a card enters a ballot with: its stored numbers, or its Elo rating adopted at maximum uncertainty.
#[must_use]
pub fn position_for(elo_rating: f64, stored: Option<Position>) -> Rating {
    match stored {
        Some(position) => Rating {
            rating: elo_rating,
            deviation: position.deviation,
            volatility: position.volatility,
        },
        None => Rating::adopted(elo_rating),
    }
}
